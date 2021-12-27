use std::collections::HashSet;

use anyhow::{anyhow, Result, Error};
use cargo::core::Source;
use cargo::ops;
use cargo::util::command_prelude::*;
use cargo::util::config;
use cargo::util::errors::CliError;
use cargo::core::shell::Shell;
use cargo::core::compiler::{UnitInterner, unit_graph, BuildContext};

use cargo_util;


fn exec(config: &mut Config) -> CliResult {
    // TODO: accept compilation options compatible with cargo test
    let arg_matches = App::new("cargo-capsule-test")
        .version(env!("CARGO_PKG_VERSION"))
        .arg(Arg::with_name("wtf").help("WTF is that arg"));

    let args = arg_matches.get_matches_safe()?;
    let ws = args.workspace(&config)?;

    let mut compile_opts = args.compile_options(
        config,
        CompileMode::Test,
        Some(&ws),
        ProfileChecking::Custom,
    )?;

    let test_args: Vec<&'static str> = vec![];

    println!("Workspace: \n{:?}\n", ws);

    let interner = UnitInterner::new();
    let BuildContext { ref roots, ref unit_graph, .. } = ops::create_bcx(&ws, &compile_opts, &interner)?;

    let _ = unit_graph::emit_serialized_unit_graph(&roots, &unit_graph, ws.config())?;

    // Look at each 'root'. For each root, find all its transitive deps.
    // For the transitive deps that are outside the workspace, represent them as tool tags.
    // for the deps that are inside the workspace, find all their sources, and include as -i.
    // Call cargo test -p 'target' under capsule with all these inputs.
    
    let empty_deps = Vec::new();
    for root in roots {
        let mut deps: Vec<_> = unit_graph.get(root).unwrap_or(&empty_deps).iter().map(|unit_dep| &unit_dep.unit).collect();
        deps.push(&root);

        let inputs : HashSet<(&str, String)> = deps.iter().map(|dep| -> Result<Vec<(&str, String)>> {
            if dep.is_local() {
                // Find all files
                let pkg = &dep.pkg;
                let mut src = cargo::sources::PathSource::new(pkg.root(), pkg.package_id().source_id(), config);
                src.update()?;
                src.list_files(pkg)?.iter().map(
                    |file| 
                        match file.as_os_str().to_str() {
                            Some(s) => Ok(("-i", s.to_string())),
                            None => Err(anyhow!("invalid path"))
                        }).collect::<Result<_>>()
            } else {
                Ok(vec![("-t", dep.pkg.package_id().to_string())])
            }
        }).flatten().flatten().collect();

        println!("Inputs for {:?} : {:?}\n\n", root, inputs.iter().map(|(a, b)| format!("{} {} ", a, b)).collect::<Vec<_>>());
        
    }

    let ops = ops::TestOptions {
        no_run: false,
        no_fail_fast: false,
        compile_opts: compile_opts,
    };

    let err = ops::run_tests(&ws, &ops, &test_args)?;
    match err {
        None => Ok(()),
        Some(err) => {
            let context = anyhow::format_err!("{}", err.hint(&ws, &ops.compile_opts));
            let e = match err.code {
                // Don't show "process didn't exit successfully" for simple errors.
                Some(i) if cargo_util::is_simple_exit_code(i) => CliError::new(context, i),
                Some(i) => CliError::new(Error::from(err).context(context), i),
                None => CliError::new(Error::from(err).context(context), 101),
            };
            Err(e.into())
        }
    }
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
