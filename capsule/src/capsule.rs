use anyhow::anyhow;
use anyhow::{Context, Result};

use glob::glob;
use indoc::indoc;
use std::process::{Child, Command, ExitStatus};

use crate::caching::backend::CachingBackend;
use crate::config::Config;
use crate::iohashing::*;
use crate::observability::logger::Logger;

static USAGE: &str = "Usage: capsule <capsule arguments ...> -- command [<arguments>]";

pub struct Capsule<'a> {
    /// Indicates whether the program has been run within the capsule.
    pub program_run: bool,

    config: &'a Config,
    caching_backend: Box<dyn CachingBackend>,
    logger: Box<dyn Logger>,
}

impl<'a> Capsule<'a> {
    pub fn new(config: &'a Config, caching_backend: Box<dyn CachingBackend>, logger: Box<dyn Logger>) -> Self {
        Self {
            program_run: false,
            config,
            caching_backend,
            logger,
        }
    }

    pub fn capsule_id(&self) -> String {
        self.config.capsule_id.as_ref().cloned().unwrap()
    }

    pub fn hash(&self) -> Result<String> {
        self.read_inputs().map(|inputs| inputs.hash)
    }

    pub fn read_inputs(&self) -> Result<HashBundle> {
        let mut inputs = InputSet::default();
        for file_pattern in &self.config.input_files {
            for file in glob(file_pattern)? {
                let file = file?;
                if file.is_file() {
                    inputs.add_input(Input::File(file));
                }
            }
        }

        for tool_tag in &self.config.tool_tags {
            inputs.add_input(Input::ToolTag(tool_tag.clone()));
        }
        let capsule_id = self.capsule_id();
        inputs
            .hash_bundle()
            .with_context(|| format!("Hashing inputs of capsule '{}'", capsule_id))
    }

    pub fn read_outputs(&self, exit_code: Option<i32>) -> Result<OutputHashBundle> {
        let mut outputs = OutputSet::default();
        if let Some(exit_code) = exit_code {
            outputs.add_output(Output::ExitCode(exit_code));
        }
        for file_pattern in &self.config.output_files {
            for file in glob(file_pattern)? {
                let file = file?;
                if file.is_dir() {
                    continue;
                }
                if file.is_file() {
                    outputs.add_output(Output::File(FileOutput {
                        filename: file.to_path_buf(),
                        present: true,
                    }));
                } else {
                    outputs.add_output(Output::File(FileOutput {
                        filename: file.to_path_buf(),
                        present: false,
                    }));
                }
            }
        }
        let capsule_id = self.capsule_id();
        outputs
            .hash_bundle()
            .with_context(|| format!("Hashing outputs of capsule '{}'", capsule_id))
    }

    fn equal_outputs(left: &OutputHashBundle, right: &OutputHashBundle) -> bool {
        left.hash == right.hash
    }

