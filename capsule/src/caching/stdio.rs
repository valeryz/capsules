use crate::caching::backend::{CachingBackend, OutputsBundle};
use anyhow::Result;
use std::ffi::OsStr;

use crate::iohashing::HashBundle;

pub struct StdioBackend {
    pub verbose_output: bool,
}

impl CachingBackend for StdioBackend {
    fn name(&self) -> &'static str {
        return "stdio";
    }

    #[allow(unused_variables)]
    fn write(
        &self,
        capsule_id: &OsStr,
        inputs_bundle: &HashBundle,
        outputs_key: &OsStr,
        output_bundle: &OutputsBundle,
    ) -> Result<()> {
        println!(
            "Capsule ID: '{}'. Inputs key: '{}'",
            capsule_id.to_string_lossy(),
            inputs_bundle.hash
        );
        if self.verbose_output {
            println!("  Capsule Inputs hashes: {:?}", inputs_bundle.hash_details);
        }
        Ok(())
    }
}
