use anyhow::anyhow;
use anyhow::{Context, Result};

use glob::glob;
use std::env;
use std::process::{Command, Child, ExitStatus};

use crate::caching::backend::CachingBackend;
use crate::config::Config;
use crate::iohashing::*;
use crate::observability::logger::Logger;

        
static USAGE: &'static str = "Usage: capsule <capsule arguments ...> -- command [<arguments>]";

struct Capsule<'a> {
    /// Indicates whether the program has been run within the capsule.
    pub program_run: bool,

    config: &'a Config,
    caching_backend: Box<dyn CachingBackend>,
    logger: Box<dyn Logger>,
    inputs: InputSet,
    outputs: OutputSet,
}

impl<'a> Capsule<'a> {
    pub fn new(config: &'a Config, caching_backend: Box<dyn CachingBackend>, logger: Box<dyn Logger>) -> Self {
        Self {
            program_run: false,
            config,
            caching_backend,
            logger,
            inputs: InputSet::default(),
            outputs: OutputSet::default(),
        }
    }

    pub fn hash(self) -> Result<String> {
        self.inputs.hash()
    }

    pub fn capsule_id(&self) -> String {
        self.config.capsule_id.as_ref().cloned().unwrap()
    }

    pub fn read_inputs(&mut self) -> Result<()> {
        for file_pattern in &self.config.input_files {
            let mut count = 0;
            for file in glob(file_pattern)? {
                let file = file?;
                if file.is_file() {
                    self.inputs.add_input(Input::File(file));
                    count += 1
                }
            }
            if count == 0 {
                return Err(anyhow!("Not found: '{}'", file_pattern));
            }
        }

        for tool_tag in &self.config.tool_tags {
            self.inputs.add_input(Input::ToolTag(tool_tag.clone()));
        }
        Ok(())
    }

    pub fn read_outputs(&mut self) -> Result<()> {
        for file_pattern in &self.config.output_files {
            for file in glob(file_pattern)? {
                let file = file?;
                if file.is_file() {
                    self.outputs.add_output(Output::File(FileOutput {
                        filename: file.to_path_buf(),
                        present: true,
                    }));
                } else {
                    self.outputs.add_output(Output::File(FileOutput {
                        filename: file.to_path_buf(),
                        present: false,
                    }));
                }
            }
        }
        Ok(())
    }

    // TODO: WTF is that thing!
    pub fn run_capsule(&mut self, program_run: &mut bool) -> Result<(HashBundle, OutputHashBundle, ExitStatus)> {
        self.read_inputs()
        .and_then(|_| self.get_inputs_bundle())
        .and_then(|inputs| {
            self.execute_command().and_then(|child| {
                // We just need to tell our caller whether we succeeded in running the program.
                // this happens as soon as we have a child program.
                *program_run = true;
                child.wait()
                    .with_context(|| "Waiting for child")
                    .and_then(
                        |exit_code| {
                            self.read_outputs().and_then(|_| {
                                self.get_outputs_bundle(exit_code)
                                    .map(|outputs| (inputs, outputs, exit_code))
                            })
                        })
            })
        })
    }

    pub fn get_inputs_bundle(self) -> Result<HashBundle> {
        let capsule_id = self.capsule_id();
        self.inputs
            .hash_bundle()
            .with_context(|| format!("Hashing inputs of capsule '{:?}'", capsule_id))
    }

    pub fn get_outputs_bundle(self, exit_status: ExitStatus) -> Result<OutputHashBundle> {
        let capsule_id = self.capsule_id();
        self.outputs
            .hash_bundle()
            .with_context(|| format!("Hashing outputs of capsule '{:?}'", capsule_id))
    }

    pub fn execute_command(&self) -> Result<Child> {
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
        Command::new("/bin/bash")
            .arg("-c")
            .arg(s)
            .spawn()
            .with_context(|| format!("Spawning command"))
    }
    
    pub async fn write_cache(self) -> Result<i32> {
        let mut program_run = false;
        let capsule_id = self.config.capsule_id.as_ref().expect("capsule_id must be specified");
        let input_bundle = self
            .inputs
            .hash_bundle()
            .with_context(|| format!("Hashing inputs of capsule '{:?}'", capsule_id))?;

        program_run = true;

        let output_bundle = self
            .outputs
            .hash_bundle()
            .with_context(|| format!("Hashing outputs of capsule '{:?}'", capsule_id))?;

        // We try logging for observability, but we will not stop it if there was a problem,
        // only try complaining about it.
        self.logger.log(&input_bundle, &output_bundle).unwrap_or_else(|err| {
            eprintln!("Failed to log results for observability: {:?}", err);
        });
        // Finally, we'll write our results to the caching backend.
        self.caching_backend.write(input_bundle, output_bundle).await?;
        Ok(0) // TODO: change this to the return code for the program.
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
        let mut capsule = Capsule::new(&config, backend, Box::new(Dummy));
        capsule.read_inputs().unwrap();
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
        let mut capsule = Capsule::new(&config, backend, Box::new(Dummy));
        assert!(capsule.read_inputs().is_err());
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
        let mut capsule = Capsule::new(&config, backend, Box::new(Dummy));
        assert!(capsule.read_inputs().is_ok());
        assert!(capsule.inputs.inputs[0] == Input::File(Path::new("/bin/echo").into()));
    }

    #[test]
    #[serial]
    fn test_invalid_glob() {
        let backend = Box::new(dummy::DummyBackend::default());
        let config = Config::new(["capsule", "-c", "wtf", "-i", "***", "--", "/bin/echo"], None, None).unwrap();
        let mut capsule = Capsule::new(&config, backend, Box::new(Dummy));
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
        let mut capsule = Capsule::new(&config, backend, Box::new(Dummy));
        assert!(capsule.read_inputs().is_ok());
        assert_eq!(
            capsule.inputs.inputs,
            [
                Input::File(root.join("dir1").join("111").into()),
                Input::File(root.join("dir2").join("subdir2").join("111").into())
            ]
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
        let mut capsule = Capsule::new(&config, backend, Box::new(Dummy));
        assert!(capsule.read_inputs().is_ok());
        assert_eq!(
            capsule.inputs.inputs,
            [Input::File(root.join("dir1").join("111").into()),]
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
        let mut capsule = Capsule::new(&config, backend, Box::new(Dummy));
        capsule.read_inputs().unwrap();
        assert_eq!(
            capsule.inputs.inputs,
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
