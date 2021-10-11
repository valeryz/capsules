use anyhow::Result;
use std::ffi::OsStr;
use std::fmt;

use crate::iohashing::HashBundle;

pub struct OutputsBundle {
    // TODO: define what goes in the outputs bundle to be cached.
    //
    // We could very well use some content-addressable storage here
    // where we don't store the bundle itself.
}

pub trait CachingBackend {
    fn name(&self) -> &'static str { return "backend"; }

    fn write(
        &self,
        capsule_id: &OsStr,
        inputs_bundle: &HashBundle,
        outputs_key: &OsStr,
        output_bundle: &OutputsBundle,
    ) -> Result<()>;
}

impl fmt::Debug for dyn CachingBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Backend: {}", self.name())
    }
}
