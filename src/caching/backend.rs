use anyhow::Result;
use std::ffi::OsStr;

pub struct OutputsBundle {
    // TODO: define what goes in the outputs bundle to be cached.
//
// We could very well use some content-addressable storage here
// where we don't store the bundle itself.
}

pub trait CachingBackend {
    fn write(
        &self,
        capsule_id: &OsStr,
        inputs_key: &OsStr,
        outputs_key: &OsStr,
        output_bundle: &OutputsBundle,
    ) -> Result<()>;
}
