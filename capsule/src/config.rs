use anyhow::{bail, Context, Result};
use clap::{App, Arg};
use itertools;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::{env, ffi::OsString};
use toml;

#[derive(Debug)]
pub enum Milestone {
    Placebo,
    BluePill,
    OragePill,
    RedPill,
}

#[derive(Debug)]
pub enum Backend {
    Stdio,
    Honeycomb,
}

#[derive(Debug)]
pub struct Config {
    pub milestone: Milestone,
    pub verbose: bool,
    pub backend: Backend,
    pub capsule_id: Option<OsString>,
    pub input_files: Vec<OsString>,
    pub tool_tags: Vec<OsString>,
    pub output_files: Vec<OsString>,
    pub capture_stdout: Option<bool>,
    pub capture_stderr: Option<bool>,
    pub command_to_run: Vec<OsString>,
}

impl std::ops::Deref for Config {
    type Target = Option<OsString>;

    fn deref(&self) -> &Self::Target {
        &self.capsule_id
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            milestone: Milestone::Placebo,
            verbose: false,
            backend: Backend::Stdio,
            capsule_id: None,
            input_files: vec![],
            tool_tags: vec![],
            output_files: vec![],
            capture_stdout: Some(false),
            capture_stderr: Some(false),
            command_to_run: vec![],
        }
    }
}

// We use this struct for Toml deserialization, because Serde deserializes
// OsString in a very weird way, therefore we deserialize into Strings,
// later converting them into OsString.
#[derive(Deserialize, Debug)]
struct StringConfig {
    #[serde(default)]
    pub capsule_id: Option<String>,

    #[serde(default)]
    pub verbose: Option<bool>,

    #[serde(default)]
    pub input_files: Vec<String>,

    #[serde(default)]
    pub tool_tags: Vec<String>,

    #[serde(default)]
    pub output_files: Vec<String>,

    #[serde(default)]
    pub capture_stdout: Option<bool>,

    #[serde(default)]
    pub capture_stderr: Option<bool>,
}

impl From<StringConfig> for Config {
    fn from(config: StringConfig) -> Self {
        Config {
            capsule_id: config.capsule_id.map(OsString::from),
            verbose: config.verbose.unwrap_or(false),
            input_files: config.input_files.iter().map(OsString::from).collect(),
            tool_tags: config.tool_tags.iter().map(OsString::from).collect(),
            output_files: config.output_files.iter().map(OsString::from).collect(),
            capture_stdout: config.capture_stdout,
            capture_stderr: config.capture_stderr,
            ..Config::default()
        }
    }
}

impl Config {
    // Merge one config (e.g. Capsule.toml) into another (~/.capsule.toml)
    // It destroys the argument.
    pub fn merge(&mut self, config: &mut Self) {
        if self.capsule_id.is_none() {
            self.capsule_id = config.capsule_id.take();
        }
        if config.verbose {
            self.verbose = true;
        }
        self.input_files.append(&mut config.input_files);
        self.output_files.append(&mut config.output_files);
        self.tool_tags.append(&mut config.tool_tags);
        self.capture_stdout = config.capture_stdout;
        self.capture_stderr = config.capture_stderr;
    }

