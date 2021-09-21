use sha2::{Digest, Sha256};
use anyhow::Result;
use ::capsule::wrapper;
use ::capsule::capsule;

#[derive(PartialOrd, Ord, PartialEq, Eq)]
enum Input {
    File(String),
    Tool(String),
}

struct InputSet {
    inputs: Vec<Input>,  // We always assume this vector is sorted.
}

impl InputSet {
    fn hash(&self) {
        // Calculate the hash of the input set independently of the order.
        let mut acc : Sha256 = Sha256::new();
        for input in &self.inputs {
            let payload = match input {
                Input::File(s) => ("File", s),
                Input::Tool(s) => ("Tool", s),
            };
            acc.input(&payload[0], &payload[1]);
        }
    }
}

struct Config {
    inputs: Vec<String>,
    outputs: Vec<String>,
}

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
    // let config = Config::new("");
    // let capsule = Capsule::new(
    Ok(())
}
