use crate::caching::backend::CachingBackend;
use anyhow::Result;

use crate::iohashing::{HashBundle, OutputHashBundle};

pub struct StdioBackend {
    pub verbose_output: bool,
    pub capsule_id: String,
}

impl CachingBackend for StdioBackend {
    fn name(&self) -> &'static str {
        return "stdio";
    }

    #[allow(unused_variables)]
    fn write(&self, inputs_bundle: &HashBundle, output_bundle: &OutputHashBundle) -> Result<()> {
        println!(
            "Capsule ID: '{}'. Inputs key: '{}'",
            self.capsule_id,
            inputs_bundle.hash
        );
        if self.verbose_output {
            println!("  Capsule Inputs hashes: {:?}", inputs_bundle.hash_details);
        }
        Ok(())
    }
}
