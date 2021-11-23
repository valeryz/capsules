use super::logger::Logger;
use anyhow::Result;
use async_trait::async_trait;
use crate::iohashing::{HashBundle, OutputHashBundle};

pub struct Dummy;

#[async_trait]
impl Logger for Dummy {
    async fn log(
        &self,
        _inputs_bundle: &HashBundle,
        _output_bundle: &OutputHashBundle,
        _non_determinism: bool,
    ) -> Result<()> {
        Ok(())
    }
}
