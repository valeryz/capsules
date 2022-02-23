use anyhow::anyhow;
use anyhow::{Context, Result};

use futures::join;
use futures::stream::{StreamExt, TryStreamExt};
use glob::glob;
use indoc::indoc;
use log::{error, info};
use std::os::unix::fs::PermissionsExt;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::{task, time};

use crate::caching::backend::CachingBackend;
use crate::config::{Config, Milestone};
use crate::iohashing::*;
use crate::observability::logger::Logger;
use crate::workspace_path::WorkspacePath;

static USAGE: &str = "Usage: capsule <capsule arguments ...> -- command [<arguments>]";

#[cfg(not(test))]
mod timeouts {
    pub(super) const TIMEOUT_LOOKUP_MILLIS: u64 = 10_000;
    pub(super) const TIMEOUT_LOGGING_MILLIS: u64 = 10_000;
    pub(super) const TIMEOUT_CACHE_WRITE_MILLIS: u64 = 10_000;
    pub(super) const TIMEOUT_UPLOAD_MILLIS: u64 = 600_000;
    pub(super) const TIMEOUT_DOWNLOAD_MILLIS: u64 = 600_000;
}

// Timeout constants to be used in unit tests.
#[cfg(test)]
mod timeouts {
    pub(super) const TIMEOUT_LOOKUP_MILLIS: u64 = 200;
    pub(super) const TIMEOUT_LOGGING_MILLIS: u64 = 200;
    pub(super) const TIMEOUT_CACHE_WRITE_MILLIS: u64 = 200;
    pub(super) const TIMEOUT_UPLOAD_MILLIS: u64 = 200;
    pub(super) const TIMEOUT_DOWNLOAD_MILLIS: u64 = 200;
}

pub struct Capsule<'a> {
    config: &'a Config,
    caching_backend: &'a dyn CachingBackend,
    logger: &'a dyn Logger,
}

impl<'a> Capsule<'a> {
    pub fn new(config: &'a Config, caching_backend: &'a dyn CachingBackend, logger: &'a dyn Logger) -> Self {
        Self {
            config,
            caching_backend,
            logger,
        }
    }

    pub fn capsule_id(&self) -> String {
        self.config.capsule_id.as_ref().cloned().unwrap()
    }

    pub fn capsule_job(&self) -> String {
        self.config.capsule_job.as_ref().cloned().unwrap_or_default()
    }

    pub fn read_inputs(&self) -> Result<InputHashBundle> {
        let mut inputs = InputSet::default();
        for file_pattern in &self.config.input_files {
            let mut file_count = 0;
            let fp = file_pattern.to_path(&self.config.workspace_root)?;
            let glob_pattern = fp.to_str().ok_or(anyhow!("can't convert path to string"))?;
            for file in glob(glob_pattern)? {
                let file = file?;
                if file.is_file() {
                    // Convert workspace relative patterns to workspace relative expansions.
                    let expansion_file_name = match *file_pattern {
                        WorkspacePath::NonWorkspace(_) => WorkspacePath::NonWorkspace(file),
                        WorkspacePath::Workspace(_) => WorkspacePath::Workspace(file),
                    };
                    inputs.add_input(Input::File(expansion_file_name));
                    file_count += 1;
                }
            }
            if file_count == 0 {
                return Err(anyhow!("Pattern '{}' didn't match any files", file_pattern));
            }
        }

        for tool_tag in &self.config.tool_tags {
            inputs.add_input(Input::ToolTag(tool_tag.clone()));
        }
        let capsule_id = self.capsule_id();
        inputs
            .hash_bundle(&self.config.workspace_root)
            .with_context(|| format!("Hashing inputs of capsule '{}'", capsule_id))
    }

