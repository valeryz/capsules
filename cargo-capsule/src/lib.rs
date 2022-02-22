use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path};
use std::process::Command;

use anyhow::{Context, Result};

use cargo::core::compiler::{CompileKind, FileFlavor, unit_graph, UnitInterner};
use cargo::core::shell::Shell;
use cargo::core::{Source, TargetKind};
use cargo::util::command_prelude::*;
use cargo::util::config;
use cargo::CliResult;
use cargo::ops;

use log::Level::Debug;
use log::{debug, info, log_enabled};

use sha2::{Digest, Sha256};

fn args_hash(args: &[OsString]) -> String {
    let mut acc = Sha256::new();
    for arg in args {
        acc.update(arg.as_bytes());
    }
    format!("{:x}", acc.finalize())
}

fn normalize_file(file: &Path, workspace_root: &Option<&str>) -> String {
    if let Some(root) = workspace_root {
        match file.strip_prefix(root) {
            Ok(path) => format!("//{}", path.display()),
            Err(_) => file.to_string_lossy().to_string(),
        }
    } else {
        file.to_string_lossy().to_string()
    }
}

pub trait CargoCapsuleCommand {
    fn command(&self) -> &'static str;

    fn mode(&self) -> CompileMode;

    fn binary_outputs(&self) -> bool {
        false
    }

    fn create_clap_app(&self) -> App;

    fn find_args_to_pass(&self, orig_args: &ArgMatches) -> Vec<OsString>;

