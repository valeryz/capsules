use std::ffi::OsString;

use cargo::util::command_prelude::*;

use cargo_capsule::main_exec;
use cargo_capsule::CargoCapsuleCommand;

// Implementaiton of the CargoCapsuleCommand trait
struct CargoCapsuleTest;

impl CargoCapsuleCommand for CargoCapsuleTest {
    fn command(&self) -> &'static str {
        "test"
    }

    fn mode(&self) -> CompileMode {
        CompileMode::Test
    }

    // Accept a subset of cargo test options.
    // Copied with minor modifications from cargo/src/bin/cargo/commands/test.rs
    // Additionally, includes the argument --capsule_id to pass to the capsule call.
    fn create_clap_app(&self) -> App {
        App::new("capsule-test")
            .settings(&[
                AppSettings::TrailingVarArg,
                AppSettings::UnifiedHelpMessage,
                AppSettings::DeriveDisplayOrder,
                AppSettings::VersionlessSubcommands,
            ])
            .setting(AppSettings::TrailingVarArg)
            .version(env!("CARGO_PKG_VERSION"))
            .arg(Arg::with_name("TESTNAME").help("If specified, only run tests containing this string in their names"))
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
            .arg(
                opt(
                    "workspace_root",
                    "If given, all paths will be normalized relative to this root",
                )
                .value_name("WORKSPACE_ROOT")
                .short("w")
                .required(false),
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
    fn find_args_to_pass(&self, orig_args: &ArgMatches) -> Vec<OsString> {
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
        // Add TESTNAME
        if let Some(testname) = orig_args.value_of("TESTNAME") {
            args.push(testname.into());
        }
        // Add all test args
        if let Some(test_args) = orig_args.values_of("args") {
            args.push("--".into());
            args.extend(test_args.map(Into::into));
        }
        args
    }
}

fn main() {
    main_exec(CargoCapsuleTest);
}
