use crate::caching::backend::CachingBackend;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::pin::Pin;
use tokio::io::AsyncRead;

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

    async fn download_object_file(&self, _item_hash: &str) -> Result<Pin<Box<dyn AsyncRead>>> {
        Err(anyhow!("downloading object file in the dummy backend"))
    }

    async fn upload_object_file(
        &self,
        _item_hash: &str,
        _file: Pin<Box<dyn AsyncRead + Send>>,
        _content_length: u64,
    ) -> Result<()> {
        Ok(())
    }

    async fn write(&self, inputs: &InputHashBundle, outputs: &OutputHashBundle, source: String) -> Result<()> {
        println!(
            "Capsule ID: '{}'. Capsule Source: '{}', Inputs key: '{}', Outputs key: {}",
            self.capsule_id, source, inputs.hash, outputs.hash,
        );
        if self.verbose_output {
            println!("  Capsule Inputs hashes: {:?}", inputs.hash_details);
            println!("  Capsule Outputs hashes: {:?}", outputs.hash_details);
        }
        Ok(())
    }
}
