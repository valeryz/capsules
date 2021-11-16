use anyhow::anyhow;
use anyhow::Result;
use reqwest;
use crate::{config::Config, iohashing::{HashBundle, OutputHashBundle, Input, Output}};
use serde_json;

use super::logger::Logger;

pub struct Honeycomb {
    /// Honeycomb dataset ('capsule', or 'capsule-test' etc.)
    pub dataset: String,

    /// Token for the Honeycomb API.
    pub honeycomb_token: String,

    /// Capsule ID of this capsule invocation.
    pub capsule_id: String,

    /// Honeycomb Trace ID
    pub trace_id: String,

    /// Honeycomb Parent trace ID.
    pub parent_id: Option<String>,

    /// Extra Key-values.
    pub extra_kv: Vec<(String, String)>,
}

impl Honeycomb {
    pub fn from_config(config: &Config) -> Result<Self> {
        Ok(Self {
            dataset: config
                .honeycomb_dataset
                .clone()
                .ok_or_else(|| anyhow!("Honeycomb dataset not specified"))?,
            honeycomb_token: config
                .honeycomb_token
                .clone()
                .ok_or_else(|| anyhow!("Honeycomb Token not specified"))?,
            capsule_id: config
                .capsule_id
                .clone()
                .ok_or_else(|| anyhow!("Capsule_id is unknown"))?,
            trace_id: config
                .honeycomb_trace_id
                .clone()
                .ok_or_else(|| anyhow!("Honeycomb Trace ID is not specified"))?,
            parent_id: config.honeycomb_parent_id.clone(),
            extra_kv: config.get_honeycomb_kv()?,
        })
    }
}

/// Max number of JSON entries in the dict. We need to cap it so that
/// the JSON Size doesn't exceed 100kB.
const MAX_JSON_ENTRIES: usize = 500;

/// Convert hash deails (with each filename and tool_tag separately) to JSON.
fn hash_details_to_json(bundle: &HashBundle) -> serde_json::Value {
    let mut file_map = serde_json::Map::<String, serde_json::Value>::new();
    let mut tool_tag_map = serde_json::Map::<String, serde_json::Value>::new();
    for (input, hash) in bundle.hash_details.iter() {
        // Cap the size of the resulting JSON.
        if file_map.len() + tool_tag_map.len() > MAX_JSON_ENTRIES {
            break;
        }
        let value = serde_json::Value::String(hash.to_string());
        match input {
            Input::File(filename) => {
                file_map.insert(filename.to_string_lossy().into(), value);
            }
            Input::ToolTag(tool_tag) => {
                tool_tag_map.insert(tool_tag.to_string(), value);
            }
        }
    }
    let mut json_map = serde_json::Map::<String, serde_json::Value>::new();
    if !file_map.is_empty() {
        json_map.insert("file".into(), serde_json::Value::Object(file_map));
    }
    if !tool_tag_map.is_empty() {
        json_map.insert("tool_tag".into(), serde_json::Value::Object(tool_tag_map));
    }
    serde_json::Value::Object(json_map)
}

/// Convert hash deails (with each filename and tool_tag separately) to JSON.
fn output_hash_details_to_json(bundle: &OutputHashBundle) -> serde_json::Value {
    let mut file_map = serde_json::Map::<String, serde_json::Value>::new();
    let mut exit_code : Option<i32> = None;
    for (output, hash) in bundle.hash_details.iter() {
        // Cap the size of the resulting JSON.
        if file_map.len() > MAX_JSON_ENTRIES {
            break;
        }
        let value = serde_json::Value::String(hash.to_string());
        match output {
            Output::File(file_output) => {
                file_map.insert(format!("{}", file_output.filename.to_string_lossy()), value);
            },
            Output::ExitCode(code) => {
                exit_code = Some(*code);
            },
            _ => { }
        }
    }
    let mut json_map = serde_json::Map::<String, serde_json::Value>::new();
    if !file_map.is_empty() {
        json_map.insert("file".into(), serde_json::Value::Object(file_map));
    }
    if let Some(code) = exit_code {
        json_map.insert("exit_code".into(), serde_json::Value::Number(code.into()));
    }
    serde_json::Value::Object(json_map)
}

impl Logger for Honeycomb {

    fn log(&self, inputs_bundle: &HashBundle, output_bundle: &OutputHashBundle) -> Result<()> {
        let mut map = serde_json::Map::new();
        map.insert("trace.trace_id".into(), self.trace_id.clone().into());
        map.insert("trace.span_id".into(), self.capsule_id.clone().into());
        map.insert("inputs_hash".into(), inputs_bundle.hash.clone().into());
        map.insert("inputs_hash_details".into(), hash_details_to_json(inputs_bundle));
        if let Some(value) = &self.parent_id {
            map.insert("trace.parent_id".into(), value.clone().into());
        }
        map.insert("outputs_hash_details".into(), output_hash_details_to_json(output_bundle));
        map.insert("outputs_hash".into(), output_bundle.hash.clone().into());
        for (key, value) in &self.extra_kv {
            map.insert(key.to_owned(), value.to_owned().into());
        }
        let client = reqwest::blocking::Client::new();
        client
            .post(format!("https://api.honeycomb.io/1/events/{}", self.dataset))
            .header("X-Honeycomb-Team", &self.honeycomb_token)
            .json(&map)
            .send()?;
        Ok(())
    }
}
