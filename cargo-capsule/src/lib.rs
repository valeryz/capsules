use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use cargo::core::shell::Shell;
use cargo::util::command_prelude::*;
use cargo::util::config;
use cargo::CliResult;

use cargo::core::compiler::{unit_graph, BuildContext, UnitInterner};
use cargo::core::Source;
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

pub trait CargoCapsuleCommand {
    fn command(&self) -> &'static str;

    fn mode(&self) -> CompileMode;

    fn create_clap_app(&self) -> App;

    fn find_args_to_pass(&self, orig_args: &ArgMatches) -> Vec<OsString>;

    fn exec(&self, config: &mut Config) -> CliResult {
        let app = self.create_clap_app();
        let args = app.get_matches_from_safe(std::env::args_os().skip(1))?;
        let ws = args.workspace(config)?;

        let pass_args = self.find_args_to_pass(&args);
        let capsule_id = args.value_of("capsule_id").expect("Capsule ID unknown");

        let compile_opts = args.compile_options(config, self.mode(), Some(&ws), ProfileChecking::Custom)?;

        debug!("Workspace: \n{:?}\n", ws);

        let interner = UnitInterner::new();

        // Create the build context - a structure that understands compile opts and the workspace, and builds
        // the cargo unit graph.
        let BuildContext {
            ref roots,
            ref unit_graph,
            ..
        } = ops::create_bcx(&ws, &compile_opts, &interner)?;

        if log_enabled!(Debug) {
            let _ = unit_graph::emit_serialized_unit_graph(roots, unit_graph, ws.config())?;
        }

        type InputSpec = HashSet<(String, String)>;

        let mut package_inputs = HashMap::<String, InputSpec>::new();
        let empty_deps = Vec::new();
        // Look at each 'root'. For each root, find all its transitive
        // deps, and add it to the package input spec for the package
        // referred to by this root.
        for root in roots {
            let mut deps: Vec<_> = unit_graph
                .get(root)
                .unwrap_or(&empty_deps)
                .iter()
                .map(|unit_dep| &unit_dep.unit)
                .collect();
            deps.push(root);

            // For the transitive deps that are outside the workspace, represent them as tool tags.
            // for the deps that are inside the workspace, find all their sources, and include as -i.
            // Call cargo <test|build> -p 'target' under capsule with all these inputs.
            let inputs: InputSpec = deps
                .iter()
                .flat_map(|dep| -> Result<Vec<(String, String)>> {
                    if dep.is_local() {
                        // Find all files
                        let pkg = &dep.pkg;
                        let mut src = cargo::sources::PathSource::new(pkg.root(), pkg.package_id().source_id(), config);
                        src.update()?;
                        src.list_files(pkg)?
                            .iter()
                            .map(|file| match file.as_os_str().to_str() {
                                Some(s) => Ok(("-i".to_string(), s.to_string())),
                                None => Err(anyhow!("invalid path")),
                            })
                            .collect::<Result<_>>()
                    } else {
                        Ok(vec![("-t".to_string(), dep.pkg.package_id().to_string())])
                    }
                })
                .flatten()
                .collect();

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
                inputs.iter().map(|(a, b)| format!("{} {} ", a, b)).collect::<Vec<_>>()
            );

            // Call 'cargo test' via capsule for the given packged. If
            // nothing changed for this package, it will be cached.
            let pass_args_hash = args_hash(&pass_args);
            let mut command = Command::new("capsule");
            command
                .arg("-c")
                .arg(capsule_id)
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
        .filter_module("cargo_capsule_test", log::LevelFilter::Info)
        .parse_env("CARGO_CAPSULE_LOG")
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
