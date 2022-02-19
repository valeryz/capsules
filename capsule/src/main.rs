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
use log::error;
use std::env;
use std::path::Path;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging. Default is INFO level, can be overridden in CAPSULE_LOG
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .parse_env("CAPSULE_LOG")
        .init();

    // Running of the capsule may fail. It may fail either before the wrapped program
    // was run, or after. This flag says whether the program was actually run.
    let mut program_run = AtomicBool::new(false);
    let program_run_ref = &mut program_run;
    // Place all the initialization logic is a separate block, so that the ? bailouts
    // return the result right there.
    let result = async move {
        let default_toml = std::env::var("HOME").ok().map(|home| home + "/.capsules.toml");
        let config = Config::new(
            env::args(),
            default_toml.as_ref().map(Path::new),
            Some(Path::new("Capsule.toml")),
        )?;
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

        let capsule = Capsule::new(&config, backend.as_ref(), logger.as_ref());

        capsule.run_capsule(program_run_ref).await
    }
    .await;

    match result {
        Ok(exit_code) => {
            // Pass the exit code of the wrapped program as our exit code.
            process::exit(exit_code);
        }
        Err(err) => {
            error!("Capsule error: {:#}", err);
            // If we failed to run the program, try falling back to
            // just 'exec' behavior without any results caching.
            if !program_run.load(Ordering::SeqCst) {
                wrapper::exec().expect("Execution of wrapped program failed");
                unreachable!()
            } else {
                Err(err)
            }
        }
    }
}
