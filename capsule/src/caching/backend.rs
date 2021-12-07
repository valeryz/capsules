use anyhow::Result;
use tokio::io::{AsyncRead};
use async_trait::async_trait;
use std::fmt;
use std::pin::Pin;

use crate::iohashing::{InputHashBundle, InputOutputBundle, OutputHashBundle};

#[async_trait]
pub trait CachingBackend {
    /// Return the name of this backend.
    fn name(&self) -> &'static str {
        "backend"
    }

    /// Lookup the cache by the inputs hash, and return Some result if there's cache hit.
    async fn lookup(&self, inputs: &InputHashBundle) -> Result<Option<InputOutputBundle>>;

    /// Read a file addressed by item_hash from the backend storage, and return an AsyncRead handle
    /// that allows the caller to keep asynchrnously fetching the content.
    async fn read_object_file(&self, item_hash: &str) -> Result<Pin<Box<dyn AsyncRead>>>;

    async fn write(&self, inputs: &InputHashBundle, outputs: &OutputHashBundle) -> Result<()>;

    async fn write_files(&self, outputs: &OutputHashBundle) -> Result<()>;
}

impl fmt::Debug for dyn CachingBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Backend: {}", self.name())
    }
}
