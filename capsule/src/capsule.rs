use anyhow;
use anyhow::{Context, Result};

use glob::glob;

use crate::caching::backend::CachingBackend;
use crate::config::Config;
use crate::iohashing::*;

pub struct Capsule<'a> {
    config: &'a Config,
    caching_backend: Box<dyn CachingBackend>,
    inputs: InputSet<'a>,
    // TODO(valeryz): enable it in Blue Pill.
    // outputs: OutputSet<'a>,
}

impl<'a> Capsule<'a> {
    pub fn new(config: &'a Config, caching_backend: Box<dyn CachingBackend>) -> Self {
        Self {
            config,
            caching_backend,
            inputs: InputSet::default(),
            // outputs: OutputSet::default(),
        }
    }

    pub fn read_inputs(&mut self) -> Result<()> {
        for file_pattern in &self.config.input_files {
            for file in glob(&file_pattern.to_string_lossy())? {
                let file = file?;
                self.inputs.add_input(Input::File(file));
            }
        }
        for tool_tag in &self.config.tool_tags {
            self.inputs.add_input(Input::ToolTag(tool_tag));
        }
        Ok(())
    }

    pub fn hash(&self) -> Result<String> {
        self.inputs.hash()
    }

    pub fn write_cache(&self) -> Result<()> {
        // Outputs bundle is ununsed in Placebo, creating an empty one.
        let output_bundle = HashBundle {
            hash: "".into(),
            hash_details: vec![],
        };
        let capsule_id = self.config.capsule_id.as_ref().expect("capsule_id must be specified");
        let input_bundle = self
            .inputs
            .hash_bundle()
            .with_context(|| format!("Hashing inputs of capsule '{:?}'", capsule_id))?;
        self.caching_backend.write(&input_bundle, &output_bundle)
    }
}