    pub fn read_outputs(&self, exit_code: Option<i32>) -> Result<OutputHashBundle> {
        let mut outputs = OutputSet::default();
        if let Some(exit_code) = exit_code {
            outputs.add_output(Output::ExitCode(exit_code));
        }
        for file_pattern in &self.config.output_files {
            let fp = file_pattern.to_path(&self.config.workspace_root)?;
            let glob_pattern = fp.to_str().ok_or(anyhow!("can't convert path to string"))?;
            let mut present = false;
            for file in glob(glob_pattern)? {
                let file = file?;
                if file.is_dir() {
                    continue;
                }
                if file.is_file() {
                    // Convert workspace relative patterns to workspace relative expansions.
                    let mode = file.metadata()?.permissions().mode();
                    let expansion_file_name =
                        WorkspacePath::from_full_path(file.as_path(), &self.config.workspace_root);
                    outputs.add_output(Output::File(FileOutput {
                        filename: expansion_file_name,
                        present: true,
                        mode,
                    }));
                    present = true;
                }
            }
            if !present {
                // This seems to be a file that hasn't matched.
                outputs.add_output(Output::File(FileOutput {
                    filename: file_pattern.clone(),
                    present: false,
                    mode: 0o644, // Default permissions just in case.
                }));
            }
        }
        let capsule_id = self.capsule_id();
        outputs
            .hash_bundle(&self.config.workspace_root)
            .with_context(|| format!("Hashing outputs of capsule '{}'", capsule_id))
    }

    fn equal_outputs(left: &OutputHashBundle, right: &OutputHashBundle) -> bool {
        left.hash == right.hash
    }

    async fn execute_command(&self, inputs: &InputHashBundle, program_run: &mut AtomicBool) -> Result<ExitStatus> {
        info!("Executing command: {:?}", self.config.command_to_run);
        if self.config.command_to_run.is_empty() {
            Err(anyhow!(USAGE))
        } else {
            let mut child = Command::new(&self.config.command_to_run[0])
                .args(&self.config.command_to_run[1..])
                .env(&self.config.inputs_hash_var, &inputs.hash)
                .spawn()
                .with_context(|| "Spawning command")?;
            // Having executed the command, just need to tell our caller whether we succeeded in
            // running the program.  this happens as soon as we have a child program.
            program_run.store(true, Ordering::SeqCst);
            let exit_status = child.wait().await?;
            Ok(exit_status)
        }
    }