    fn exec(&self, config: &mut Config) -> CliResult {
        let app = self.create_clap_app();
        let args = app.get_matches_from_safe(std::env::args_os().skip(1))?;
        let ws = args.workspace(config)?;
        let workspace_root = args.value_of("workspace_root");

        let pass_args = self.find_args_to_pass(&args);
        let capsule_id = args.value_of("capsule_id").expect("Capsule ID unknown");

        let mut compile_opts = args.compile_options(config, self.mode(), Some(&ws), ProfileChecking::Custom)?;

        if let Some(out_dir) = args.value_of_path("out-dir", config) {
            compile_opts.build_config.export_dir = Some(out_dir);
        } else if let Some(out_dir) = config.build_config()?.out_dir.as_ref() {
            let out_dir = out_dir.resolve_path(config);
            compile_opts.build_config.export_dir = Some(out_dir);
        }

        debug!("Workspace: \n{:?}\n", ws);

        let interner = UnitInterner::new();
        // Create the build context - a structure that understands compile opts and the workspace, and builds
        // the cargo unit graph.
        let bcx = ops::create_bcx(&ws, &compile_opts, &interner)?;

        if log_enabled!(Debug) {
            let _ = unit_graph::emit_serialized_unit_graph(&bcx.roots, &bcx.unit_graph, ws.config())?;
        }

        // We determine the paths for host and target compilations.
        // This is modeled after cargo/compiler/context/mod.rs, see prepare_units()
        let dest = bcx.profiles.get_dir_name();
        let output_host = ws.target_dir().join(&dest);
        let mut targets = HashMap::new();
        for kind in bcx.all_kinds.iter() {
            if let CompileKind::Target(target) = *kind {
                let mut output_target = ws.target_dir();
                output_target.push(target.short_name());
                output_target.push(dest);
                targets.insert(target, output_target);
            }
        }

        // Now what matters is: output_host, output_targets, export_dir, and roots,
        // like in CompilationFiles.  But we still have to find 'outputs' somewhere.

        type InputSpec = HashSet<(String, String)>;

        let mut package_inputs = HashMap::<String, InputSpec>::new();
        let empty_deps = Vec::new();
        // Look at each 'root'. For each root, find all its transitive
        // deps, and add it to the package input spec for the package
        // referred to by this root.
        for root in &bcx.roots {
            let mut deps: Vec<_> = bcx
                .unit_graph
                .get(root)
                .unwrap_or(&empty_deps)
                .iter()
                .map(|unit_dep| &unit_dep.unit)
                .collect();
            deps.push(root);

            // For the transitive deps that are outside the workspace, represent them as tool tags.
            // for the deps that are inside the workspace, find all their sources, and include as -i.
            // Call cargo <test|build> -p 'target' under capsule with all these inputs.
            let mut inputs: InputSpec = deps
                .iter()
                .flat_map(|dep| -> Result<Vec<(String, String)>> {
                    if dep.is_local() {
                        // Find all files
                        let pkg = &dep.pkg;
                        let mut src = cargo::sources::PathSource::new(pkg.root(), pkg.package_id().source_id(), config);
                        src.update()?;
                        src.list_files(pkg)?
                            .iter()
                            .map(|file| Ok(("-i".to_string(), normalize_file(file.as_path(), &workspace_root))))
                            .collect::<Result<_>>()
                    } else {
                        Ok(vec![("-t".to_string(), dep.pkg.package_id().to_string())])
                    }
                })
                .flatten()
                .collect();

            if self.binary_outputs() && *root.target.kind() == TargetKind::Bin {
                let info = bcx.target_data.info(root.kind);
                let triple = bcx.target_data.short_name(&root.kind);
                let (file_types, _) = info.rustc_outputs(root.mode, root.target.kind(), triple)?;
                for file_type in file_types {
                    if file_type.flavor == FileFlavor::Normal {
                        let suffix = file_type.output_filename(&root.target, None);
                        let file_name = match root.kind {
                            CompileKind::Host => output_host.join(suffix),
                            CompileKind::Target(target) => targets.get(&target).expect("given target").join(suffix),
                        };
                        inputs.insert((
                            "-o".to_string(),
                            normalize_file(&file_name.as_path_unlocked(), &workspace_root),
                        ));
                    }
                }
            }

            match package_inputs.entry(root.pkg.name().to_string()) {
                Entry::Occupied(mut e) => e.get_mut().extend(inputs),
                Entry::Vacant(e) => {
                    e.insert(inputs);
                }
            }
        }

        for (package, inputs) in package_inputs {
            // Modify capsule-id to include a specific root + hash of the args.
            let capsule_id = format!("{}-{}", capsule_id, package);
            let capsule_args = inputs.iter().flat_map(|(a, b)| [a, b]);

            debug!(
                "Inputs for {:?} : {:?}\n\n",
                package,
                inputs.iter().map(|(a, b)| format!("{} {}", a, b)).collect::<Vec<_>>()
            );

            // Call 'cargo test' via capsule for the given packged. If
            // nothing changed for this package, it will be cached.
            let pass_args_hash = args_hash(&pass_args);
            let mut command = Command::new("capsule");
            command
                .arg("-c")
                .arg(capsule_id);
            if let Some(root) = workspace_root {
                command.arg("-w").arg(root);
            }
            command
                .args(capsule_args)
                .args(["-t", &pass_args_hash])
                .arg("--")
                .arg("cargo")
                .arg(self.command())
                .args(["--package", &package])
                .args(&pass_args);

            info!(
                "capsule {}",
                shell_words::join(command.get_args().map(OsStr::to_string_lossy))
            );
            command
                .spawn()
                .with_context(|| format!("Spawning cargo {}", self.command()))?
                .wait()
                .with_context(|| format!("Waiting for cargo {}", self.command()))?;
        }

        Ok(())
    }
}

pub fn main_exec(build: impl CargoCapsuleCommand) {
    // Initialize logging. Default is INFO level, can be overridden in CAPSULE_LOG
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Error)
        .filter_module("cargo_capsule", log::LevelFilter::Info)
        .parse_filters(&format!(
            "cargo_capsule={}",
            std::env::var("CARGO_CAPSULE_LOG").unwrap_or("info".to_owned())
        ))
        .init();

    let mut config = match config::Config::default() {
        Ok(cfg) => cfg,
        Err(e) => {
            let mut shell = Shell::new();
            cargo::exit_with_error(e.into(), &mut shell)
        }
    };

    let result = build.exec(&mut config);
    if let Err(e) = result {
        cargo::exit_with_error(e, &mut *config.shell())
    }
}
