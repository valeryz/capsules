use anyhow::Result;
use std::fmt;

use crate::iohashing::{HashBundle, OutputHashBundle};

pub trait CachingBackend {
    fn name(&self) -> &'static str {
        "backend"
    }

    fn write(&self, inputs_bundle: &HashBundle, output_bundle: &OutputHashBundle) -> Result<()>;
}

impl fmt::Debug for dyn CachingBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Backend: {}", self.name())
    }
}
