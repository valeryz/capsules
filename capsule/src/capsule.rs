use anyhow::anyhow;
use anyhow::{Context, Result};

use glob::glob;

use crate::caching::backend::CachingBackend;
use crate::config::Config;
use crate::iohashing::*;
use crate::observability::logger::Logger;

pub struct Capsule<'a> {
    config: &'a Config,
    caching_backend: Box<dyn CachingBackend>,
    logger: Box<dyn Logger>,
    inputs: InputSet,
    // TODO(valeryz): enable it in Blue Pill.
    // outputs: OutputSet<'a>,
}

impl<'a> Capsule<'a> {
    pub fn new(config: &'a Config, caching_backend: Box<dyn CachingBackend>, logger: Box<dyn Logger>) -> Self {
        Self {
            config,
            caching_backend,
            logger,
            inputs: InputSet::default(),
            // outputs: OutputSet::default(),
        }
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

    pub fn hash(self) -> Result<String> {
        self.inputs.hash()
    }

    pub async fn write_cache(self) -> Result<()> {
        // Outputs bundle is ununsed in Placebo, creating an empty one.
        let output_bundle = OutputHashBundle {
            hash: "".into(),
            hash_details: vec![],
        };
        let capsule_id = self.config.capsule_id.as_ref().expect("capsule_id must be specified");
        let input_bundle = self
            .inputs
            .hash_bundle()
            .with_context(|| format!("Hashing inputs of capsule '{:?}'", capsule_id))?;

        // TODO: call the wrapped program.

        // TODO: calculate the output bundle.

        // We try logging for observability, but we will not stop it if there was a problem,
        // only try complaining about it.
        self.logger.log(&input_bundle, &output_bundle).unwrap_or_else(|err| {
            eprintln!("Failed to log results for observability: {:?}", err);
        });
        // Finally, we'll write our results to the caching backend.
        self.caching_backend.write(input_bundle, output_bundle).await
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
