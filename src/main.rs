use std::env;
use std::path::Path;
use anyhow::Result;
use capsule::caching::backend::CachingBackend;
use capsule::caching::honeycomb;
use capsule::caching::stdio;
use capsule::capsule::Capsule;
use capsule::config::{Backend, Config};
use capsule::wrapper;

fn capsule_main() -> Result<()> {
    let default_toml =  std::env::var("HOME").ok().and_then(|home| Some(home + "/.capsules.toml"));
    let config = Config::new(env::args(),
                             default_toml.as_ref().map(Path::new),
                             Some(Path::new("Capsule.toml").as_ref()))?;
    let backend : Box<dyn CachingBackend> = match config.backend {
        Backend::Stdio => Box::new(stdio::StdioBackend {}),
        Backend::Honeycomb => Box::new(honeycomb::HoneycombBackend {}),
    };
    let capsule = Capsule::new(&config, backend);
    capsule.write_cache()
}

fn main() -> Result<()> {
    capsule_main().unwrap_or_else(|e| eprintln!("Capsule error: {:#}", e));
    wrapper::execute()
}
