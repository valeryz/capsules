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
    let default_toml = std::env::var("HOME").ok().map(|home| home + "/.capsules.toml");
    let config = Config::new(
        env::args(),
        default_toml.as_ref().map(Path::new),
        Some(Path::new("Capsule.toml")),
    )?;
    let backend: Box<dyn CachingBackend> = match config.backend {
        Backend::Stdio => Box::new(stdio::StdioBackend {
            verbose_output: config.verbose,
            capsule_id: config
                .capsule_id
                .clone()
                .ok_or_else(|| anyhow!("no capsule_id"))?,
        }),
        Backend::Honeycomb => Box::new(honeycomb::HoneycombBackend {
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
        }),
    };
    let mut capsule = Capsule::new(&config, backend);
    capsule.read_inputs()?;
    capsule.write_cache()
}

fn main() -> Result<()> {
    capsule_main().unwrap_or_else(|e| eprintln!("Capsule error: {:#}", e));
    // TODO: this goes away! - or maybe not!
    wrapper::execute()

    // TODO: pass through the exit code from the wrapped program.
}