    async fn execute_and_cache(
        &self,
        inputs: &InputHashBundle,
        lookup_result: &Option<InputOutputBundle>,
        program_run: &mut AtomicBool,
    ) -> Result<ExitStatus> {
        let exit_status = self
            .execute_command(inputs, program_run)
            .await
            .with_context(|| "Waiting for child")?;
        // Now that we got the exit code, we try hard to pass it back to exit.
        // If we fail along the way, we should complain, but still continue.
        match self.read_outputs(exit_status.code()) {
            Ok(outputs) => {
                let non_determinism = lookup_result.as_ref().map_or(false, |lookup_result| {
                    !Self::equal_outputs(&lookup_result.outputs, &outputs)
                });

                if non_determinism {
                    error!(
                        indoc! {"
                        Non-determinism detected:
                        Old: {:?}
                        vs
                        New: {:?}\n"},
                        lookup_result.as_ref().unwrap().outputs,
                        &outputs
                    );
                }

                // Concurrently write the log, cache entry and cache objects (files).
                // The larger of each of the timeouts is applied to the combined branch.
                let logger_fut = time::timeout(
                    Duration::from_millis(timeouts::TIMEOUT_LOGGING_MILLIS),
                    self.logger.log(inputs, &outputs, false, non_determinism),
                );
                let cache_write_fut = time::timeout(
                    Duration::from_millis(timeouts::TIMEOUT_CACHE_WRITE_MILLIS),
                    self.caching_backend.write(inputs, &outputs, self.capsule_job()),
                );
                let upload_fut = time::timeout(
                    Duration::from_millis(timeouts::TIMEOUT_UPLOAD_MILLIS),
                    self.upload_files(&outputs),
                );
                let (logger_result, cache_result, upload_result) = join!(logger_fut, cache_write_fut, upload_fut);

                // If any of the above failed, we should just complain in the output, no need
                // to return and error, or interrupt the flow - the errors are affecting caching
                // or logging, but the wrapped binary had already been run by now.
                if let Ok(result) = logger_result {
                    result.unwrap_or_else(|err| {
                        error!("Failed to log results for observability: {}", err);
                    });
                } else {
                    error!("Time out logging results for observability");
                }

                if let Ok(result) = cache_result {
                    result.unwrap_or_else(|err| {
                        error!("Failed to write entry to cache: {}", err);
                    });
                } else {
                    error!("Time out writing entry to cache");
                }

                if let Ok(result) = upload_result {
                    result.unwrap_or_else(|err| {
                        error!("Failed to upload files to cache: {}", err);
                    });
                } else {
                    error!("Time out uploading files to cache");
                }
            }
            Err(err) => {
                error!("Failed to get command outputs: {}", err);
            }
        }
        Ok(exit_status)
    }

    /// Download all output files from the caching backend, and place them into destination paths.
    async fn download_files(&self, outputs: &OutputHashBundle) -> Result<()> {
        // Now download all files that should be present.
        let mut all_files_futures = Vec::new();
        // This loop generates futures for all downloadable files, and places them
        // into all_files_futures.
        for (item, item_hash) in &outputs.hash_details {
            if let Output::File(ref fileoutput) = item {
                if fileoutput.present {
                    info!("Downloading file '{}' hash '{}'", fileoutput.filename, item_hash);
                    let filename = fileoutput.filename.to_path(&self.config.workspace_root)?;
                    let dir = filename.parent().context("No parent directory")?;
                    std::fs::create_dir_all(dir)?;
                    let file = NamedTempFile::new_in(dir)?;
                    let (file, path) = file.into_parts();
                    let mut file_stream = tokio::fs::File::from_std(file);
                    let download_file_fut = async move {
                        let mut file_body_reader = self.caching_backend.download_object_file(item_hash).await?;
                        tokio::io::copy(&mut file_body_reader, &mut file_stream).await?;
                        file_stream.flush().await?;
                        info!("File {} downloaded, verifying hash", fileoutput.filename);
                        // Calculating the SHA256 is a long CPU bound op, better do in a thread.
                        let tmp_path = path.to_path_buf();
                        let received_hash = task::spawn_blocking(move || file_hash(&tmp_path)).await??;
                        if received_hash != *item_hash {
                            return Err(anyhow!("Mismatch of the downloaded file hash"));
                        }
                        path.persist(&filename)?;
                        std::fs::set_permissions(&filename, std::fs::Permissions::from_mode(fileoutput.mode))?;
                        Ok::<(), anyhow::Error>(())
                    };
                    all_files_futures.push(download_file_fut);
                }
            }
        }
        // Limit concurrency to max configured download threads.
        futures::stream::iter(all_files_futures.into_iter())
            .buffer_unordered(self.config.concurrent_download_max)
            .try_collect()
            .await?;
        Ok(())
    }

    /// Upload output files into S3, keyed by their hash (content addressed).
    async fn upload_files(&self, outputs: &OutputHashBundle) -> Result<()> {
        let mut all_files_futures = Vec::new();
        for (item, item_hash) in &outputs.hash_details {
            if let Output::File(ref fileoutput) = item {
                if fileoutput.present {
                    let object_name = fileoutput.filename.to_string();
                    let file_name = fileoutput.filename.to_path(&self.config.workspace_root)?;
                    let tokio_file = tokio::fs::File::open(&file_name).await?;
                    let content_length = tokio_file.metadata().await?.len();
                    all_files_futures.push(self.caching_backend.upload_object_file(
                        object_name,
                        item_hash,
                        Box::pin(tokio_file),
                        content_length,
                    ));
                }
            }
        }
        // Limit concurrency to max configured upload threads.
        futures::stream::iter(all_files_futures.into_iter())
            .buffer_unordered(self.config.concurrent_upload_max)
            .try_collect()
            .await?;
        Ok(())
    }

    const DEFAULT_EXIT_CODE: i32 = 1; // A catchall error code with no special meaning.

    pub async fn run_capsule(&self, program_run: &mut AtomicBool) -> Result<i32> {
        let inputs = self.read_inputs()?;

        // If we only need to output the hash, just do it and quit.
        if self.config.inputs_hash_output {
            print!("{}", inputs.hash);
            return Ok(0);
        }

        info!("Capsule inputs hash: {}", inputs.hash);

        // In passive mode, skip everything, except reading inputs as we still want to fill
        // CAPSULE_INPUTS_HASH with data about the capsule inputs.
        if self.config.passive {
            return self
                .execute_command(&inputs, program_run)
                .await
                .with_context(|| "Waiting for child")
                .map(|exit_status| exit_status.code().unwrap_or(Self::DEFAULT_EXIT_CODE));
        }

        let lookup_result = time::timeout(
            Duration::from_millis(timeouts::TIMEOUT_LOOKUP_MILLIS),
            self.caching_backend.lookup(&inputs),
        )
        .await
        .context("Timeout looking up in cache")? // Outer Result wrapping is from Timeout.
        .context("Looking in cache")?; // Inner Result wrapping is from the lookup itself.
        if let Some(ref lookup_result) = lookup_result {
            let log_cache_hit = |msg: &str| {
                info!(
                    "Cache hit on {} from {} ({}): {}",
                    self.capsule_id(),
                    lookup_result.source,
                    lookup_result.inputs.hash,
                    msg
                )
            };
            // We have a cache hit, but in case we are in placebo mode, or we have cached a failure,
            // we should still not use the cache. Let's figure this out while printing the solution.
            let mut use_cache = true;
            if self.config.milestone == Milestone::Placebo {
                log_cache_hit("ignoring and proceeding with execution");
                use_cache = false
            } else {
                if !self.config.cache_failure {
                    // If result code from the command is not 0
                    if lookup_result.outputs.result_code().unwrap_or(1) != 0 {
                        log_cache_hit("cached failure, proceeding with execution");
                        use_cache = false;
                    }
                }
                // Check whether we should avoid caching when output files from the cache hit
                // don't match with the capsule output files from config.
                if use_cache {
                    // a predicate selecting all paths for Output::Files from all cached outputs.
                    fn predicate<X>((output, _): &(Output, X)) -> Option<&WorkspacePath> {
                        if let Output::File(fileoutput) = output {
                            if fileoutput.present {
                                return Some(&fileoutput.filename);
                            }
                        }
                        None
                    }
                    let iter = lookup_result.outputs.hash_details.iter().filter_map(predicate);
                    // If anything doesn't match, don't use the cache!
                    if !self.config.outputs_match(iter)? {
                        log_cache_hit("mismatch in output patterns, proceeding with execution");
                        use_cache = false;
                    }
                }
            }

            if use_cache {
                if let Ok(result) = time::timeout(
                    Duration::from_millis(timeouts::TIMEOUT_DOWNLOAD_MILLIS),
                    self.download_files(&lookup_result.outputs),
                )
                .await
                {
                    match result {
                        Ok(_) => {
                            log_cache_hit("success");
                            // Log successful cached results.
                            self.logger
                                .log(&inputs, &lookup_result.outputs, true, false)
                                .await
                                .unwrap_or_else(|err| {
                                    error!("Failed to log results for observability: {}", err);
                                });
                            return Ok(lookup_result.outputs.result_code().unwrap_or(Self::DEFAULT_EXIT_CODE));
                        }
                        Err(e) => {
                            log_cache_hit(&format!("failed to retrieve from the cache: {}", e));
                        }
                    }
                } else {
                    error!("Time out downloading files");
                }
            }
        }

        // If we got here, we should execute.
        self.execute_and_cache(&inputs, &lookup_result, program_run)
            .await
            .map(|exit_status| exit_status.code().unwrap_or(Self::DEFAULT_EXIT_CODE))
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
        let config = Config::new(["capsule", "-c", "wtf", "--", "/bin/echo"].iter(), None).unwrap();
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
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        assert!(capsule.read_inputs().is_err());
    }

    #[test]
    #[serial]
    fn test_ok_glob() {
        let backend = dummy::DummyBackend::default();
        let config = Config::new(
            ["capsule", "-c", "wtf", "-i", "/bin/echo", "--", "/bin/echo"].iter(),
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
    async fn test_cache_hit_job_id() {
        let tmp_dir = TempDir::new().unwrap();
        let backend = TestBackend::new("wtf", TestBackendConfig::default());
        let out_file_1 = tmp_dir.path().join("xxyy");
        let config = Config::new(
            [
                "capsule",
                "-c",
                "wtf",
                "-j",
                "https://wtfjob.org",
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
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        assert!(program_run.load(Ordering::SeqCst));

        let inputs = capsule.read_inputs().unwrap();
        let lookup_result = backend.lookup(&inputs).await.unwrap();
        assert_eq!(lookup_result.unwrap().source, "https://wtfjob.org");
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
    // Here the logic changed. OutputBundles contain the information about whether an output file
    // was present when the cache entry was created.  Before 0.2.9, once we had cache hit with the
    // output file absent, capsule would try to make sure that the file is removed. In reality an
    // absent file is usually a miconfiguration. Fixing that misconfiguration should then fix the
    // job and caching, but with the old logic it would just get cache hits with no output.
    // So this test actuall checks that the file is *not* removed on cache hit with a file 'not present',
    // and that the cache hit is ignored.
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
        assert!(program_run.load(Ordering::SeqCst));

        // Because the out file was not present when the run was cached, we should expect it
        // to be removed.
        assert!(out_file.exists());
    }

    #[tokio::test]
    #[serial]
    async fn test_lookup_timeout() {
        let tmp_dir = TempDir::new().unwrap();
        let backend = TestBackend::new(
            "wtf",
            TestBackendConfig {
                lookup_timeout: true,
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
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        capsule.run_capsule(&mut program_run).await.unwrap_err();
    }

    #[tokio::test]
    #[serial]
    async fn test_download_timeout() {
        let tmp_dir = TempDir::new().unwrap();
        let backend = TestBackend::new(
            "wtf",
            TestBackendConfig {
                download_timeout: true,
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
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        assert!(program_run.load(Ordering::SeqCst));

        std::fs::remove_file(&out_file_1).unwrap();

        // Running 2nd time, expect a cache hit, but a download problem.
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        // Returns ok, despite the download problem, as it would just execute the program
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);

        // The 2nd time the program should be run, because of timeout downloading.
        assert!(program_run.load(Ordering::SeqCst));
    }

    #[tokio::test]
    #[serial]
    async fn test_cache_write_timeout() {
        let tmp_dir = TempDir::new().unwrap();
        let backend = TestBackend::new(
            "wtf",
            TestBackendConfig {
                write_timeout: true,
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
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        // Despite the write errors, the capsule successfully executed the program,
        // so the return code is zero.
        assert_eq!(code, 0);
        assert!(program_run.load(Ordering::SeqCst));

        // 2nd capsule, should NOT be cached, as the capsule call above failed to write to the
        // cache, despite successful completion of the underlying program.
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);
        // The program must have been run.
        assert!(program_run.load(Ordering::SeqCst));
        assert!(out_file_1.is_file());
    }

    #[tokio::test]
    #[serial]
    async fn test_cache_upload_timeout() {
        let tmp_dir = TempDir::new().unwrap();
        let backend = TestBackend::new(
            "wtf",
            TestBackendConfig {
                upload_timeout: true,
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
        )
        .unwrap();
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        // Despite the upload errors, the capsule successfully executed the program,
        // so the return code is zero.
        assert_eq!(code, 0);
        assert!(program_run.load(Ordering::SeqCst));

        // 2nd capsule, should NOT be cached, as the capsule call above failed to upload to the
        // cache, despite successful completion of the underlying program.
        let capsule = Capsule::new(&config, &backend, &Dummy);
        let mut program_run = AtomicBool::new(false);
        let code = capsule.run_capsule(&mut program_run).await.unwrap();
        assert_eq!(code, 0);

        // The program must have been run.
        assert!(program_run.load(Ordering::SeqCst));
        assert!(out_file_1.is_file());
    }
}
