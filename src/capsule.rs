use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use anyhow::Result;

#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub enum Input {
    File(String),               // input file
    Tool(String),               // string uniquely defining the tool version (could be even the hash of its binary).
}

/// Input set is the set of all inputs to the build step.
pub struct InputSet {
    pub inputs: Vec<Input>,  // We will always assume this vector is sorted.
}

// TODO: should we also add exec bit?
#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub struct FileOutput {
    filename: String,
    present: bool,
    contents: String
}

#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub enum Output {
    File(FileOutput),
    Stdout(String),
    Stderr(String),
    Log(FileOutput),
}

/// Output set is the set of all process outputs.
pub struct OutputSet {
    pub outputs: Vec<(Output, bool)> // The bool indicates whether we store this output in the cache.
}

/// Returns the hash of the given file.
///
/// TODO(valeryz): Cache these in a parent process' memory by the
/// output of stat(2), except atime, so that we don't have to read
/// them twice during a single build process.
fn file_hash(filename: &String) -> Result<String> {
    const BUFSIZE: usize = 4096;
    let mut acc = Sha256::new();
    let mut f = File::open(filename)?;
    let mut buf : [u8; BUFSIZE] = [0; BUFSIZE];
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
        let mut acc : Sha256 = Sha256::new();
        for input in &self.inputs {
            let payload = match input {
                Input::File(s) => ("File", file_hash(s)?),
                Input::Tool(s) => ("Tool", s.to_owned()),
            };
            acc.update(&payload.0);
            acc.update(&payload.1);
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
    inputs: Vec<String>,
    outputs: Vec<String>,
}

struct Configuration {
    config_file: String,
}

struct CachingBackend {}

#[derive(Default)]
struct Bundle {}

struct Capsule<'a> {
    config: &'a Configuration,
    caching_backend: &'a CachingBackend,
    key: Option<String>,
    cacheable_bundle: Option<Bundle>
}

impl<'a> Capsule<'a> {
    fn new(caching_backend: &'a CachingBackend,
           config: &'a Configuration) -> Self {
        Self {
            config: config,
            caching_backend,
            key: None,
            cacheable_bundle: None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_set_1() {
    }

    #[test]
    fn test_input_set_empty() {
    }

    #[test]
    fn test_input_set_different_order() {
    }
}
