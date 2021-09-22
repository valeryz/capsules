use anyhow::Result;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::fs::File;
use std::io::Read;

#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub enum Input {
    File(OsString), // input file
    Tool(OsString), // string uniquely defining the tool version (could be even the hash of its binary).
}

/// Input set is the set of all inputs to the build step.
pub struct InputSet {
    pub inputs: Vec<Input>, // We will always assume this vector is sorted.
}

// TODO: should we also add exec bit?
#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub struct FileOutput {
    filename: OsString,
    present: bool,
    contents: Bytes,
}

#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub enum Output {
    File(FileOutput),
    Stdout(OsString),
    Stderr(OsString),
    Log(FileOutput),
}

/// Output set is the set of all process outputs.
pub struct OutputSet {
    pub outputs: Vec<(Output, bool)>, // The bool indicates whether we store this output in the cache.
}

/// Returns the hash of the given file.
///
/// TODO(valeryz): Cache these in a parent process' memory by the
/// output of stat(2), except atime, so that we don't have to read
/// them twice during a single build process.
fn file_hash(filename: &OsString) -> Result<String> {
    const BUFSIZE: usize = 4096;
    let mut acc = Sha256::new();
    let mut f = File::open(filename)?;
    let mut buf: [u8; BUFSIZE] = [0; BUFSIZE];
    loop {
        let rd = f.read(&mut buf)?;
        if rd == 0 {
            break;
        }
        acc.update(&buf[..rd]);
    }
    Ok(format!("{:x}", acc.finalize()))
}

impl InputSet {
    pub fn hash(&self) -> Result<String> {
        // Calculate the hash of the input set independently of the order.
        let mut acc: Sha256 = Sha256::new();
        for input in &self.inputs {
            match input {
                Input::File(s) => {
                    acc.update("File");
                    acc.update(file_hash(s)?)
                }
                Input::Tool(s) => {
                    acc.update("Tool");
                    acc.update(s.clone().into_vec())
                }
            };
        }
        Ok(format!("{:x}", acc.finalize()))
    }

    pub fn add_input(&mut self, input: Input) {
        match self.inputs.binary_search(&input) {
            Ok(_) => {} // element already in vector @ `pos`
            Err(pos) => self.inputs.insert(pos, input),
        }
    }
}

struct Config {
    inputs: Vec<OsString>,
    outputs: Vec<OsString>,
}

struct Configuration {
    config_file: OsString,
}

struct CachingBackend {}

#[derive(Default)]
struct Bundle {}

struct Capsule<'a> {
    config: &'a Configuration,
    caching_backend: &'a CachingBackend,
    key: Option<String>,
    cacheable_bundle: Option<Bundle>,
}

impl<'a> Capsule<'a> {
    fn new(caching_backend: &'a CachingBackend, config: &'a Configuration) -> Self {
        Self {
            config: config,
            caching_backend,
            key: None,
            cacheable_bundle: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_set_1() {}

    #[test]
    fn test_input_set_empty() {}

    #[test]
    fn test_input_set_different_order() {}
}
