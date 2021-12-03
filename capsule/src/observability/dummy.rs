use super::logger::Logger;
use crate::iohashing::{InputHashBundle, OutputHashBundle};
use anyhow::Result;
use async_trait::async_trait;

pub struct Dummy;

#[async_trait]
impl Logger for Dummy {
    async fn log(
        &self,
        _inputs_bundle: &InputHashBundle,
        _output_bundle: &OutputHashBundle,
        _result_from_cache: bool,
        _non_determinism: bool,
    ) -> Result<()> {
        Ok(())
    }
}
