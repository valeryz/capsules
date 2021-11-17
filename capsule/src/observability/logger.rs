use crate::iohashing::{HashBundle, OutputHashBundle};
use anyhow::Result;

pub trait Logger {
    fn log(
        &self,
        _inputs_bundle: &HashBundle,
        _output_bundle: &OutputHashBundle,
        _non_determinism: bool,
    ) -> Result<()> {
        Ok(())
    }
}