    pub async fn run_capsule(&self, program_run: &mut bool) -> Result<ExitStatus> {
        let inputs = self.read_inputs()?;
        let lookup_result = self.caching_backend.lookup(&inputs).await?;
        let cache_hit = lookup_result.is_some();
        if cache_hit {
            println!(
                "Cache hit on {}: ignoring and proceeding with execution",
                self.capsule_id()
            );
        }

        // TODO: skip execution in blue pill on cache hit.

        eprintln!("Executing command: {:?}", self.config.command_to_run);
        let mut child = self.execute_command()?;
        // Having executed the command, just need to tell our caller
        // whether we succeeded in running the program.  this happens
        // as soon as we have a child program.
        *program_run = true;
        let exit_status = child.wait().with_context(|| "Waiting for child")?;

        // Now that we got the exit code, we try hard to pass it back to exit.
        // If we fail along the way, we should complain, but still continue.
        match self.read_outputs(exit_status.code()) {
            Ok(outputs) => {
                let non_determinism = cache_hit
                    && lookup_result.as_ref().map_or(false, |lookup_result| {
                        !Self::equal_outputs(&lookup_result.outputs, &outputs)
                    });

                if non_determinism {
                    eprintln!(
                        indoc! {"
                        Non-determinism detected:
                        Old: {:?}
                        vs
                        New: {:?}\n"},
                        &(lookup_result.unwrap().outputs),
                        &outputs
                    );
                }

                // TODO: make these truly async simultaneous to onw another. It is atm complicated
                // because caching_backend.write moves its args.

                // We try logging for observability, but we will not stop it if there was a problem,
                // only try complaining about it.
                self.logger
                    .log(&inputs, &outputs, non_determinism)
                    .await
                    .unwrap_or_else(|err| {
                        eprintln!("Failed to log results for observability: {}", err);
                    });
                self.caching_backend.write(inputs, outputs).await.unwrap_or_else(|err| {
                    eprintln!("Failed to write entry to cache: {}", err);
                });
            }
            Err(err) => {
                eprintln!("Failed to get command outputs: {}", err);
            }
        }
        Ok(exit_status)
    }

    pub fn execute_command(&self) -> Result<Child> {
        if self.config.command_to_run.is_empty() {
            Err(anyhow!(USAGE))
        } else {
            Command::new(&self.config.command_to_run[0])
                .args(&self.config.command_to_run[1..])
                .spawn()
                .with_context(|| "Spawning command")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::{
        fs::File,
        path::{Path, PathBuf},
    };

    use super::*;
    use crate::caching::dummy;
    use crate::observability::dummy::Dummy;
    use serial_test::serial;
    use tempfile::TempDir;

    const EMPTY_SHA256: &'static str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    #[serial]
    fn test_empty_capsule() {
        let backend = Box::new(dummy::DummyBackend::default());
        let config = Config::new(["capsule", "-c", "wtf", "--", "/bin/echo"], None, None).unwrap();
        let capsule = Capsule::new(&config, backend, Box::new(Dummy));
        assert_eq!(capsule.hash().unwrap(), EMPTY_SHA256);
    }

    #[test]
    #[serial]
    fn test_nonexistent_glob() {
        let backend = Box::new(dummy::DummyBackend::default());
        let config = Config::new(
            ["capsule", "-c", "wtf", "-i", "/nonexistent-glob", "--", "/bin/echo"],
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, backend, Box::new(Dummy));
        assert!(capsule.read_inputs().is_ok());
    }

    #[test]
    #[serial]
    fn test_ok_glob() {
        let backend = Box::new(dummy::DummyBackend::default());
        let config = Config::new(
            ["capsule", "-c", "wtf", "-i", "/bin/echo", "--", "/bin/echo"],
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, backend, Box::new(Dummy));
        let inputs = capsule.read_inputs();
        assert!(inputs.is_ok());
        assert!(inputs.unwrap().hash_details[0].0 == Input::File(Path::new("/bin/echo").into()));
    }

    #[test]
    #[serial]
    fn test_invalid_glob() {
        let backend = Box::new(dummy::DummyBackend::default());
        let config = Config::new(["capsule", "-c", "wtf", "-i", "***", "--", "/bin/echo"], None, None).unwrap();
        let capsule = Capsule::new(&config, backend, Box::new(Dummy));
        assert!(capsule.read_inputs().is_err());
    }

    fn create_file_tree(dir: &Path) -> PathBuf {
        let root = dir.join("root");
        fs::create_dir_all(root.join("dir1").join("subdir1")).unwrap();
        fs::create_dir_all(root.join("dir2").join("subdir2")).unwrap();
        File::create(root.join("123")).unwrap();
        File::create(root.join("dir1").join("111")).unwrap();
        File::create(root.join("dir1").join("222")).unwrap();
        File::create(root.join("dir2").join("subdir2").join("111")).unwrap();
        File::create(root.join("dir2").join("subdir2").join("222")).unwrap();
        root
    }

    #[test]
    #[serial]
    fn test_recursive_glob() {
        let tmp_dir = TempDir::new().unwrap();
        let root = create_file_tree(tmp_dir.path());
        let backend = Box::new(dummy::DummyBackend::default());
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "-i",
                &format!("{}/**/111", root.to_str().unwrap()),
                "--",
                "/bin/echo",
            ],
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, backend, Box::new(Dummy));
        let inputs = capsule.read_inputs();
        assert!(inputs.is_ok());
        let inputs = inputs.unwrap();
        assert_eq!(
            inputs.hash_details[0].0,
            Input::File(root.join("dir1").join("111").into())
        );
        assert_eq!(
            inputs.hash_details[1].0,
            Input::File(root.join("dir2").join("subdir2").join("111").into())
        );
    }

    #[test]
    #[serial]
    fn test_single_glob() {
        let tmp_dir = TempDir::new().unwrap();
        let root = create_file_tree(tmp_dir.path());
        let backend = Box::new(dummy::DummyBackend::default());
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "-i",
                &format!("{}/*/111", root.to_str().unwrap()),
                "--",
                "/bin/echo",
            ],
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, backend, Box::new(Dummy));
        let inputs = capsule.read_inputs();
        assert!(inputs.is_ok());
        assert_eq!(
            inputs.unwrap().hash_details[0].0,
            Input::File(root.join("dir1").join("111").into())
        );
    }

    #[test]
    #[serial]
    fn test_full_recursive_glob() {
        let tmp_dir = TempDir::new().unwrap();
        let root = create_file_tree(tmp_dir.path());
        let backend = Box::new(dummy::DummyBackend::default());
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "-i",
                &format!("{}/**/*", root.to_str().unwrap()),
                "--",
                "/bin/echo",
            ],
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, backend, Box::new(Dummy));
        let inputs = capsule.read_inputs();
        assert_eq!(
            inputs
                .unwrap()
                .hash_details
                .into_iter()
                .map(|x| x.0)
                .collect::<Vec<_>>(),
            [
                Input::File(root.join("123").into()),
                Input::File(root.join("dir1").join("111").into()),
                Input::File(root.join("dir1").join("222").into()),
                Input::File(root.join("dir2").join("subdir2").join("111").into()),
                Input::File(root.join("dir2").join("subdir2").join("222").into())
            ]
        );
    }
}
