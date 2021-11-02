use anyhow::anyhow;
use anyhow::Result;
use capsule::caching::backend::CachingBackend;
use capsule::caching::honeycomb;
use capsule::caching::stdio;
use capsule::capsule::Capsule;
use capsule::config::{Backend, Config};
use std::env;
use std::path::Path;
use std::process::Command;
use std::process::ExitStatus;

static USAGE: &'static str = "Usage: capsule <capsule arguments ...> -- command [<arguments>]";

fn create_capsule() -> Result<Capsule<'static>> {
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
    //let mut capsule = Capsule::new(&config, backend);
    //capsule.read_inputs()?;
    //capsule.write_cache()
    Ok(Capsule::new(Box::new(config), backend))
}

fn execute_command() -> Result<ExitStatus> {
    let mut args = env::args();
    let argv0 = &mut args.next();
    if argv0.is_none() {
        return Err(anyhow!(USAGE));
    }
    // Consume the rest of the arguments until we have the -- part
    for arg in args.by_ref() {
        if arg == "--" {
            break;
        }
    }

    let args: Vec<String> = args.collect();
    let s: String = args.join(" ");
    let mut p = Command::new("/bin/bash")
        .arg("-c")
        .arg(s)
        .spawn()
        .expect("failed to execute process");
    Ok(p.wait()?)
}

fn main() -> Result<()> {
    let _capsule = create_capsule();
    // TODO:

    let result = execute_command();

    match result {
        Ok(exit_status) => {
            // TODO:
            if exit_status.success() {

            }
        },
        Err(_err) => {
            // TODO:
        }
    }

    Ok(())
}
