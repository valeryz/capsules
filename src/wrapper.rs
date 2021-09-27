use std::env;
use std::ffi::{OsString, CString};
use std::os::unix::ffi::OsStringExt;
use itertools;
use anyhow::{anyhow, Result};
use nix::unistd::execvp;


/// Execute a given command transparently passing the original arguments.
pub fn execute(command: &Vec<OsString>) -> Result<()> {
    let program_cstring = CString::new(command[0].clone().into_vec())?;
    let arg_cstrings = command.iter().map(|x| CString::new(x.clone().into_vec())).collect::<Result<Vec<_>, _>>()?;

    match execvp(&program_cstring, &arg_cstrings) {
        Ok(_) => unreachable!(),
        Err(error) => Err(error.into())
    }
}
