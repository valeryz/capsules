use anyhow::Result;
use crate::caching::backend::{CachingBackend, OutputsBundle};
use std::ffi::OsStr;

pub struct StdioBackend {
}

impl CachingBackend for HoneycombBackend {

    #[allow(unused_variables)]
    fn write(
        &self,
        capsule_id: &OsStr,
        inputs_key: &OsStr,
        outputs_key: &OsStr,
        output_bundle: &OutputsBundle,
    ) -> Result<()> {
        println!("Capsule ID: '{}'. Inputs key: '{}'", capsule_id, inputs_key);
        Ok(())
    }
}
