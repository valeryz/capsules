use crate::caching::backend::CachingBackend;
use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::iohashing::{InputHashBundle, InputOutputBundle, OutputHashBundle};

#[derive(Default)]
pub struct TestBackendConfig {
    pub failing_lookup: bool,
    pub failing_write: bool,
    pub failing_download_files: bool,
    pub failing_upload_files: bool,
}

// We have to use Arc<RwLock<_>> for internal mutability here because
// async functions require the whole struct to be Send.
#[derive(Default)]
pub struct TestBackend {
    keys: Arc<RwLock<HashMap<String, InputOutputBundle>>>,
    objects: Arc<RwLock<HashMap<String, Vec<u8>>>>,
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
            let hashmap = self.keys.read().unwrap();
            Ok(hashmap.get(&inputs.hash).cloned())
        }
    }

    async fn write(&self, inputs: &InputHashBundle, outputs: &OutputHashBundle) -> Result<()> {
        if self.test_config.failing_download_files {
            Err(anyhow!("Failed to write key"))
        } else {
            let mut hashmap = self.keys.write().unwrap();
            hashmap.insert(
                inputs.hash.clone(),
                InputOutputBundle {
                    inputs: inputs.clone(),
                    outputs: outputs.clone(),
                },
            );
            Ok(())
        }
    }

    async fn download_object_file(&self, item_hash: &str) -> Result<Pin<Box<dyn AsyncRead>>> {
        if self.test_config.failing_download_files {
            Err(anyhow!("Failed to download file"))
        } else {
            let hashmap = self.objects.read().unwrap();
            let object = hashmap.get(item_hash).ok_or_else(|| anyhow!("file not found"))?;
            Ok(Box::pin(std::io::Cursor::new(object.clone())))
        }
    }

    async fn upload_object_file(
        &self,
        key: &str,
        mut file: Pin<Box<dyn AsyncRead + Send>>,
        _content_length: u64,
    ) -> Result<()> {
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).await?;
        let mut hashmap = self.objects.write().unwrap();
        hashmap.insert(key.to_string(), buf);
        Ok(())
    }
}
