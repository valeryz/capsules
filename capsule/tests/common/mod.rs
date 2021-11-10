use std::process::{Child, Command};
use std::{thread, time};

use tempfile::{self, TempDir};

pub const MINIO_PORT: u16 = 54444;

pub struct SetupData {
    minio: Child,
    directory: TempDir,
}

pub fn setup() -> SetupData {
    let directory = tempfile::tempdir().expect("Failed to create temp dir");
    let minio = Command::new("minio")
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
    SetupData { minio, directory }
}

pub fn teardown(setup_data: SetupData) {
    let mut child = setup_data.minio;
    child.kill().expect("Failed to stop Minio");
    setup_data.directory.close().unwrap();
}
