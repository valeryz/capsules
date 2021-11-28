use crate::caching::backend::CachingBackend;
use anyhow::Result;
use async_trait::async_trait;

use crate::iohashing::{HashBundle, OutputHashBundle, InputOutputBundle};

#[derive(Default)]
pub struct DummyBackend {
    pub verbose_output: bool,
    pub capsule_id: String,
}

#[async_trait]
impl CachingBackend for DummyBackend {
    fn name(&self) -> &'static str {
        "dummy"
    }

    async fn lookup(&self, _inputs: &HashBundle) -> Result<Option<InputOutputBundle>> {
        // Always return a cache miss.
        Ok(None)
    }

    /// Read all output files from S3, and place them into destination paths.
    async fn read_files(&self, _outputs: &OutputHashBundle) -> Result<()> {
        Ok(())
    }

    async fn write(&self, inputs: &HashBundle, outputs: &OutputHashBundle) -> Result<()> {
        println!(
            "Capsule ID: '{}'. Inputs key: '{}', Outputs key: {}",
            self.capsule_id,
            inputs.hash,
            outputs.hash,
        );
        if self.verbose_output {
            println!("  Capsule Inputs hashes: {:?}", inputs.hash_details);
            println!("  Capsule Outputs hashes: {:?}", outputs.hash_details);
        }
        Ok(())
    }

    async fn write_files(&self, _outputs: &OutputHashBundle) -> Result<()> {
        Ok(())
    }
}
