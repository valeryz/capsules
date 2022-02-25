use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use cargo::core::compiler::{unit_graph, CompileKind, FileFlavor, UnitInterner};
use cargo::core::shell::Shell;
use cargo::core::{Source, TargetKind};
use cargo::ops;
use cargo::util::command_prelude::*;
use cargo::util::config;
use cargo::CliResult;

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

pub fn add_standard_args(args: &mut Vec<OsString>, orig_args: &ArgMatches, spec: &PackageSpec) {
    // All single or multiple args, except "bin", "test", "bench".
    for opt_arg in [
        "features",
        "out-dir",
        "target",
        "target-dir",
        "manifest-path",
        "message-format",
        "jobs",
    ] {
        if orig_args.is_present(opt_arg) {
            for value in orig_args.values_of(opt_arg).unwrap() {
                args.push(format!("--{}", opt_arg).into());
                args.push(value.into());
            }
        }
    }

    // "bin", "test", "bench" are special because we don't want to reuse the same
    // argument values given, but limit them to the targets present in a specific package.
    let empty = Vec::new();
    for opt_arg in ["bin", "test", "bench", "example"] {
        if orig_args.is_present(opt_arg) {
            for value in spec.targets.get(opt_arg).unwrap_or(&empty) {
                args.push(format!("--{}", opt_arg).into());
                args.push(value.into());
            }
        }
    }
}

type IoSpec = HashSet<(String, String)>;

// What should we build/test for each package.
pub struct PackageSpec<'package> {
    pub io_spec: IoSpec,                              // set of -i and -o flags for capsules.
    pub targets: HashMap<&'package str, Vec<String>>, // list of targets for each node type (--bin, --test, --bench etc)
}

pub trait CargoCapsuleCommand {
    // Name of the command ('build', 'test')
    fn command(&self) -> &'static str;

    // One of the values of the CompileMode enum.
    fn mode(&self) -> CompileMode;

    // Whether this command outputs binaries (e.g. tests don't do this).
    fn binary_outputs(&self) -> bool {
        false
    }

    // Create command line parsing app (with clap crate).
    fn create_clap_app(&self) -> App;

    // Find arguments to pass to child cargo calls from curren args.
    fn find_args_to_pass(&self, orig_args: &ArgMatches, spec: &PackageSpec) -> Vec<OsString>;

    // Parse the dependency graph, and make child calls to cargo under capsule.
    fn exec(&self, config: &mut Config) -> CliResult {
        let app = self.create_clap_app();
        let args = app.get_matches_from_safe(std::env::args_os().skip(1))?;
        let ws = args.workspace(config)?;
        let workspace_root = args.value_of("workspace_root");

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

        // For each package
        let mut package_specs = HashMap::<String, PackageSpec>::new();
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
            let mut io_spec: IoSpec = deps
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

            let target_kind = root.target.kind().description(); // "bin", "test", "bench", etc...
            let mut file_name: Option<String> = None;
            if self.binary_outputs() && matches!(*root.target.kind(), TargetKind::Bin) {
                let info = bcx.target_data.info(root.kind);
                let triple = bcx.target_data.short_name(&root.kind);
                let (file_types, _) = info.rustc_outputs(root.mode, root.target.kind(), triple)?;
                for file_type in file_types {
                    if file_type.flavor == FileFlavor::Normal {
                        // This will be run at most once, because there's only one "normal" file in the set.
                        let suffix = file_type.uplift_filename(&root.target);
                        file_name = Some(suffix.clone());
                        let file_name = match root.kind {
                            CompileKind::Host => output_host.join(suffix),
                            CompileKind::Target(target) => targets.get(&target).expect("given target").join(suffix),
                        };
                        io_spec.insert((
                            "-o".to_string(),
                            normalize_file(file_name.as_path_unlocked(), &workspace_root),
                        ));
                    }
                }
            }

            let target_spec_present = file_name.is_some() && ["bin", "test", "bench", "example"].contains(&target_kind);
            // Add the current unit to the package spec for the package of this unit.
            match package_specs.entry(root.pkg.name().to_string()) {
                Entry::Occupied(mut e) => {
                    let package_spec = e.get_mut();
                    package_spec.io_spec.extend(io_spec);
                    if target_spec_present {
                        package_spec
                            .targets
                            .entry(target_kind)
                            .and_modify(|e| e.push(file_name.clone().unwrap()))
                            .or_insert(vec![file_name.unwrap()]);
                    }
                }
                Entry::Vacant(e) => {
                    let mut targets = HashMap::new();
                    if target_spec_present {
                        targets.insert(target_kind, vec![file_name.unwrap()]);
                    }
                    e.insert(PackageSpec { io_spec, targets });
                }
            }
        }

        for (package, spec) in package_specs {
            // Modify capsule-id to include a specific root + hash of the args.
            let capsule_id = format!("{}-{}", capsule_id, package);
            let capsule_args = spec.io_spec.iter().flat_map(|(a, b)| [a, b]);

            let pass_args = self.find_args_to_pass(&args, &spec);

            debug!(
                "Inputs for {:?} : {:?}\n\n",
                package,
                spec.io_spec
                    .iter()
                    .map(|(a, b)| format!("{} {}", a, b))
                    .collect::<Vec<_>>()
            );

            // Call 'cargo test' via capsule for the given packged. If
            // nothing changed for this package, it will be cached.
            let pass_args_hash = args_hash(&pass_args);
            let mut command = Command::new("capsule");
            command.arg("-c").arg(capsule_id);
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
            std::env::var("CARGO_CAPSULE_LOG").unwrap_or_else(|_| "info".to_owned())
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
