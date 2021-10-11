use crate::caching::backend::{CachingBackend, OutputsBundle};
use anyhow::Result;
use std::ffi::OsStr;

use crate::iohashing::HashBundle;

pub struct StdioBackend {}

impl CachingBackend for StdioBackend {
    fn name(&self) -> &'static str { return "stdio"; }
    
    #[allow(unused_variables)]
    fn write(
        &self,
        capsule_id: &OsStr,
        inputs_bundle: &HashBundle,
        outputs_key: &OsStr,
        output_bundle: &OutputsBundle,
    ) -> Result<()> {
        println!(
            "Capsule ID: '{}'. Inputs key: '{}'\n  Capsule Inputs hashes: {:?}",
            capsule_id.to_string_lossy(),
            inputs_bundle.hash,
            inputs_bundle.input_hashes
        );
        Ok(())
    }
}
