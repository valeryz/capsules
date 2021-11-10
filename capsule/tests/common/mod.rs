use assert_cmd;
use std::process;
use std::{thread, time};

use tempfile::{self, TempDir};

pub const MINIO_PORT: u16 = 54444;

pub struct SetupData {
    minio: process::Child,
    directory: Option<TempDir>,
}

impl Drop for SetupData {
    fn drop(&mut self) {
        self.minio.kill().expect("Failed to stop Minio");
        self.directory.take().map(|d| d.close());
    }
}

pub fn setup() -> SetupData {
    let directory = tempfile::tempdir().expect("Failed to create temp dir");
    let minio = process::Command::new("minio")
        .args([
            "server",
            directory.path().to_str().unwrap(),
            "--address",
            &format!("127.0.0.1:{}", MINIO_PORT),
            "--quiet",
        ])
        .spawn()
        .expect("Minio failed to start");
    // TODO: Wait until we can connect to the port, instead of sleeping.
    thread::sleep(time::Duration::from_millis(100));
    SetupData {
        minio,
        directory: Some(directory),
    }
}

pub fn capsule(args: &[&str]) {
    assert_cmd::Command::cargo_bin("capsule")
        .expect("Couldn't find capsule target")
        .args(args)
        .output()
        .expect("Couldn't execute capsule");
}
