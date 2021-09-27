use anyhow::Result;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use std::ffi::{OsString, OsStr};
use std::fs::File;
use std::io::Read;
use std::os::unix::prelude::OsStrExt;

use crate::config::Config;
use crate::caching::backend::{CachingBackend, OutputsBundle};

#[derive(PartialOrd, Ord, PartialEq, Eq, Debug)]
pub enum Input<'a> {
    /// Input file.
    File(&'a OsString),
    /// string uniquely defining the tool version (could be even the hash of its binary).    
    ToolTag(&'a OsString),
}

/// Input set is the set of all inputs to the build step.
#[derive(Default, Debug)]
pub struct InputSet <'a> {
    pub inputs: Vec<Input<'a>>, // We will always assume this vector is sorted.
}

// TODO: should we also add exec bit?
#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub struct FileOutput <'a> {
    filename: &'a OsString,
    present: bool,
    contents: Bytes,
}

#[derive(PartialOrd, Ord, PartialEq, Eq)]
pub enum Output<'a> {
    File(&'a FileOutput<'a>),
    Stdout(&'a OsString),
    Stderr(&'a OsString),
    Log(&'a FileOutput<'a>),
}

/// Output set is the set of all process outputs.
#[derive(Default)]
pub struct OutputSet <'a> {
    pub outputs: Vec<(Output<'a>, bool)>, // The bool indicates whether we store this output in the cache.
}

/// Returns the hash of the given file.
///
/// TODO(valeryz): Cache these in a parent process' memory by the
/// output of stat(2), except atime, so that we don't have to read
/// them twice during a single build process.
fn file_hash(filename: &OsStr) -> Result<String> {
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

fn string_hash(s: &OsStr) -> String {
    let mut acc = Sha256::new();
    acc.update(s.as_bytes());
    format!("{:x}", acc.finalize())
}

impl<'a> InputSet<'a> {

    /// Returns the HEX string of the hash of the whole input set.
    ///
    /// It does this by calculating a SHA256 hash of all SHA256 hashes
    /// of inputs (being either file or tool tag) sorted by the values
    /// of the hashes themselves.
    ///
    pub fn hash(&self) -> Result<String> {
        // Calculate the hash of the input set independently of the order.
        let mut all_hashes = Vec::new();
        for input in &self.inputs {
            match input {
                Input::File(s) => {
                    all_hashes.push(format!("File{}", file_hash(s)?));
                }
                Input::ToolTag(s) => {
                    all_hashes.push(format!("ToolTag{}", string_hash(s)));
                }
            }
        }
        all_hashes.sort();
        let mut acc: Sha256 = Sha256::new();
        for hash in all_hashes {
            acc.update(hash);
        }
        Ok(format!("{:x}", acc.finalize()))
    }

    pub fn add_input(&mut self, input: Input<'a>) {
        match self.inputs.binary_search(&input) {
            Ok(_) => {} // element already in vector @ `pos`
            Err(pos) => self.inputs.insert(pos, input),
        }
    }
}

pub struct Capsule<'a> {
    config: &'a Config,
    caching_backend: Box<dyn CachingBackend>,
    inputs: InputSet<'a>,
    outputs: OutputSet<'a>,
}

impl<'a> Capsule<'a> {
    pub fn new(config: &'a Config, caching_backend: Box<dyn CachingBackend>) -> Self {
        let mut capsule = Self {
            config,
            caching_backend,
            inputs: InputSet::default(),
            outputs: OutputSet::default(),
        };

        for file in &config.input_files {
            capsule.inputs.add_input(Input::File(file));
        }
        for tool_tag in &config.tool_tags {
            capsule.inputs.add_input(Input::ToolTag(tool_tag));
        }
        capsule
    }

    pub fn hash(&self) -> Result<String> {
        self.inputs.hash()
    }

    pub fn write_cache(&self) -> Result<()> {
        // Outputs bundle is ununsed in Placebo.
        let bundle = OutputsBundle {};
        self.caching_backend.write(self.config.capsule_id.as_ref().expect("capsule_id must be specified"),
                                   &OsString::from(self.hash()?),
                                   &OsString::from(""),
                                   &bundle)

    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    const EMPTY_SHA256 : &'static str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn file_hash_test() -> Result<()> {
        let file = NamedTempFile::new()?;
        let hash = file_hash(file.path().as_os_str())?;
        // Sha256 hash of an empty file.
        assert_eq!(hash, EMPTY_SHA256);
        Ok(())
    }

    #[test]
    fn file_hash_nonexistent() {
        assert!(file_hash(&OsString::from("/nonexistent-capsule-input")).is_err());
    }

    #[test]
    fn test_input_set_empty() {
        let input_set = InputSet::default();
        assert_eq!(input_set.hash().unwrap(), EMPTY_SHA256);
    }

    #[test]
    fn test_input_set_1() {
        let mut input_set = InputSet::default();
        let tool_tag = OsString::from("some tool_tag");
        input_set.add_input(Input::ToolTag(&tool_tag));
        let hash1 = input_set.hash().unwrap();
        assert_ne!(hash1, EMPTY_SHA256);
        let hash2 = input_set.hash().unwrap();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_input_set_different_order() {
        let mut input_set1 = InputSet::default();
        let tool_tag1 = OsString::from("some tool_tag");
        let tool_tag2 = OsString::from("another tool_tag");
        input_set1.add_input(Input::ToolTag(&tool_tag1));
        input_set1.add_input(Input::ToolTag(&tool_tag2));
        let mut input_set2 = InputSet::default();
        input_set2.add_input(Input::ToolTag(&tool_tag2));
        input_set2.add_input(Input::ToolTag(&tool_tag1));
        assert_eq!(input_set1.hash().unwrap(),
                   input_set2.hash().unwrap());
    }

    #[test]
    fn test_input_set_file() {
        let mut file1 = NamedTempFile::new().unwrap();
        file1.write("file1".as_bytes()).unwrap();
        file1.flush().unwrap();
        let mut file2 = NamedTempFile::new().unwrap();
        file2.write("file2".as_bytes()).unwrap();
        file2.flush().unwrap();
        let mut input_set = InputSet::default();
        let path1 = OsString::from(file1.path());
        input_set.add_input(Input::File(&path1));
        // These hashes were obtained by manual manipulation files and `openssl sha256`
        assert_eq!(input_set.hash().unwrap(),
                   "f409e4c7ae76997e69556daae6139bee1f02e4f618d3da8deea10bb35b6c0ebd");
        let path2 = OsString::from(file2.path());
        input_set.add_input(Input::File(&path2));
        assert_eq!(input_set.hash().unwrap(),
                   "a282f3da61a4bc322a8d31da6d30a0e924017962acbef2f6996b81709de8cdc3");
    }
}
