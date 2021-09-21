use libc;
use std::env;
use std::ffi::CString;
use std::ptr;
use std::io;
use anyhow::{anyhow, Result};

static USAGE: &'static str =
    "Usage: capsules-wrapper --in=<filename> ... --out=<filename>> -- command [<arguments>]";

pub fn exec_program<I>(program_name: String, args: I) -> Result<()>
where
    I: Iterator<Item = String>,
{
    let program_cstring = CString::new(program_name)?;
    let arg_cstrings = args.map(CString::new).collect::<Result<Vec<_>, _>>()?;
    let mut arg_charptrs: Vec<* const i8> = arg_cstrings.into_iter().map(|arg| arg.as_ptr()).collect();
    arg_charptrs.push(ptr::null());

    unsafe { libc::execvp(program_cstring.as_ptr(), arg_charptrs.as_ptr()) };
    Err(io::Error::last_os_error().into())
}


/// Execute a given command transparently passing the original arguments.
pub fn execute() -> Result<()> {
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
        exec_program(program_name, args)
    } else {
        Err(anyhow!(USAGE.to_string()))
    }
}
