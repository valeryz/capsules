use assert_cmd;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process;
use std::{thread, time};

use tempfile::{self, TempDir};

pub const MINIO_PORT: u16 = 54444;

pub struct SetupData {
    minio: process::Child,
    pub directory: Option<TempDir>,
}

impl Drop for SetupData {
    fn drop(&mut self) {
        self.minio.kill().expect("Failed to stop Minio");
        self.minio.wait().expect("Error waiting for minio to stop");
        // thread::sleep(time::Duration::from_millis(2_000));
        self.directory.take().map(|d| d.close());
    }
}

impl SetupData {
    pub fn path(&self, elem: &str) -> PathBuf {
        self.directory.as_ref().unwrap().path().join(elem)
    }
}

pub fn setup() -> SetupData {
    let directory = tempfile::tempdir().expect("Failed to create temp dir");
    fs::create_dir_all(directory.path().join("minio").join("capsule-test")).unwrap();
    let minio = process::Command::new("minio")
        .args([
            "server",
            directory.path().join("minio").to_str().unwrap(),
            "--address",
            &format!("127.0.0.1:{}", MINIO_PORT),
            "--quiet",
        ])
        .spawn()
        .expect("Minio failed to start");
    // TODO: Wait until we can connect to the port, instead of sleeping.
    thread::sleep(time::Duration::from_millis(1_000));
    SetupData {
        minio,
        directory: Some(directory),
    }
}

pub fn capsule(args: &[&str]) {
    let output = assert_cmd::Command::cargo_bin("capsule")
        .expect("Couldn't find capsule target")
        .env("AWS_ACCESS_KEY_ID", "minioadmin")
        .env("AWS_SECRET_ACCESS_KEY", "minioadmin")
        .env("CAPSULE_ARGS",
             format!("--s3_bucket=capsule-test --s3_region=eu-central-1 --s3_endpoint=http://127.0.0.1:{}", MINIO_PORT))
        .args(args)
        .output()
        .expect("Couldn't execute capsule");
    io::stdout().write_all(&output.stdout).unwrap();
    io::stderr().write_all(&output.stderr).unwrap();
    assert!(output.status.success());
}
