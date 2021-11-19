use assert_cmd;
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
    thread::sleep(time::Duration::from_millis(1000));
    SetupData {
        minio,
        directory: Some(directory),
    }
}

pub fn capsule(args: &[&str]) {
    let output = assert_cmd::Command::cargo_bin("capsule")
        .expect("Couldn't find capsule target")
        .env("CAPSULE_ARGS",
             format!("--s3_bucket=capsule-test --s3_endpoint=http://127.0.0.1:{}", MINIO_PORT))
        .args(args)
        .output()
        .expect("Couldn't execute capsule");
    io::stdout().write_all(&output.stdout).unwrap();
    io::stderr().write_all(&output.stderr).unwrap();
    assert!(output.status.success());
}
