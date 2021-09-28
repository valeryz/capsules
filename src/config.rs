use anyhow::{bail, Result};
use clap::{App, Arg};
use itertools;
use serde::Deserialize;
use std::collections::HashMap;
use std::{env, ffi::OsString};
use toml;

#[derive(Deserialize)]
pub enum Milestone {
    Placebo,
    BluePill,
    OragePill,
    RedPill,
}

#[derive(Deserialize)]
pub enum Backend {
    Stdio,
    Honeycomb,
}

#[derive(Deserialize)]
pub struct Config {
    pub milestone: Milestone,
    pub backend: Backend,
    pub capsule_id: Option<OsString>,
    pub input_files: Vec<OsString>,
    pub tool_tags: Vec<OsString>,
    pub output_files: Vec<OsString>,
    pub capture_stdout: bool,
    pub capture_stderr: bool,
    pub command_to_run: Vec<OsString>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            milestone: Milestone::Placebo,
            backend: Backend::Stdio,
            capsule_id: None,
            input_files: vec![],
            tool_tags: vec![],
            output_files: vec![],
            capture_stdout: false,
            capture_stderr: false,
            command_to_run: vec![],
        }
    }
}

impl Config {
    pub fn new<I, T>(cmdline_args: I,
                     default_toml: Option<&std::path::Path>,
                     current_toml: Option<&std::path::Path>) -> Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let mut config = Self::default();
        if let Some(default_toml) = default_toml {
            if let Ok(contents) = std::fs::read_to_string(default_toml) {
                if let Ok(home_config) = toml::from_str(&contents) {
                    config = home_config;
                }
            }
        }

        let mut dir_config = HashMap::<String, Config>::new();
        if let Some(current_toml) = current_toml {
            if let Ok(contents) = std::fs::read_to_string(current_toml) {
                if let Ok(config) = toml::from_str(&contents) {
                    dir_config = config;
                } else {
                    bail!("Could not parse Capsules.toml");
                }
            }
        }

        // Command line.
        let arg_matches = App::new("Capsules")
            .version("1.0")
            .arg(
                Arg::new("capsule_id")
                    .about("The ID of the capsule (usually a target path)")
                    .short('c')
                    .long("capsule_id")
                    .takes_value(true)
                    .multiple_occurrences(false),
            )
            .arg(
                Arg::new("input")
                    .about("Input file")
                    .short('i')
                    .long("input")
                    .takes_value(true)
                    .multiple_occurrences(true),
            )
            .arg(
                Arg::new("tool")
                    .about("Tool string (usually with a version)")
                    .short('t')
                    .long("tool")
                    .takes_value(true)
                    .multiple_occurrences(true),
            )
            .arg(
                Arg::new("output")
                    .about("Output file")
                    .short('o')
                    .long("output")
                    .takes_value(true)
                    .multiple_occurrences(true),
            )
            .arg(
                Arg::new("stdout")
                    .about("Capture stdout with the cached bundle")
                    .long("stdout")
                    .takes_value(false),
            )
            .arg(
                Arg::new("stderr")
                    .about("Capture stderr with the cached bundle")
                    .long("stderr")
                    .takes_value(false),
            )
            .arg(Arg::new("command_to_run").last(true));

        let match_sources = [
            // First we look at the environment variable CAPSULE_ARGS,
            // which has the default args, not listed on command line.
            arg_matches.clone().get_matches_from(itertools::chain(
                ["capsule"],
                env::var("CAPSULE_ARGS")
                    .unwrap_or_default()
                    .split_whitespace(),
            )),
            // Then we look at the actual command line args.
            arg_matches.clone().get_matches_from(cmdline_args),
        ];

        for matches in &match_sources {
            if let Some(capsule_id) = matches.value_of_os("capsule_id") {
                config.capsule_id = Some(capsule_id.to_owned());
            }
        }

        // If there's only one entry in Capsules.toml, it is implied,
        // and we don't have to specify the -c flag.
        if config.capsule_id.is_none() {
            if dir_config.len() == 1 {
                config.capsule_id = Some(dir_config.keys().nth(0).unwrap().into());
            } else {
                bail!("Cannot determine capsule_id");
            }
        }

        for matches in match_sources {
            if let Some(inputs) = matches.values_of_os("input") {
                config.input_files.extend(inputs.map(|x| x.to_owned()));
            }
            if let Some(tools) = matches.values_of_os("tool") {
                config.tool_tags.extend(tools.map(|x| x.to_owned()));
            }
            if let Some(outputs) = matches.values_of_os("output") {
                config.output_files.extend(outputs.map(|x| x.to_owned()));
            }
            if matches.is_present("stdout") {
                config.capture_stdout = true;
            }
            if matches.is_present("stderr") {
                config.capture_stderr = true;
            }
            if let Some(command) = matches.values_of_os("command_to_run") {
                config.command_to_run = command.map(|x| x.to_os_string()).collect();
            }
        }

        if config.command_to_run.is_empty() {
            bail!("The command to run was not specified");
        }

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::iter;

    const EMPTY_ARGS: iter::Empty<OsString> = std::iter::empty::<OsString>();

    #[test]
    fn test_command_line_1() {
        env::set_var("CAPSULE_ARGS", "-c my_capsule -- /bin/echo");
        let config = Config::new(EMPTY_ARGS, None, None).unwrap();
        assert_eq!(config.capsule_id.unwrap(), "my_capsule");
    }

    #[test]
    fn test_command_line_2() {
        let config = Config::new(vec!["placebo", "-c", "my_capsule", "--", "/bin/echo"], None, None).unwrap();
        assert_eq!(config.capsule_id.unwrap(), "my_capsule");
    }

    fn test_toml() {

    }

    fn test_toml_defaults() {}

    fn test_comamnd_line_precedence() {}

    fn test_required_canister_id() {}
}
