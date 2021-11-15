use anyhow::Result;
use crate::iohashing::{HashBundle, OutputHashBundle};

pub trait Logger {
    fn log(&self, _inputs_bundle: &HashBundle, _output_bundle: &OutputHashBundle) -> Result<()> {
        Ok(())
    }
}
