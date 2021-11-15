use anyhow::anyhow;
use anyhow::Result;
use capsule::caching::backend::CachingBackend;
use capsule::caching::s3;
use capsule::caching::dummy;
use capsule::honeycomb;
use capsule::capsule::Capsule;
use capsule::config::{Backend, Config};
use capsule::wrapper;
use std::env;
use std::path::Path;

async fn capsule_main() -> Result<()> {
    let default_toml = std::env::var("HOME").ok().map(|home| home + "/.capsules.toml");
    let config = Config::new(
        env::args(),
        default_toml.as_ref().map(Path::new),
        Some(Path::new("Capsule.toml")),
    )?;
    let backend: Box<dyn CachingBackend> = match config.backend {
        Backend::Dummy => Box::new(dummy::DummyBackend),
        Backend::S3 => Box::new(s3::S3Backend::from_config(&config)),
    };
    let tracer = honeycomb::Honeycomb {
    }
    let mut capsule = Capsule::new(&config, backend, tracer);
    capsule.read_inputs()?;
    capsule.write_cache()
}

#[tokio::main]
async fn main() -> Result<()> {
    capsule_main()
        .await
        .unwrap_or_else(|e| eprintln!("Capsule error: {:#}", e));
    // TODO: this goes away! - or maybe not!
    wrapper::execute()

    // TODO: pass through the exit code from the wrapped program.
}
