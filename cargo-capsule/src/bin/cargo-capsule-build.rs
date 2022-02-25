use std::ffi::OsString;

use cargo::util::command_prelude::*;

use cargo_capsule::{PackageSpec, CargoCapsuleCommand, add_standard_args, main_exec};

// Implementaiton of the CargoCapsuleCommand trait
struct CargoCapsuleBuild;

impl CargoCapsuleCommand for CargoCapsuleBuild {
    fn command(&self) -> &'static str {
        "build"
    }

    fn mode(&self) -> CompileMode {
        CompileMode::Build
    }

    fn binary_outputs(&self) -> bool {
        true
    }

    // Accept a subset of cargo build
    // Copied from cargo/src/bin/cargo/commands/build.rs
    // Additionally, includes the argument --capsule_id to pass to the capsule call.
    fn create_clap_app(&self) -> App {
        App::new("capsule-build")
            .about("Compile a local package and all of its dependencies")
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
            .arg(opt("quiet", "No output printed to stdout").short("q"))
            .arg_package_spec(
                "Package to build (see `cargo help pkgid`)",
                "Build all packages in the workspace",
                "Exclude packages from the build",
            )
            .arg_jobs()
            .arg_targets_all(
                "Build only this package's library",
                "Build only the specified binary",
                "Build all binaries",
                "Build only the specified example",
                "Build all examples",
                "Build only the specified test target",
                "Build all tests",
                "Build only the specified bench target",
                "Build all benches",
                "Build all targets",
            )
            .arg_release("Build artifacts in release mode, with optimizations")
            .arg_profile("Build artifacts with the specified profile")
            .arg_features()
            .arg_target_triple("Build for the target triple")
            .arg_target_dir()
            .arg(opt("out-dir", "Copy final artifacts to this directory (unstable)").value_name("PATH"))
            .arg_manifest_path()
            .arg_ignore_rust_version()
            .arg_message_format()
            .arg_build_plan()
            .arg_unit_graph()
            .arg_future_incompat_report()
            .after_help("Run `cargo help build` for more detailed information.\n")
    }

    // Args should match the ones specified in create_clap_app.
    fn find_args_to_pass(&self, orig_args: &ArgMatches, spec: &PackageSpec) -> Vec<OsString> {
        let mut args = Vec::new();
        // All flag arguments, except target selection arguments.
        for opt_arg in [
            "quiet",
            "doc",
            "release",
            "ignore-rust-version",
            "lib",
            "bins",
            "examples",
            "tests",
            "benches",
            "all-targets",
            "all-features",
            "no-default-features",
            "profile",
            "frozen",
            "locked",
            "offline",
            "build-plan",
        ] {
            if orig_args.is_present(opt_arg) {
                args.push(format!("--{}", opt_arg).into());
            }
        }

        add_standard_args(&mut args, &orig_args, &spec);

        args
    }
}

fn main() {
    main_exec(CargoCapsuleBuild)
}
