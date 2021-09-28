use std::env;
use anyhow::Result;
use capsule::caching::backend::CachingBackend;
use capsule::caching::honeycomb;
use capsule::caching::stdio;
use capsule::capsule::Capsule;
use capsule::config::{Backend, Config};
use capsule::wrapper;

fn main() -> Result<()> {
    // Read arguments
    // Find out which are inputs, and which are outputs
    // Calculate the key from the inputs:
    //   - this process could as well be cached - we don't have to read all files all the time.
    // Lookup the key in the cache.
    // If a key is found, extract the required results, and skip running the command.
    // If not found:
    //     run the command
    //     if succeeded (return code == 0):
    //          store the results in the cache
    //     else:
    //          store the failure code.
    // let caching_backend = CachingBackend::new();
    // let config = parse_config();
    // let calculate_key(config,
    // let results = wrapper::execute();
    // if results.success() {
    //     let cacheable_bundle = create_cacheable_bundle(configuration, results);
    //     let caching_backend.store =
    // } else {
    // }
    let config = Config::new(env::args())?;
    let backend : Box<dyn CachingBackend> = match config.backend {
        Backend::Stdio => Box::new(stdio::StdioBackend {}),
        Backend::Honeycomb => Box::new(honeycomb::HoneycombBackend {}),
    };
    let capsule = Capsule::new(&config, backend);
    capsule.write_cache()?;
    wrapper::execute(&config.command_to_run)
}
