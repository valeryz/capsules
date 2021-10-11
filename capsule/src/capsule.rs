use anyhow;
use anyhow::{Context, Result};
use std::ffi::{OsStr, OsString};

use crate::caching::backend::{CachingBackend, OutputsBundle};
use crate::config::Config;
use crate::iohashing::*;

pub struct Capsule<'a> {
    config: &'a Config,
    caching_backend: Box<dyn CachingBackend>,
    inputs: InputSet<'a>,
    outputs: OutputSet<'a>,
}

impl<'a> Capsule<'a> {
    pub fn new(config: &'a Config, caching_backend: Box<dyn CachingBackend>) -> Self {
        let mut capsule = Self {
            config,
            caching_backend,
            inputs: InputSet::default(),
            outputs: OutputSet::default(),
        };

        for file in &config.input_files {
            capsule.inputs.add_input(Input::File(file));
        }
        for tool_tag in &config.tool_tags {
            capsule.inputs.add_input(Input::ToolTag(tool_tag));
        }
        capsule
    }

    pub fn hash(&self) -> Result<String> {
        self.inputs.hash()
    }

    pub fn write_cache(&self) -> Result<()> {
        // Outputs bundle is ununsed in Placebo.
        let bundle = OutputsBundle {};
        let capsule_id = self
            .config
            .capsule_id
            .as_ref()
            .expect("capsule_id must be specified");
        let input_bundle = self.inputs.hash_bundle()
            .with_context(|| format!("Hashing inputs of capsule '{:?}'", capsule_id))?;
        self.caching_backend.write(
            capsule_id,
            &input_bundle,
            &OsString::from(""), // output_hash
            &bundle,
        )
    }
}

