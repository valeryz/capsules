use crate::caching::backend::{CachingBackend, OutputsBundle};
use anyhow::Result;
use std::ffi::OsStr;

pub struct StdioBackend {}

impl CachingBackend for StdioBackend {
    #[allow(unused_variables)]
    fn write(
        &self,
        capsule_id: &OsStr,
        inputs_key: &OsStr,
        outputs_key: &OsStr,
        output_bundle: &OutputsBundle,
    ) -> Result<()> {
        println!(
            "Capsule ID: '{}'. Inputs key: '{}'",
            capsule_id.to_string_lossy(),
            inputs_key.to_string_lossy()
        );
        Ok(())
    }
}
