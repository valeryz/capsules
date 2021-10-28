use crate::caching::backend::CachingBackend;
use anyhow::Result;
use reqwest;
use std::ffi::OsStr;
// use serde;
// use serde::Serialize;
use crate::iohashing::{HashBundle, Input};
use serde_json;
use serde_json::json;

pub struct HoneycombBackend {
    // TODO: add whatever is necessary for Honeycomb.
    pub dataset: String,
    pub honeycomb_token: String,
    pub capsule_id: String,
    pub trace_id: String,
    pub parent_id: Option<String>,
}

fn hash_details_to_json(bundle: &HashBundle) -> serde_json::Value {
    let mut json_map = serde_json::Map::<String, serde_json::Value>::new();
    for (input, hash) in bundle.hash_details.iter() {
        match input {
            Input::File(filename) => json_map.insert("file".into(), json!({ filename.to_string_lossy(): hash })),
            Input::ToolTag(tool_tag) => json_map.insert("tool_tag".into(), json!({ tool_tag.to_string_lossy(): hash })),
        };
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
        if let Some(value) = &self.parent_id {
            map.insert("trace.parent_id".into(), value.clone().into());
        }
        map.insert("outputs_hash_details".into(), hash_details_to_json(inputs_bundle));
        map.insert("outputs_hash".into(), inputs_bundle.hash.clone().into());
        let client = reqwest::blocking::Client::new();
        let request = client
            .post(format!("https://api.honeycomb.io/1/events/{}", self.dataset))
            .header("X-Honeycomb-Team", &self.honeycomb_token)
            .json(&map)
            .send();
        Ok(())
    }
}
