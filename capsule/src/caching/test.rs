use crate::caching::backend::CachingBackend;
use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::time;

use crate::iohashing::{InputHashBundle, InputOutputBundle, OutputHashBundle};

// This config enables various kinds of failures in the test caching backend.
#[derive(Default)]
pub struct TestBackendConfig {
    pub failing_lookup: bool,
    pub failing_write: bool,
    pub failing_download_files: bool,
    pub failing_upload_files: bool,
    pub lookup_timeout: bool,
    pub write_timeout: bool,
    pub upload_timeout: bool,
    pub download_timeout: bool,
}

// We have to use Arc<RwLock<_>> for internal mutability here because
// async functions require the whole struct to be Send.
#[derive(Default)]
pub struct TestBackend {
    keys: Arc<RwLock<HashMap<String, InputOutputBundle>>>,
    objects: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    test_config: TestBackendConfig,
    capsule_id: String,
}

impl TestBackend {
    pub fn new(capsule_id: &str, test_config: TestBackendConfig) -> Self {
        Self {
            test_config,
            capsule_id: capsule_id.to_string(),
            ..Default::default()
        }
    }
    pub fn remove_all(&self) {
        let mut hashmap = self.keys.write().unwrap();
        hashmap.clear();
    }

    fn normalize_key(&self, key: &str) -> String {
        format!("{}/{}", self.capsule_id, key)
    }
}

#[async_trait]
impl CachingBackend for TestBackend {
    fn name(&self) -> &'static str {
        "test"
    }

    async fn lookup(&self, inputs: &InputHashBundle) -> Result<Option<InputOutputBundle>> {
        if self.test_config.lookup_timeout {
            time::sleep(Duration::from_millis(500)).await;
        }
        if self.test_config.failing_lookup {
            Err(anyhow!("Failed to lookup key"))
        } else {
            let key = self.normalize_key(&inputs.hash);
            let hashmap = self.keys.read().unwrap();
            Ok(hashmap.get(&key).cloned())
        }
    }

    async fn write(&self, inputs: &InputHashBundle, outputs: &OutputHashBundle, source: String) -> Result<()> {
        if self.test_config.write_timeout {
            time::sleep(Duration::from_millis(500)).await;
        }
        if self.test_config.failing_write {
            Err(anyhow!("Failed to write key"))
        } else {
            let key = self.normalize_key(&inputs.hash);
            let mut hashmap = self.keys.write().unwrap();
            hashmap.insert(
                key,
                InputOutputBundle {
                    inputs: inputs.clone(),
                    outputs: outputs.clone(),
                    source,
                },
            );
            Ok(())
        }
    }

    async fn download_object_file(&self, item_hash: &str) -> Result<Pin<Box<dyn AsyncRead>>> {
        if self.test_config.download_timeout {
            time::sleep(Duration::from_millis(500)).await;
        }
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
        name: String,
        key: &str,
        mut file: Pin<Box<dyn AsyncRead + Send>>,
        _content_length: u64,
    ) -> Result<()> {
        if self.test_config.upload_timeout {
            time::sleep(Duration::from_millis(500)).await;
        }
        if self.test_config.failing_upload_files {
            Err(anyhow!("Failed to upload object file {}", name))
        } else {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).await?;
            let mut hashmap = self.objects.write().unwrap();
            hashmap.insert(key.to_string(), buf);
            Ok(())
        }
    }
}
