use crate::caching::backend::CachingBackend;
use anyhow::Result;
use async_trait::async_trait;
use tokio::io::AsyncRead;
use std::pin::Pin;

use crate::iohashing::{InputHashBundle, InputOutputBundle, OutputHashBundle};

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

    async fn lookup(&self, _inputs: &InputHashBundle) -> Result<Option<InputOutputBundle>> {
        // Always return a cache miss.
        Ok(None)
    }

    async fn read_object_file(&self, _item_hash: &str) -> Result<Pin<Box<dyn AsyncRead>>> {
        Ok(Box::pin(tokio::io::empty()))
    }

    async fn write(&self, inputs: &InputHashBundle, outputs: &OutputHashBundle) -> Result<()> {
        println!(
            "Capsule ID: '{}'. Inputs key: '{}', Outputs key: {}",
            self.capsule_id, inputs.hash, outputs.hash,
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
