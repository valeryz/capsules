use anyhow::Result;
use crate::caching::backend::{CachingBackend, OutputsBundle};
use std::ffi::OsStr;

pub struct HoneycombBackend {
    // TODO: add whatever is necessary for Honeycomb.
}

impl CachingBackend for HoneycombBackend {

    #[allow(unused_variables)]
    fn write(
        &self,
        capsule_id: &OsStr,
        inputs_key: &OsStr,
        outputs_key: &OsStr,
        output_bundle: &OutputsBundle,
    ) -> Result<()> {
        // TODO: implementation goes here!.
        Ok(())
    }

}
