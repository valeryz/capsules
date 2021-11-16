use anyhow::Result;
use capsule::caching::backend::CachingBackend;
use capsule::caching::dummy;
use capsule::caching::s3;
use capsule::capsule::Capsule;
use capsule::config::{Backend, Config};
use capsule::observability::dummy::Dummy as DummyLogger;
use capsule::observability::honeycomb;
use capsule::observability::logger::Logger;
use capsule::wrapper;
use std::env;
use std::os::unix::prelude::ExitStatusExt;
use std::process;
use std::path::Path;

fn create_capsule(config: &Config) -> Result<Capsule<'_>> {
    // First, instantiate our caching backend (S3, Dummy, or possibly other in the future).
    let backend: Box<dyn CachingBackend> = match config.backend {
        Backend::Dummy => Box::new(dummy::DummyBackend {
            verbose_output: config.verbose,
            capsule_id: config.capsule_id.as_ref().cloned().unwrap(),
        }),
        Backend::S3 => Box::new(s3::S3Backend::from_config(&config)?),
    };
    // Instantiate our logger (for observability)
    let logger: Box<dyn Logger> = if config.honeycomb_dataset.is_some() {
        Box::new(honeycomb::Honeycomb::from_config(&config)?)
    } else {
        Box::new(DummyLogger)
    };
    // Create the capsule with the caching backend and logger.
    Ok(Capsule::new(config, backend, logger))
}

#[tokio::main]
async fn main() -> Result<()> {
    let default_toml = std::env::var("HOME")
        .ok()
        .and_then(|home| Some(home + "/.capsules.toml"));
    let config = Config::new(
        env::args(),
        default_toml.as_ref().map(Path::new),
        Some(Path::new("Capsule.toml").as_ref()),
    )?;
    let capsule = create_capsule(&config)?;

    // Running of the capsule may fail. It may fail either before the wrapped program
    // was run, or after. This flag says whether the program was actually run.
    let mut program_run = false;
    let result = capsule.run_capsule(&mut program_run);

    match result {
        Ok((inputs, outputs, exit_status)) => {
            capsule
                .write_cache(inputs, outputs)
                .await
                .unwrap_or_else(|err| eprintln!("Couldn't write to cache: {}", err));
            process::exit(exit_status.into_raw());
        }
        Err(err) => {
            eprintln!("Capsule error: {:#}", err);
            // If we failed to run the program, try falling back to
            // just 'exec' behavior without any results caching.
            if !program_run {
                wrapper::execute_legacy().expect("Execution of wrapped program failed");
                unreachable!()
            } else {
                Err(err)
            }
        }
    }
}
