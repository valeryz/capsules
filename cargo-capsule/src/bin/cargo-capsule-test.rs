use anyhow::{self, Error};
use cargo::ops;
use cargo::util::command_prelude::*;
use cargo::util::config;
use cargo::util::errors::CliError;
use cargo::core::shell::Shell;

use cargo_util;


fn exec(config: &mut Config) -> CliResult {
    let arg_matches = App::new("cargo-capsule-test")
        .version(env!("CARGO_PKG_VERSION"))
        .arg(Arg::with_name("wtf").help("WTF is that arg"));

    let args = arg_matches.get_matches_safe()?;
    let ws = args.workspace(&config)?;

    println!("Workspace: \n{:?}", ws);

    let mut compile_opts = args.compile_options(
        config,
        CompileMode::Test,
        Some(&ws),
        ProfileChecking::Custom,
    )?;

    let ops = ops::TestOptions {
        no_run: false,
        no_fail_fast: false,
        compile_opts: compile_opts,
    };

    let test_args: Vec<&'static str> = vec![];

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
