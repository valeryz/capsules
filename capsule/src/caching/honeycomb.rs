use crate::caching::backend::CachingBackend;
use anyhow::Result;
use reqwest;
use std::ffi::OsStr;
// use serde;
// use serde::Serialize;
use crate::iohashing::{HashBundle, Input};
use serde_json;

pub struct HoneycombBackend {
    // TODO: add whatever is necessary for Honeycomb.
    pub dataset: String,
    pub honeycomb_token: String,
    pub capsule_id: String,
    pub trace_id: String,
    pub parent_id: Option<String>,
}

/// Convert hash deails (with each filename and tool_tag separately) to JSON.
fn hash_details_to_json(bundle: &HashBundle) -> serde_json::Value {
    let mut file_map = serde_json::Map::<String, serde_json::Value>::new();
    let mut tool_tag_map = serde_json::Map::<String, serde_json::Value>::new();
    for (input, hash) in bundle.hash_details.iter() {
        let value = serde_json::Value::String(hash.to_string());
        match input {
            Input::File(filename) => {
                file_map.insert(filename.to_string_lossy().into(), value);
            }
            Input::ToolTag(tool_tag) => {
                tool_tag_map.insert(tool_tag.to_string_lossy().into(), value);
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

impl CachingBackend for HoneycombBackend {
    fn name(&self) -> &'static str {
        return "backend";
    }

    #[allow(unused_variables)]
    fn write(&self, capsule_id: &OsStr, inputs_bundle: &HashBundle, output_bundle: &HashBundle) -> Result<()> {
        let mut map = serde_json::Map::new();
        map.insert("trace.trace_id".into(), self.trace_id.clone().into());
        map.insert("trace.span_id".into(), self.capsule_id.clone().into());
        map.insert("inputs_hash".into(), inputs_bundle.hash.clone().into());
        map.insert("inputs_hash_details".into(), hash_details_to_json(inputs_bundle));
        if let Some(value) = &self.parent_id {
            map.insert("trace.parent_id".into(), value.clone().into());
        }
        map.insert("outputs_hash_details".into(), hash_details_to_json(output_bundle));
        map.insert("outputs_hash".into(), output_bundle.hash.clone().into());
        let client = reqwest::blocking::Client::new();
        let request = client
            .post(format!("https://api.honeycomb.io/1/events/{}", self.dataset))
            .header("X-Honeycomb-Team", &self.honeycomb_token)
            .json(&map)
            .send();
        Ok(())
    }
}