    pub fn new<I, T>(
        cmdline_args: I,
        default_toml: Option<&Path>,
        current_toml: Option<&Path>,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        // Read the defaults TOML (usually from ~/.capsule.toml).
        let mut config = Self::default();
        if let Some(default_toml) = default_toml {
            if let Ok(contents) = std::fs::read_to_string(default_toml) {
                let home_config = toml::from_str::<StringConfig>(&contents)
                    .with_context(|| format!("Parsing default config {:?}", default_toml))?;
                config = Config::from(home_config);
            }
        }

        // Read the main TOML (usually from Capsule.toml in the current directory).
        let mut dir_config: BTreeMap<String, Config> = BTreeMap::new();
        if let Some(current_toml) = current_toml {
            if let Ok(contents) = std::fs::read_to_string(current_toml) {
                match toml::from_str::<BTreeMap<String, StringConfig>>(&contents) {
                    Ok(config) => {
                        dir_config = config
                            .into_iter()
                            .map(|(key, value)| (key, Config::from(value)))
                            .collect();
                    }
                    Err(e) => {
                        bail!("Could not parse Capsules.toml: {}", e)
                    }
                }
            }
        }

        // Read the command line from both os::args and the environment.
        let arg_matches = App::new("capsule")
            .version(env!("CARGO_PKG_VERSION"))
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

        // Take capsule_is as String (we get lots of OsString here!)
        let capsule_id: &OsString = config.capsule_id.as_ref().unwrap();
        let capsule_id: OsString = capsule_id.clone();
        let capsule_id: String = capsule_id.into_string().unwrap();

        // Dir_config can have many sections, relating to manu capsules.
        // We pick the onle related to the current capsule_id.
        // We call .remove() to take full ownership of the single_config.
        if let Some(mut single_config) = dir_config.remove(&capsule_id) {
            config.merge(&mut single_config);
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
                config.capture_stdout = Some(true);
            }
            if matches.is_present("stderr") {
                config.capture_stderr = Some(true);
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
    use indoc::indoc;
    use serial_test::serial;
    use std::io::Write;
    use std::iter;
    use tempfile::NamedTempFile;

    const EMPTY_ARGS: iter::Empty<OsString> = std::iter::empty::<OsString>();

    #[test]
    #[serial] // Must serialize these tests so that env vars don't affect other tests.
    fn test_command_line_1() {
        env::set_var("CAPSULE_ARGS", "-c my_capsule -- /bin/echo");
        let config = Config::new(EMPTY_ARGS, None, None).unwrap();
        assert_eq!(config.capsule_id.unwrap(), "my_capsule");
        assert_eq!(config.command_to_run[0], "/bin/echo");
        env::remove_var("CAPSULE_ARGS");
    }

    #[test]
    #[serial]
    fn test_command_line_2() {
        let config = Config::new(
            vec!["placebo", "-c", "my_capsule", "--", "/bin/echo"],
            None,
            None,
        )
        .unwrap();
        assert_eq!(config.capsule_id.unwrap(), "my_capsule");
        assert_eq!(config.command_to_run[0], "/bin/echo");
    }

    #[test]
    #[serial]
    fn test_command_line_no_command() {
        Config::new(vec!["placebo", "-c", "my_capsule"], None, None).unwrap_err();
    }

    #[test]
    #[serial]
    fn test_toml() {
        let mut config_file = NamedTempFile::new().unwrap();
        let config_contents: &'static str = indoc! {r#"
           [my_capsule]
           output_files=["compiled_binary"]
           input_files=["/etc/passwd", "/nonexistent"]
        "#};
        println!("Config file:\n{}", config_contents);
        config_file.write(config_contents.as_bytes()).unwrap();
        config_file.flush().unwrap();

        let config = Config::new(
            vec!["placebo", "-c", "my_capsule", "--", "/bin/echo"],
            None,
            Some(config_file.path()),
        )
        .unwrap();
        assert_eq!(config.input_files, vec!["/etc/passwd", "/nonexistent"]);
        assert_eq!(config.output_files, vec!["compiled_binary"]);
    }

    #[test]
    #[serial]
    fn test_toml_defaults() {
        let mut config_file = NamedTempFile::new().unwrap();
        let config_contents: &'static str = indoc! {r#"
           capture_stdout = true
           tool_tags = ["docker-ABCDEF"]
        "#};
        println!("Config file:\n{}", config_contents);
        config_file.write(config_contents.as_bytes()).unwrap();
        config_file.flush().unwrap();

        let config = Config::new(
            vec!["placebo", "-c", "my_capsule", "--", "/bin/echo"],
            Some(config_file.path()),
            None,
        )
        .unwrap();
        assert_eq!(config.capture_stdout, Some(true));
        assert!(config.capture_stderr.is_none());
    }

    #[test]
    #[serial]
    fn test_toml_precedence() {
        let mut default_config_file = NamedTempFile::new().unwrap();
        let config_contents: &'static str = indoc! {r#"
           capture_stdout = true
           tool_tags = ["docker-ABCDEF"]
        "#};
        println!("Config file:\n{}", config_contents);
        default_config_file
            .write(config_contents.as_bytes())
            .unwrap();
        default_config_file.flush().unwrap();

        let mut current_config_file = NamedTempFile::new().unwrap();
        let config_contents: &'static str = indoc! {r#"
           [my_capsule]
           capture_stdout = false
           output_files=["compiled_binary"]
           input_files=["/etc/passwd", "/nonexistent"]
           tool_tags = ["docker-1234"]
        "#};
        current_config_file
            .write(config_contents.as_bytes())
            .unwrap();
        current_config_file.flush().unwrap();

        let config = Config::new(
            vec!["placebo", "-c", "my_capsule", "--", "/bin/echo"],
            Some(default_config_file.path()),
            Some(current_config_file.path()),
        )
        .unwrap();
        assert_eq!(config.capture_stdout, Some(false));
        assert!(config.capture_stderr.is_none());
        assert_eq!(config.tool_tags, vec!["docker-ABCDEF", "docker-1234"]);
    }

    #[test]
    #[serial]
    fn test_toml_capsule_id_mismatch() {
        let mut current_config_file = NamedTempFile::new().unwrap();
        let config_contents: &'static str = indoc! {r#"
           [another_capsule]
           capture_stdout = false
           output_files=["compiled_binary"]
           input_files=["/etc/passwd", "/nonexistent"]
           tool_tags = ["docker-1234"]
        "#};
        current_config_file
            .write(config_contents.as_bytes())
            .unwrap();
        current_config_file.flush().unwrap();

        let config = Config::new(
            vec!["placebo", "-c", "my_capsule", "--", "/bin/echo"],
            None,
            Some(current_config_file.path()),
        )
        .unwrap();
        assert_eq!(config.tool_tags, Vec::<&str>::new());
        assert_eq!(config.input_files, Vec::<&str>::new());
        assert_eq!(config.output_files, Vec::<&str>::new());
    }

    #[test]
    #[serial]
    fn test_unique_canister_id() {
        let mut current_config_file = NamedTempFile::new().unwrap();
        let config_contents: &'static str = indoc! {r#"
           [my_capsule_id]
           output_files=["compiled_binary"]
           input_files=["/etc/passwd", "/nonexistent"]
        "#};
        current_config_file
            .write(config_contents.as_bytes())
            .unwrap();
        current_config_file.flush().unwrap();

        let config = Config::new(
            vec!["placebo", "--", "/bin/echo"],
            None,
            Some(current_config_file.path()),
        )
        .unwrap();
        assert_eq!(config.capsule_id, Some(OsString::from("my_capsule_id")));
    }

    #[test]
    #[serial]
    fn test_missing_canister_id() {
        let mut current_config_file = NamedTempFile::new().unwrap();
        // This config has two sections, two capsule_ids. We don't know which one is meant.
        let config_contents: &'static str = indoc! {r#"
           [my_capsule_id]
           output_files=["compiled_binary"]
           input_files=["/etc/passwd", "/nonexistent"]

           [other_capsule_id]
           output_files=["compiled_binary"]
           input_files=["/etc/passwd", "/nonexistent"]
        "#};
        current_config_file
            .write(config_contents.as_bytes())
            .unwrap();
        current_config_file.flush().unwrap();

        Config::new(
            vec!["placebo", "--", "/bin/echo"],
            None,
            Some(current_config_file.path()),
        )
        .unwrap_err();
    }
}
