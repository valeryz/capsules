use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::os::unix::prelude::OsStrExt;
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use cargo::core::compiler::{BuildContext, UnitInterner};
use cargo::core::shell::Shell;
use cargo::core::Source;
use cargo::ops;
use cargo::util::command_prelude::*;
use cargo::util::config;

use sha2::{Digest, Sha256};

// Accept a subset of cargo test options.
// Copied with minor modifications from cargo/src/bin/cargo/commands/test.rs
// Additionally, includes the argument --capsule_id to pass to the capsule call.
fn create_clap_app() -> App {
    App::new("cargo-capsule-test")
        .settings(&[
            AppSettings::UnifiedHelpMessage,
            AppSettings::DeriveDisplayOrder,
            AppSettings::DontCollapseArgsInUsage,
        ])
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::with_name("args")
                .help("Arguments for the test binary")
                .multiple(true)
                .last(true),
        )
        .arg(
            opt("capsule_id", "Set the capsule ID for the call")
                .value_name("CAPSULE_ID")
                .short("c")
                .required(true),
        )
        .arg(opt("quiet", "Display one character per test instead of one line").short("q"))
        .arg(opt("doc", "Test only this library's documentation"))
        .arg(opt("no-run", "Compile, but don't run tests"))
        .arg(opt("no-fail-fast", "Run all tests regardless of failure"))
        .arg_targets_all(
            "Test only this package's library unit tests",
            "Test only the specified binary",
            "Test all binaries",
            "Test only the specified example",
            "Test all examples",
            "Test only the specified test target",
            "Test all tests",
            "Test only the specified bench target",
            "Test all benches",
            "Test all targets",
        )
        .arg_package_spec(
            "Package to run tests for",
            "Test all packages in the workspace",
            "Exclude packages from the test",
        )
        .arg_jobs()
        .arg_release("Build artifacts in release mode, with optimizations")
        .arg_profile("Build artifacts with the specified profile")
        .arg_features()
        .arg_target_triple("Build for the target triple")
        .arg_target_dir()
        .arg_manifest_path()
        .arg_ignore_rust_version()
        .arg_message_format()
        .after_help("Run `cargo help test` for more detailed information.\n")
}

// Args should match the ones specified in create_clap_app.
fn find_args_to_pass(orig_args: &ArgMatches) -> Vec<OsString> {
    let mut args = Vec::new();
    // All flag arguments.
    for opt_arg in [
        "quiet",
        "doc",
        "no-run",
        "no-fail-fast",
        "release",
        "ignore-rust-version",
        "lib",
        "bins",
        "examples",
        "tests",
        "benches",
        "all-targets",
    ] {
        if orig_args.is_present(opt_arg) {
            args.push(format!("--{}", opt_arg).into());
        }
    }
    // All single or multiple args.
    for opt_arg in [
        "bin",
        "example",
        "test",
        "bench",
        "features",
        "target",
        "target-dir",
        "manifest-path",
        "message-format",
    ] {
        if orig_args.is_present(opt_arg) {
            args.push(format!("--{}", opt_arg).into());
            args.extend(orig_args.values_of(opt_arg).unwrap().map(Into::into));
        }
    }
    args
}

fn args_hash(args: &[OsString]) -> String {
    let mut acc = Sha256::new();
    for arg in args {
        acc.update(arg.as_bytes());
    }
    format!("{:x}", acc.finalize())
}

fn exec(config: &mut Config) -> CliResult {
    let app = create_clap_app();
    let args = app.get_matches_safe()?;
    let ws = args.workspace(&config)?;

    let pass_args = find_args_to_pass(&args);
    let capsule_id = args.value_of("capsule_id").expect("Capsule ID unknown");

    let compile_opts = args.compile_options(config, CompileMode::Test, Some(&ws), ProfileChecking::Custom)?;

    // let test_args: Vec<&'static str> = vec![];

    // println!("Workspace: \n{:?}\n", ws);

    let interner = UnitInterner::new();

    // Create the build context - a structure that understands compile opts and the workspace, and builds
    // the cargo unit graph.
    let BuildContext {
        ref roots,
        ref unit_graph,
        ..
    } = ops::create_bcx(&ws, &compile_opts, &interner)?;

    // let _ = unit_graph::emit_serialized_unit_graph(&roots, &unit_graph, ws.config())?;

    // Look at each 'root'. For each root, find all its transitive deps.
    let empty_deps = Vec::new();
    for root in roots {
        let mut deps: Vec<_> = unit_graph
            .get(root)
            .unwrap_or(&empty_deps)
            .iter()
            .map(|unit_dep| &unit_dep.unit)
            .collect();
        deps.push(&root);

        // For the transitive deps that are outside the workspace, represent them as tool tags.
        // for the deps that are inside the workspace, find all their sources, and include as -i.
        // Call cargo test -p 'target' under capsule with all these inputs.
        let inputs: HashSet<(String, String)> = deps
            .iter()
            .map(|dep| -> Result<Vec<(String, String)>> {
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
            .flatten()
            .collect();

        // Modify capsule-id to include a specific root.
        let capsule_id = format!("{}-{}", capsule_id, root.pkg);
        let pass_args_tool_tag = ["-t".to_string(), args_hash(&pass_args)];
        let capsule_args = inputs
            .iter()
            .map(|(a, b)| [a, b])
            .flatten()
            .chain(pass_args_tool_tag.iter());
        // println!(
        //     "Inputs for {:?} : {:?}\n\n",
        //     root,
        //     inputs.iter().flattenmap(|(a, b)| format!("{} {} ", a, b)).collect::<Vec<_>>()
        // );
        Command::new("capsule")
            .arg("-c")
            .arg(capsule_id)
            .args(capsule_args)
            .arg("--")
            .arg("cargo")
            .arg("test")
            .args(&pass_args)
            .spawn()
            .context("Spawning cargo test")?
            .wait()
            .context("Waiting for cargo test")?;
    }

    Ok(())

    // let ops = ops::TestOptions {
    //     no_run: false,
    //     no_fail_fast: false,
    //     compile_opts: compile_opts,
    // };

    // let err = ops::run_tests(&ws, &ops, &test_args)?;

    // match err {
    //     None => Ok(()),
    //     Some(err) => {
    //         let context = anyhow::format_err!("{}", err.hint(&ws, &ops.compile_opts));
    //         let e = match err.code {
    //             // Don't show "process didn't exit successfully" for simple errors.
    //             Some(i) if cargo_util::is_simple_exit_code(i) => CliError::new(context, i),
    //             Some(i) => CliError::new(Error::from(err).context(context), i),
    //             None => CliError::new(Error::from(err).context(context), 101),
    //         };
    //         Err(e.into())
    //     }
    // }
}

fn main() {
    let mut config = match config::Config::default() {
        Ok(cfg) => cfg,
        Err(e) => {
            let mut shell = Shell::new();
            cargo::exit_with_error(e.into(), &mut shell)
        }
    };

    let result = exec(&mut config);
    match result {
        Err(e) => cargo::exit_with_error(e, &mut *config.shell()),
        Ok(()) => {}
    }
}
