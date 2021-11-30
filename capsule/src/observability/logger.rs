use crate::iohashing::{HashBundle, OutputHashBundle};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait Logger {
    async fn log(
        &self,
        inputs_bundle: &HashBundle,
        output_bundle: &OutputHashBundle,
        result_from_cache: bool,
        non_determinism: bool,
    ) -> Result<()>;
}
