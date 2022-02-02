use anyhow::{anyhow, Result};
use itertools;
use nix::unistd::execvp;
use std::env;
use std::ffi::CString;

static USAGE: &str = "Usage: capsule <capsule arguments ...> -- command [<arguments>]";

fn exec_program<I>(program_name: String, args: I) -> Result<()>
where
    I: Iterator<Item = String>,
{
    let program_cstring = CString::new(program_name)?;
    let args: Vec<String> = args.collect();
    eprintln!("Fallback exec'ing {:?}", args);
    let arg_cstrings = args.into_iter().map(CString::new).collect::<Result<Vec<_>, _>>()?;

    match execvp(&program_cstring, &arg_cstrings) {
        Ok(_) => unreachable!(),
        Err(error) => Err(error.into()),
    }
}

// Execute a given command transparently passing the original arguments.
pub fn exec() -> Result<()> {
    let mut args = env::args();
    let argv0 = &mut args.next();
    if argv0.is_none() {
        return Err(anyhow!(USAGE));
    }
    // Consume the rest of the arguments until we have the -- part
    for arg in args.by_ref() {
        if arg == "--" {
            break;
        }
    }

    if let Some(program_name) = args.next() {
        exec_program(program_name.clone(), itertools::chain!([program_name], args))
    } else {
        Err(anyhow!(USAGE.to_string()))
    }
}
