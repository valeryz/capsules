use anyhow::{anyhow, bail, Context, Result};
use clap::{App, Arg};
use derivative::Derivative;
use itertools;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{env, ffi::OsString};
use toml;

#[derive(Debug, Derivative, PartialEq)]
#[derivative(Default)]
pub enum Milestone {
    #[derivative(Default)]
    Placebo,
    BluePill,
    OragePill,
    RedPill,
}

#[derive(Debug, Derivative)]
#[derivative(Default)]
pub enum Backend {
    #[derivative(Default)]
    Dummy, // No backend means dummy.
    S3,
}

#[derive(Debug, Deserialize, Derivative)]
#[derivative(Default)]
pub struct Config {
    #[serde(skip)]
    pub milestone: Milestone,

    #[serde(default)]
    pub verbose: bool,

    #[serde(default)]
    pub cache_failure: bool,

    #[serde(skip)]
    pub backend: Backend,

    #[serde(default)]
    pub capsule_id: Option<String>,

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

    #[serde(default)]
    pub command_to_run: Vec<String>,

    #[serde(default)]
    pub honeycomb_token: Option<String>,

    #[serde(default)]
    pub honeycomb_dataset: Option<String>,

    #[serde(default)]
    pub honeycomb_trace_id: Option<String>,

    #[serde(default)]
    pub honeycomb_parent_id: Option<String>,

    // values of --honeycomb_kv flag, to be accessed via a method.
    #[serde(default)]
    honeycomb_kv: Vec<String>,

    #[serde(default)]
    pub s3_bucket: Option<String>,

    #[serde(default)]
    pub s3_bucket_objects: Option<String>,

    #[serde(default)]
    pub s3_endpoint: Option<String>,

    #[serde(default)]
    pub s3_region: Option<String>,
}

