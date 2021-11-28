use anyhow::Result;
use async_trait::async_trait;
use std::fmt;

use crate::iohashing::{HashBundle, InputOutputBundle, OutputHashBundle};

#[async_trait]
pub trait CachingBackend {
    fn name(&self) -> &'static str {
        "backend"
    }

    async fn lookup(&self, inputs: &HashBundle) -> Result<Option<InputOutputBundle>>;

    /// Read all output files from S3, and place them into destination paths.
    async fn read_files(&self, outputs: &OutputHashBundle) -> Result<()>;

    async fn write(&self, inputs: &HashBundle, outputs: &OutputHashBundle) -> Result<()>;

    async fn write_files(&self, outputs: &OutputHashBundle) -> Result<()>;
}

impl fmt::Debug for dyn CachingBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Backend: {}", self.name())
    }
}
