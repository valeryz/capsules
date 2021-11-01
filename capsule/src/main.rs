use anyhow::anyhow;
use anyhow::Result;
use capsule::caching::backend::CachingBackend;
use capsule::caching::honeycomb;
use capsule::caching::stdio;
use capsule::capsule::Capsule;
use capsule::config::{Backend, Config};
use capsule::wrapper;
use std::env;
use std::path::Path;

fn capsule_main() -> Result<()> {
    let default_toml = std::env::var("HOME")
        .ok()
        .and_then(|home| Some(home + "/.capsules.toml"));
    let config = Config::new(
        env::args(),
        default_toml.as_ref().map(Path::new),
        Some(Path::new("Capsule.toml").as_ref()),
    )?;
    let backend: Box<dyn CachingBackend> = match config.backend {
        Backend::Stdio => Box::new(stdio::StdioBackend {
            verbose_output: config.verbose,
            capsule_id: config
                .capsule_id
                .as_ref()
                .ok_or(anyhow!("no capsule_id"))?
                .to_string_lossy()
                .into(),
        }),
        Backend::Honeycomb => Box::new(honeycomb::HoneycombBackend {
            dataset: config
                .honeycomb_dataset
                .clone()
                .ok_or(anyhow!("Honeycomb dataset not specified"))?
                .to_string_lossy()
                .into_owned(),
            honeycomb_token: config
                .honeycomb_token
                .clone()
                .ok_or(anyhow!("Honeycomb Token not specified"))?
                .to_string_lossy()
                .into_owned(),
            capsule_id: config
                .capsule_id
                .clone()
                .ok_or(anyhow!("Capsule_id is unknown"))?
                .to_string_lossy()
                .into_owned(),
            trace_id: config
                .honeycomb_trace_id
                .clone()
                .ok_or(anyhow!("Honeycomb Trace ID is not specified"))?
                .to_string_lossy()
                .into_owned(),
            parent_id: config.honeycomb_parent_id.as_ref().map(|x| x.to_string_lossy().into()),
        }),
    };
    let mut capsule = Capsule::new(&config, backend);
    capsule.read_inputs()?;
    capsule.write_cache()
}

fn main() -> Result<()> {
    capsule_main().unwrap_or_else(|e| eprintln!("Capsule error: {:#}", e));
    wrapper::execute()
}