impl Config {
    // Merge one config (e.g. Capsule.toml) into another (~/.capsules.toml)
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
        if self.honeycomb_dataset.is_none() {
            self.honeycomb_dataset = config.honeycomb_dataset.take();
        }
        if self.honeycomb_token.is_none() {
            self.honeycomb_token = config.honeycomb_token.take();
        }
    }

    pub fn new<I, T>(cmdline_args: I, default_toml: Option<&Path>, current_toml: Option<&Path>) -> Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        // Read the defaults TOML (usually from ~/.capsules.toml).
        let mut config = Self::default();
        if let Some(default_toml) = default_toml {
            if let Ok(contents) = std::fs::read_to_string(default_toml) {
                let home_config = toml::from_str::<Config>(&contents)
                    .with_context(|| format!("Parsing default config '{}'", default_toml.to_string_lossy()))?;
                config = home_config;
            }
        }

        // Read the main TOML (usually from Capsule.toml in the current directory).
        let mut dir_config: BTreeMap<String, Config> = BTreeMap::new();
        if let Some(current_toml) = current_toml {
            if let Ok(contents) = std::fs::read_to_string(current_toml) {
                dir_config = toml::from_str::<BTreeMap<String, Config>>(&contents)?;
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
            .arg(
                Arg::new("verbose")
                    .about("Verbose output")
                    .short('v')
                    .long("verbose")
                    .takes_value(false),
            )
            .arg(
                Arg::new("cache_failure")
                    .about("Verbose output")
                    .short('f')
                    .long("cache_failure"),
            )
            .arg(
                Arg::new("backend")
                    .short('b')
                    .long("backend")
                    .about("which backend to use")
                    .possible_values(&["dummy", "s3"]),
            )
            .arg(
                Arg::new("honeycomb_dataset")
                    .long("honeycomb_dataset")
                    .about("Honeycomb Dataset")
                    .takes_value(true),
            )
            .arg(
                Arg::new("honeycomb_token")
                    .long("honeycomb_token")
                    .about("Honeycomb Access Token")
                    .takes_value(true),
            )
            .arg(
                Arg::new("honeycomb_trace_id")
                    .long("honeycomb_trace_id")
                    .about("Honeycomb Trace ID")
                    .takes_value(true),
            )
            .arg(
                Arg::new("honeycomb_parent_id")
                    .long("honeycomb_parent_id")
                    .about("Honeycomb trace span parent ID")
                    .takes_value(true),
            )
            .arg(
                Arg::new("honeycomb_kv")
                    .long("honeycomb_kv")
                    .about("Honeycomb Extra Key-Value")
                    .takes_value(true)
                    .multiple_occurrences(true),
            )
            .arg(
                Arg::new("s3_bucket")
                    .long("s3_bucket")
                    .about("S3 bucket name")
                    .takes_value(true),
            )
            .arg(
                Arg::new("s3_bucket_objects")
                    .long("s3_bucket_objects")
                    .about("S3 bucket for objects name")
                    .takes_value(true),
            )
            .arg(
                Arg::new("s3_endpoint")
                    .long("s3_endpoint")
                    .about("S3 endpoint")
                    .takes_value(true),
            )
            .arg(
                Arg::new("s3_region")
                    .long("s3_region")
                    .about("S3 region")
                    .takes_value(true),
            )
            .arg(Arg::new("command_to_run").last(true));

        // Look at the first element of command line, to find and remember argv[0].

        // If we explicitly name our program placebo, it will act as such, otherwise we move to Blue
        // Pill milestone.
        let cmdline_args: Vec<OsString> = cmdline_args.into_iter().map(Into::into).collect();
        if cmdline_args.len() < 1 {
            return Err(anyhow!("No argv0"));
        }

        let capsule_args: Vec<OsString> = shell_words::split(&env::var("CAPSULE_ARGS").unwrap_or_default())
            .context("failed to parse CAPSULE_ARGS")?
            .into_iter()
            .map(Into::into)
            .collect();

        let matches = arg_matches.get_matches_from(itertools::chain(
            &cmdline_args[..1],
            itertools::chain(&capsule_args[..], &cmdline_args[1..]),
        ));

        if PathBuf::from(cmdline_args[0].clone()).ends_with("placebo") {
            config.milestone = Milestone::Placebo;
        } else {
            config.milestone = Milestone::BluePill;
        }

        if let Some(capsule_id) = matches.value_of("capsule_id") {
            config.capsule_id = Some(capsule_id.to_owned());
        }

        // If there's only one entry in Capsules.toml, it is implied,
        // and we don't have to specify the -c flag.
        if config.capsule_id.is_none() {
            if dir_config.len() == 1 {
                config.capsule_id = Some(dir_config.keys().next().unwrap().into());
            } else {
                bail!("Cannot determine capsule_id");
            }
        }

        let capsule_id = config.capsule_id.as_ref().unwrap();

        // Dir_config can have many sections, relating to manu capsules.
        // We pick the onle related to the current capsule_id.
        // We call .remove() to take full ownership of the single_config.
        if let Some(mut single_config) = dir_config.remove(capsule_id) {
            config.merge(&mut single_config);
        }

        config.backend = Backend::Dummy; // default caching backend.

        if let Some(inputs) = matches.values_of("input") {
            config.input_files.extend(inputs.map(|x| x.to_owned()));
        }
        if let Some(tools) = matches.values_of("tool") {
            config.tool_tags.extend(tools.map(|x| x.to_owned()));
        }
        if let Some(outputs) = matches.values_of("output") {
            config.output_files.extend(outputs.map(|x| x.to_owned()));
        }
        if matches.is_present("stdout") {
            config.capture_stdout = Some(true);
        }
        if matches.is_present("stderr") {
            config.capture_stderr = Some(true);
        }
        if matches.is_present("verbose") {
            config.verbose = true;
        }
        if matches.is_present("cache_failure") {
            config.cache_failure = true;
        }
        if let Some(command) = matches.values_of("command_to_run") {
            config.command_to_run = command.map(|x| x.to_owned()).collect();
        }
        if let Some(backend) = matches.value_of("backend") {
            if backend == "s3" {
                config.backend = Backend::S3;
            }
        }
        if let Some(value) = matches.value_of("honeycomb_dataset") {
            config.honeycomb_dataset = Some(value.into());
        }
        if let Some(value) = matches.value_of("honeycomb_token") {
            config.honeycomb_token = Some(value.into());
        }
        if let Some(value) = matches.value_of("honeycomb_trace_id") {
            config.honeycomb_trace_id = Some(value.into());
        }
        if let Some(value) = matches.value_of("honeycomb_parent_id") {
            config.honeycomb_parent_id = Some(value.into());
        }
        if let Some(values) = matches.values_of("honeycomb_kv") {
            config.honeycomb_kv.extend(values.map(|x| x.to_owned()));
        }
        if let Some(value) = matches.value_of("s3_bucket") {
            config.s3_bucket = Some(value.into());
        }
        if let Some(value) = matches.value_of("s3_bucket_objects") {
            config.s3_bucket_objects = Some(value.into());
        }
        if let Some(value) = matches.value_of("s3_region") {
            config.s3_region = Some(value.into());
        }
        if let Some(value) = matches.value_of("s3_endpoint") {
            config.s3_endpoint = Some(value.into());
        }
        if config.command_to_run.is_empty() {
            bail!("The command to run was not specified");
        }

        Ok(config)
    }

    pub fn get_honeycomb_kv(&self) -> Result<Vec<(String, String)>> {
        self.honeycomb_kv
            .iter()
            .map(|value| value.split_once('=').map(|(a, b)| (a.to_owned(), b.to_owned())))
            .collect::<Option<_>>()
            .ok_or_else(|| anyhow!("Can't parse honeycomb_kv"))
    }

    // Check if all paths match at least one of the specified outputs.
    pub fn outputs_match<'a, I: Iterator<Item = &'a Path>>(&self, paths: I) -> Result<bool> {
        // Take all patterns from globs in self.output_files
        let patterns = self
            .output_files
            .iter()
            .map(|path| glob::Pattern::from_str(path))
            .collect::<Result<Vec<glob::Pattern>, _>>()
            .with_context(|| "Invalid output file pattern")?;
        // For each given path, try to find at least one match in the patterns.
        for path in paths {
            let mut has_match = false;
            for pattern in &patterns {
                if pattern.matches_path(path) {
                    has_match = true;
                    break;
                }
            }
            if !has_match {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use serial_test::serial;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    #[serial] // Must serialize these tests so that env vars don't affect other tests.
    fn test_command_line_1() {
        env::set_var("CAPSULE_ARGS", "-c my_capsule -- /bin/echo");
        let config = Config::new(["capsule"], None, None);
        env::remove_var("CAPSULE_ARGS");
        let config = config.unwrap();
        assert_eq!(config.capsule_id.unwrap(), "my_capsule");
        assert_eq!(config.command_to_run[0], "/bin/echo");
    }

    #[test]
    #[serial]
    fn test_capsule_args_with_space() {
        env::set_var("CAPSULE_ARGS", "-c 'my capsule id' -- /bin/echo");
        let config = Config::new(["capsule"], None, None);
        env::remove_var("CAPSULE_ARGS");
        let config = config.unwrap();
        assert_eq!(config.capsule_id.unwrap(), "my capsule id");
        assert_eq!(config.command_to_run[0], "/bin/echo");
    }

    #[test]
    #[serial]
    fn test_command_line_2() {
        let config = Config::new(vec!["placebo", "-c", "my_capsule", "--", "/bin/echo"], None, None).unwrap();
        assert_eq!(config.get_honeycomb_kv().unwrap(), vec![]);
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
        default_config_file.write(config_contents.as_bytes()).unwrap();
        default_config_file.flush().unwrap();

        let mut current_config_file = NamedTempFile::new().unwrap();
        let config_contents: &'static str = indoc! {r#"
           [my_capsule]
           capture_stdout = false
           output_files=["compiled_binary"]
           input_files=["/etc/passwd", "/nonexistent"]
           tool_tags = ["docker-1234"]
        "#};
        current_config_file.write(config_contents.as_bytes()).unwrap();
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
        current_config_file.write(config_contents.as_bytes()).unwrap();
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
        current_config_file.write(config_contents.as_bytes()).unwrap();
        current_config_file.flush().unwrap();

        let config = Config::new(
            vec!["placebo", "--", "/bin/echo"],
            None,
            Some(current_config_file.path()),
        )
        .unwrap();
        assert_eq!(config.capsule_id, Some(String::from("my_capsule_id")));
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
        current_config_file.write(config_contents.as_bytes()).unwrap();
        current_config_file.flush().unwrap();

        Config::new(
            vec!["placebo", "--", "/bin/echo"],
            None,
            Some(current_config_file.path()),
        )
        .unwrap_err();
    }

    #[test]
    #[serial]
    fn test_honeycomb_kv() {
        let config = Config::new(
            vec![
                "placebo",
                "-c",
                "my_capsule",
                "--honeycomb_kv",
                "branch=master",
                "--",
                "/bin/echo",
            ],
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            config.get_honeycomb_kv().unwrap(),
            vec![("branch".to_owned(), "master".to_owned())]
        );
    }

    #[test]
    #[serial]
    fn test_honeycomb_kv_2() {
        let config = Config::new(
            vec![
                "placebo",
                "-c",
                "my_capsule",
                "--honeycomb_kv",
                "branch=master",
                "--honeycomb_kv",
                "foo=bar",
                "--",
                "/bin/echo",
            ],
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            config.get_honeycomb_kv().unwrap(),
            vec![
                ("branch".to_owned(), "master".to_owned()),
                ("foo".to_owned(), "bar".to_owned())
            ]
        );
        assert_eq!(config.capsule_id.unwrap(), "my_capsule");
        assert_eq!(config.command_to_run[0], "/bin/echo");
    }

    #[test]
    #[serial]
    fn test_honeycomb_kv_empty() {
        let config = Config::new(
            vec![
                "placebo",
                "-c",
                "my_capsule",
                "--honeycomb_kv=foo=",
                "--honeycomb_kv",
                "bar=",
                "--",
                "/bin/echo",
            ],
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            config.get_honeycomb_kv().unwrap(),
            vec![("foo".to_owned(), "".to_owned()), ("bar".to_owned(), "".to_owned())]
        );
    }
}
