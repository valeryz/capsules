use anyhow::Result;
use async_trait::async_trait;
use std::fmt;
use std::pin::Pin;
use tokio::io::AsyncRead;

use crate::iohashing::{InputHashBundle, InputOutputBundle, OutputHashBundle};

#[async_trait]
pub trait CachingBackend {
    /// Return the name of this backend.
    fn name(&self) -> &'static str {
        "backend"
    }

    /// Lookup the cache by the inputs hash, and return Some result if there's cache hit.
    async fn lookup(&self, inputs: &InputHashBundle) -> Result<Option<InputOutputBundle>>;

    /// Write a cache entry keyed by input, containing hashes of outputs.
    async fn write(&self, inputs: &InputHashBundle, outputs: &OutputHashBundle, source: String) -> Result<()>;

    /// Download a file addressed by item_hash from the backend storage, and return an AsyncRead handle
    /// that allows the caller to keep asynchrnously fetching the content.
    async fn download_object_file(&self, item_hash: &str) -> Result<Pin<Box<dyn AsyncRead>>>;

    /// Upload a file addressed by item_hash to the backend storage. The file is represented by an
    /// AsyncRead handle that allows us to keep reading the file during the async upload.
    async fn upload_object_file(
        &self,
        name: String,
        item_hash: &str,
        file: Pin<Box<dyn AsyncRead + Send>>,
        content_length: u64,
    ) -> Result<()>;
}

impl fmt::Debug for dyn CachingBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Backend: {}", self.name())
    }
}
