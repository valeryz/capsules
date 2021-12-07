use anyhow::anyhow;
use crate::caching::backend::CachingBackend;
use anyhow::Result;
use async_trait::async_trait;
use std::pin::Pin;
use std::collections::HashMap;
use tokio::io::AsyncRead;

use crate::iohashing::{InputHashBundle, InputOutputBundle, OutputHashBundle};

#[derive(Default)]
pub struct TestBackendConfig {
    pub failing_lookup: bool,
    pub failing_read_files: bool,
    pub failing_write: bool,
    pub failing_write_files: bool,
}

#[derive(Default)]
pub struct TestBackend {
    keys: HashMap<String, InputOutputBundle>,
    objects: HashMap<String, InputOutputBundle>,
    test_config: TestBackendConfig,
}

impl TestBackend {
    fn new(test_config: TestBackendConfig) -> Self {
        Self {
            test_config,
            ..Default::default()
        }
    }
}

#[async_trait]
impl CachingBackend for TestBackend {
    fn name(&self) -> &'static str {
        "test"
    }

    async fn lookup(&self, inputs: &InputHashBundle) -> Result<Option<InputOutputBundle>> {
        if self.test_config.failing_lookup {
            Err(anyhow!("Failed to lookup key"))
        } else {
            Ok(self.keys.get(&inputs.hash).cloned())
        }
    }

    async fn write(&self, inputs: &InputHashBundle, outputs: &OutputHashBundle) -> Result<()> {
        Ok(())
    }

    async fn download_object_file(&self, item_hash: &str) -> Result<Pin<Box<dyn AsyncRead>>> {
        Ok(Box::pin(tokio::io::empty()))
    }

    async fn upload_object_file(
        &self,
        item_hash: &str,
        file: Pin<Box<dyn AsyncRead + Send>>,
        content_length: u64,
    ) -> Result<()> {
        Ok(())
    }
}
