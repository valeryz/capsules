use anyhow::anyhow;
use anyhow::{Context, Result};

use futures::future::try_join_all;
use futures::join;
use glob::glob;
use indoc::indoc;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::prelude::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};

use crate::caching::backend::CachingBackend;
use crate::config::{Config, Milestone};
use crate::iohashing::*;
use crate::observability::logger::Logger;

static USAGE: &str = "Usage: capsule <capsule arguments ...> -- command [<arguments>]";

pub struct Capsule<'a> {
    /// Indicates whether the program has been run within the capsule.
    pub program_run: bool,

    config: &'a Config,
    caching_backend: &'a dyn CachingBackend,
    logger: &'a dyn Logger,
}

impl<'a> Capsule<'a> {
    pub fn new(config: &'a Config, caching_backend: &'a dyn CachingBackend, logger: &'a dyn Logger) -> Self {
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

    pub fn read_inputs(&self) -> Result<InputHashBundle> {
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
            let mut present = false;
            for file in glob(file_pattern)? {
                let file = file?;
                if file.is_dir() {
                    continue;
                }
                if file.is_file() {
                    outputs.add_output(Output::File(FileOutput {
                        filename: file.to_path_buf(),
                        present: true,
                        mode: file.metadata()?.permissions().mode(),
                    }));
                    present = true;
                }
            }
            if !present {
                // This seems to be a file that hasn't matched.
                outputs.add_output(Output::File(FileOutput {
                    filename: PathBuf::from(file_pattern),
                    present: false,
                    mode: 0o644, // Default permissions just in case.
                }));
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

    async fn execute_and_cache(
        &self,
        inputs: &InputHashBundle,
        lookup_result: &Option<InputOutputBundle>,
        program_run: &mut AtomicBool,
    ) -> Result<ExitStatus> {
        eprintln!("Executing command: {:?}", self.config.command_to_run);
        let mut child = self.execute_command(inputs).await?;
        // Having executed the command, just need to tell our caller
        // whether we succeeded in running the program.  this happens
        // as soon as we have a child program.
        program_run.store(true, Ordering::SeqCst);
        let exit_status = child.wait().await.with_context(|| "Waiting for child")?;

        // Now that we got the exit code, we try hard to pass it back to exit.
        // If we fail along the way, we should complain, but still continue.
        match self.read_outputs(exit_status.code()) {
            Ok(outputs) => {
                let non_determinism = lookup_result.as_ref().map_or(false, |lookup_result| {
                    !Self::equal_outputs(&lookup_result.outputs, &outputs)
                });

                if non_determinism {
                    eprintln!(
                        indoc! {"
                        Non-determinism detected:
                        Old: {:?}
                        vs
                        New: {:?}\n"},
                        lookup_result.as_ref().unwrap().outputs,
                        &outputs
                    );
                }

                let logger_fut = self.logger.log(inputs, &outputs, false, non_determinism);
                let cache_write_fut = self.caching_backend.write(inputs, &outputs);
                let cache_writefiles_fut = self.upload_files(&outputs);
                let (logger_result, cache_result, cache_files_result) =
                    join!(logger_fut, cache_write_fut, cache_writefiles_fut);
                logger_result.unwrap_or_else(|err| {
                    eprintln!("Failed to log results for observability: {}", err);
                });
                cache_result.unwrap_or_else(|err| {
                    eprintln!("Failed to write entry to cache: {}", err);
                });
                cache_files_result.unwrap_or_else(|err| {
                    eprintln!("Failed to write output files to cache: {}", err);
                });
            }
            Err(err) => {
                eprintln!("Failed to get command outputs: {}", err);
            }
        }
        Ok(exit_status)
    }

    /// Download all output files from the caching backend, and place them into destination paths.
    async fn download_files(&self, outputs: &OutputHashBundle) -> Result<()> {
        // First, try removing files that should not be present, and bail out if we fail with that,
        // before starting any S3 downloads.
        for (item, _) in &outputs.hash_details {
            if let Output::File(ref fileoutput) = item {
                if !fileoutput.present {
                    // If the file should not be present, let's remove it, ignoring ENOENT.
                    if let Err(e) = std::fs::remove_file(&fileoutput.filename) {
                        if !matches!(e.kind(), ErrorKind::NotFound) {
                            return Err(e.into());
                        }
                    }
                }
            }
        }
        // Now download all files that should be present.
        let mut all_files_futures = Vec::new();
        for (item, item_hash) in &outputs.hash_details {
            if let Output::File(ref fileoutput) = item {
                if fileoutput.present {
                    let dir = fileoutput.filename.parent().context("No parent directory")?;
                    std::fs::create_dir_all(dir)?;
                    let file = NamedTempFile::new_in(dir)?;
                    let (file, path) = file.into_parts();
                    let mut file_stream = tokio::fs::File::from_std(file);
                    let download_file_fut = async move {
                        let mut file_body_reader = self.caching_backend.download_object_file(item_hash).await?;
                        tokio::io::copy(&mut file_body_reader, &mut file_stream).await?;
                        file_stream.flush().await?;
                        path.persist(&fileoutput.filename)?;
                        std::fs::set_permissions(
                            &fileoutput.filename,
                            std::fs::Permissions::from_mode(fileoutput.mode),
                        )?;
                        Ok::<(), anyhow::Error>(())
                    };
                    all_files_futures.push(download_file_fut);
                }
            }
        }
        try_join_all(all_files_futures).await?;
        Ok(())
    }

    /// Upload output files into S3, keyed by their hash (content addressed).
    async fn upload_files(&self, outputs: &OutputHashBundle) -> Result<()> {
        let mut all_files_futures = Vec::new();
        for (item, item_hash) in &outputs.hash_details {
            if let Output::File(ref fileoutput) = item {
                if fileoutput.present {
                    let tokio_file = tokio::fs::File::open(&fileoutput.filename).await?;
                    let content_length = tokio_file.metadata().await?.len();
                    all_files_futures.push(self.caching_backend.upload_object_file(
                        item_hash,
                        Box::pin(tokio_file),
                        content_length,
                    ));
                }
            }
        }
        try_join_all(all_files_futures).await?;
        Ok(())
    }

    pub async fn run_capsule(&self, program_run: &mut AtomicBool) -> Result<i32> {
        let inputs = self.read_inputs()?;
        let lookup_result = self.caching_backend.lookup(&inputs).await?;
        if let Some(ref lookup_result) = lookup_result {
            // We have a cache hit, but in case we are in placebo mode, or we have cached a failure,
            // we should still not use the cache. Let's figure this out while printing the solution.
            let mut use_cache = true;
            if self.config.milestone == Milestone::Placebo {
                println!(
                    "Cache hit on {}: ignoring and proceeding with execution.",
                    self.capsule_id()
                );
                use_cache = false
            } else {
                if !self.config.cache_failure {
                    // If result code from the command is not 0
                    if lookup_result.outputs.result_code().unwrap_or(1) != 0 {
                        println!(
                            "Cache hit on {}: cached failure, proceeding with execution.",
                            self.capsule_id()
                        );
                        use_cache = false;
                    }
                }
                // Check whether we should avoid caching when output files from the cache hit
                // don't match with the capsule output files from config.
                if use_cache {
                    // a predicate selecting all paths for Output::Files from all cached outputs.
                    fn predicate<X>((output, _): &(Output, X)) -> Option<&Path> {
                        if let Output::File(fileoutput) = output {
                            if fileoutput.present {
                                return Some(fileoutput.filename.as_path());
                            }
                        }
                        None
                    }
                    let iter = lookup_result.outputs.hash_details.iter().filter_map(predicate);
                    // If anything doesn't match, don't use the cache!
                    if !self.config.outputs_match(iter)? {
                        eprintln!(
                            "Cache hit on {}: mismatch in output patterns, proceeding with execution",
                            self.capsule_id()
                        );
                        use_cache = false;
                    }
                }
            }

            if use_cache {
                match self.download_files(&lookup_result.outputs).await {
                    Ok(_) => {
                        println!("Cache hit on {}: success.", self.capsule_id());
                        // Log successful cached results.
                        self.logger
                            .log(&inputs, &lookup_result.outputs, true, false)
                            .await
                            .unwrap_or_else(|err| {
                                eprintln!("Failed to log results for observability: {}", err);
                            });
                        return Ok(lookup_result.outputs.result_code().unwrap_or(127));
                    }
                    Err(e) => {
                        println!(
                            "Cache hit on {}: failed to retrieve from the cache: {}",
                            self.capsule_id(),
                            e
                        );
                    }
                }
            }
        }

        // If we got here, we should execute.
        self.execute_and_cache(&inputs, &lookup_result, program_run)
            .await
            .map(|exit_status| exit_status.into_raw())
    }

    async fn execute_command(&self, inputs: &InputHashBundle) -> Result<Child> {
        if self.config.command_to_run.is_empty() {
            Err(anyhow!(USAGE))
        } else {
            Command::new(&self.config.command_to_run[0])
                .args(&self.config.command_to_run[1..])
                .env("CAPSULE_INPUTS_HASH", &inputs.hash)
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
    use crate::caching::test::{TestBackend, TestBackendConfig};
    use crate::observability::dummy::Dummy;
    use serial_test::serial;
    use tempfile::TempDir;

    const EMPTY_SHA256: &'static str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    #[serial]
    fn test_empty_capsule() {
        let backend = dummy::DummyBackend::default();
        let config = Config::new(["capsule", "-c", "wtf", "--", "/bin/echo"].iter(), None, None).unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        assert_eq!(capsule.read_inputs().unwrap().hash, EMPTY_SHA256);
    }

    #[tokio::test]
    #[serial]
    async fn test_capsule_inputs_hash_env() {
        let tmp_dir = TempDir::new().unwrap();
        let out_file = tmp_dir.path().join("xx");
        let backend = dummy::DummyBackend::default();
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "--",
                "/bin/bash",
                "-c",
                &format!("echo -n ${{CAPSULE_INPUTS_HASH}} > {}", out_file.to_string_lossy()),
            ]
            .iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        assert_eq!(capsule.read_inputs().unwrap().hash, EMPTY_SHA256);
        let mut program_run = AtomicBool::new(false);
        let _ = capsule.run_capsule(&mut program_run).await.unwrap();
        let out_file_contents = std::fs::read_to_string(out_file).unwrap();
        assert_eq!(out_file_contents, EMPTY_SHA256);
    }

    #[test]
    #[serial]
    fn test_nonexistent_glob() {
        let backend = dummy::DummyBackend::default();
        let config = Config::new(
            ["capsule", "-c", "wtf", "-i", "/nonexistent-glob", "--", "/bin/echo"].iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        assert!(capsule.read_inputs().is_ok());
    }

    #[test]
    #[serial]
    fn test_ok_glob() {
        let backend = dummy::DummyBackend::default();
        let config = Config::new(
            ["capsule", "-c", "wtf", "-i", "/bin/echo", "--", "/bin/echo"].iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let inputs = capsule.read_inputs();
        assert!(inputs.is_ok());
        assert!(inputs.unwrap().hash_details[0].0 == Input::File(Path::new("/bin/echo").into()));
    }

    #[test]
    #[serial]
    fn test_invalid_glob() {
        let backend = dummy::DummyBackend::default();
        let config = Config::new(
            ["capsule", "-c", "wtf", "-i", "***", "--", "/bin/echo"].iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
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
        let backend = dummy::DummyBackend::default();
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "-i",
                &format!("{}/**/111", root.to_str().unwrap()),
                "--",
                "/bin/echo",
            ]
            .iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
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
        let backend = dummy::DummyBackend::default();
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "-i",
                &format!("{}/*/111", root.to_str().unwrap()),
                "--",
                "/bin/echo",
            ]
            .iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
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
        let backend = dummy::DummyBackend::default();
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "-i",
                &format!("{}/**/*", root.to_str().unwrap()),
                "--",
                "/bin/echo",
            ]
            .iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
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

    #[tokio::test]
    #[serial]
    async fn test_cache_hit() {
        let tmp_dir = TempDir::new().unwrap();
        let backend = TestBackend::new("wtf", TestBackendConfig::default());
        let out_file_1 = tmp_dir.path().join("xx");
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "-i",
                "/bin/echo",
                "-o",
                out_file_1.to_str().unwrap(),
                "--",
                "/bin/bash",
                "-c",
                &format!("echo '123' > {}", out_file_1.to_str().unwrap()),
            ]
            .iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        assert!(program_run.load(Ordering::SeqCst));

        std::fs::remove_file(&out_file_1).unwrap();

        // 2nd should be cached, and command not run.
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        // The 2nd time the program should not be run.
        assert!(!program_run.load(Ordering::SeqCst));

        assert!(out_file_1.is_file());
    }

    #[tokio::test]
    #[serial]
    async fn test_cache_miss() {
        let tmp_dir = TempDir::new().unwrap();
        let backend = TestBackend::new("wtf", TestBackendConfig::default());
        let out_file_1 = tmp_dir.path().join("xx");
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "-i",
                "/bin/echo",
                "-o",
                out_file_1.to_str().unwrap(),
                "--",
                "/bin/bash",
                "-c",
                &format!("echo '123' > {}", out_file_1.to_str().unwrap()),
            ]
            .iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        assert!(program_run.load(Ordering::SeqCst));

        std::fs::remove_file(&out_file_1).unwrap();

        backend.remove_all();

        // 2nd should NOT be cached, and command run.
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        // The 2nd time the program should be run, as there's no cache hit.
        assert!(program_run.load(Ordering::SeqCst));

        assert!(out_file_1.is_file());
    }

    #[tokio::test]
    #[serial]
    async fn test_cache_miss_capsule_id() {
        let tmp_dir = TempDir::new().unwrap();
        let backend = TestBackend::new("wtf1", TestBackendConfig::default());
        let out_file_1 = tmp_dir.path().join("xx");
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf1",
                "-i",
                "/bin/echo",
                "-o",
                out_file_1.to_str().unwrap(),
                "--",
                "/bin/bash",
                "-c",
                &format!("echo '123' > {}", out_file_1.to_str().unwrap()),
            ]
            .iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        assert!(program_run.load(Ordering::SeqCst));

        std::fs::remove_file(&out_file_1).unwrap();

        let backend = TestBackend::new("wtf2", TestBackendConfig::default());
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf2", // The only difference is capsule_id, but it should not cache.
                "-i",
                "/bin/echo",
                "-o",
                out_file_1.to_str().unwrap(),
                "--",
                "/bin/bash",
                "-c",
                &format!("echo '123' > {}", out_file_1.to_str().unwrap()),
            ]
            .iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        // The 2nd time the program should be run, as there's no cache hit.
        assert!(program_run.load(Ordering::SeqCst));

        assert!(out_file_1.is_file());
    }

    #[tokio::test]
    #[serial]
    async fn test_cache_failed_lookup() {
        let backend = TestBackend::new(
            "wtf",
            TestBackendConfig {
                failing_lookup: true,
                ..Default::default()
            },
        );
        let config = Config::new(
            ["capsule", "-c", "wtf", "-i", "/bin/echo", "--", "/bin/echo"].iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await;
        assert!(code.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn test_cache_hit_failure_object() {
        let tmp_dir = TempDir::new().unwrap();
        let backend = TestBackend::new(
            "wtf",
            TestBackendConfig {
                failing_download_files: true,
                ..Default::default()
            },
        );
        let out_file_1 = tmp_dir.path().join("xx");
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "-i",
                "/bin/echo",
                "-o",
                out_file_1.to_str().unwrap(),
                "--",
                "/bin/bash",
                "-c",
                &format!("echo '123' > {}", out_file_1.to_str().unwrap()),
            ]
            .iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        assert!(program_run.load(Ordering::SeqCst));

        std::fs::remove_file(&out_file_1).unwrap();

        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        assert!(program_run.load(Ordering::SeqCst));
        assert!(out_file_1.is_file());
    }

    #[tokio::test]
    #[serial]
    async fn test_cache_hit_permissions() {
        let tmp_dir = TempDir::new().unwrap();
        let backend = TestBackend::new("wtf", TestBackendConfig::default());
        let out_file = tmp_dir.path().join("xx");
        let out_file_name = out_file.to_string_lossy();
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "-i",
                "/bin/echo",
                "-o",
                &out_file_name,
                "--",
                "/bin/bash",
                "-c",
                &format!("echo '123' > {}; chmod 755 {}", out_file_name, out_file_name),
            ]
            .iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        assert!(program_run.load(Ordering::SeqCst));

        std::fs::remove_file(&out_file).unwrap();

        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        // The 2nd time the program should NOT run.
        assert!(!program_run.load(Ordering::SeqCst));
        assert!(out_file.is_file());
        assert_eq!(out_file.metadata().unwrap().permissions().mode() & 0o777, 0o755);
    }

    #[tokio::test]
    #[serial]
    async fn test_cache_file_removal() {
        let tmp_dir = TempDir::new().unwrap();
        let backend = TestBackend::new("wtf", TestBackendConfig::default());
        let out_file = tmp_dir.path().join("xx");
        let out_file_name = out_file.to_string_lossy();
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "-i",
                "/bin/echo",
                "-o",
                &out_file_name,
                "--",
                "/bin/echo",
            ]
            .iter(),
            None,
            None,
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        assert!(program_run.load(Ordering::SeqCst));

        // Create the file
        std::fs::File::create(&out_file).unwrap();
        assert!(out_file.exists());

        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        // The 2nd time the program should NOT run.
        assert!(!program_run.load(Ordering::SeqCst));

        // Because the out file was not present when the run was cached, we should expect it
        // to be removed.
        assert!(!out_file.exists());
    }
}
