use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;

mod common;

#[test]
fn test_error_code() {
    let setup_data = common::setup(); // RAII - clean up on destruction.
    let path = setup_data.path("output.txt");
    let command = format!("echo 'wtf' > {}; exit 111", path.to_str().unwrap());
    let error_code = common::capsule(
        setup_data.port,
        &["-c", "wtf", "-b", "s3", "--", "/bin/bash", "-c", &command],
    );
    println!("Checking file {:?}", path);
    assert!(path.exists());
    assert_eq!(error_code, 111);
}

#[test]
fn test_s3_cache_miss() {
    let setup_data = common::setup(); // RAII - clean up on destruction.
    let path = setup_data.path("output.txt");
    let command = format!("echo 'wtf' > {}", path.to_str().unwrap());
    common::capsule(
        setup_data.port,
        &["-c", "wtf", "-b", "s3", "--", "/bin/bash", "-c", &command],
    );
    println!("Checking file {:?}", path);
    assert!(path.exists());
}

#[test]
fn test_s3_cache_hit() {
    let setup_data = common::setup(); // RAII - clean up on destruction.
    let input = setup_data.path("input.txt");
    std::fs::write(&input, "input data").unwrap();

    let side_effect = setup_data.path("side_effect.txt");
    let command = format!("echo 'hello!' > {}", side_effect.to_str().unwrap());
    // Run it first time.
    common::capsule(
        setup_data.port,
        &[
            "-c",
            "wtf",
            "-b",
            "s3",
            "-i",
            input.to_str().unwrap(),
            "--",
            "/bin/bash",
            "-c",
            &command,
        ],
    );
    // Creating
    println!("Checking file {:?}", side_effect);
    assert!(side_effect.exists());
    std::fs::remove_file(side_effect).unwrap();

    // Run it second time.
    let side_effect = setup_data.path("side_effect_2.txt");
    let command = format!("echo 'wtf' > {}", side_effect.to_str().unwrap());
    common::capsule(
        setup_data.port,
        &[
            "-c",
            "wtf",
            "-b",
            "s3",
            "-i",
            input.to_str().unwrap(),
            "--",
            "/bin/bash",
            "-c",
            &command,
        ],
    );
    println!("Checking file {:?}", side_effect);
    // Verify that the second time the side effect is absent.
    assert!(!side_effect.exists());
}

#[test]
fn test_cache_expiration() {
    let setup_data = common::setup(); // RAII - clean up on destruction.

    let side_effect = setup_data.path("side_effect.txt");
    let command = format!("echo 'hello!' > {}", side_effect.to_str().unwrap());
    // Run it first time.
    common::capsule(
        setup_data.port,
        &["-c", "wtf", "-t", "foo", "--", "/bin/bash", "-c", &command],
    );
    // Creating
    println!("Checking file {:?}", side_effect);
    assert!(side_effect.exists());

    // Inbetween, clean the cache.
    common::remove_bucket(setup_data.port, "capsule-test");

    // Run it second time.
    let side_effect = setup_data.path("side_effect_2.txt");
    let command = format!("echo 'wtf' > {}", side_effect.to_str().unwrap());
    common::capsule(
        setup_data.port,
        &["-c", "wtf", "-t", "foo", "--", "/bin/bash", "-c", &command],
    );
    println!("Checking file {:?}", side_effect);
    // Verify that the second time the side effect is present
    assert!(side_effect.exists());
}

#[test]
fn test_inputs_hash() {
    let setup_data = common::setup(); // RAII - clean up on destruction.
    let input = setup_data.path("input.txt");
    std::fs::write(&input, "input data").unwrap();
    let output = assert_cmd::Command::cargo_bin("capsule")
        .expect("Couldn't find capsule target")
        .args(["--inputs_hash", "-t", "foo", "-i", input.to_str().unwrap()])
        .output()
        .expect("Couldn't execute capsule");

    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"6683fee73b2d88cd8414b00fdc6ea103e6e3d47f23dd8d67379b5be41fc72273"
    );
}

fn file_hash(filename: &std::path::PathBuf) -> Result<String> {
    const BUFSIZE: usize = 4096;
    let mut acc = Sha256::new();
    let mut f =
        fs::File::open(filename).with_context(|| format!("Reading input file '{}'", filename.to_string_lossy()))?;
    let mut buf: [u8; BUFSIZE] = [0; BUFSIZE];
    loop {
        let rd = f.read(&mut buf)?;
        if rd == 0 {
            break;
        }
        acc.update(&buf[..rd]);
    }
    Ok(format!("{:x}", acc.finalize()))
}

#[test]
fn test_cas_optimization() {
    let setup_data = common::setup(); // RAII - clean up on destruction.
    let output = setup_data.path("output.txt");

    // Make sure the bucket exists before we start the test
    common::put_object(setup_data.port, "capsule-objects", "foo", b"bar");

    let command = format!("echo 'foo' > {}", output.to_str().unwrap());
    // Run it for the first time.
    common::capsule(
        setup_data.port,
        &[
            "-c",
            "wtf",
            "-b",
            "s3",
            "-t",
            "xxx",
            "-o",
            output.to_str().unwrap(),
            "--",
            "/bin/bash",
            "-c",
            &command,
        ],
    );

    let hash = file_hash(&output).unwrap();
    let key = format!("{}/{}", &hash[0..2], hash);

    // Overwrite the blob in the bucket with something else.
    common::put_object(setup_data.port, "capsule-objects", &key, b"bar");

    // Run exactly the same the 2nd time, but with a different capsule ID, so we don't get a cache hit,
    // but still produce the same blob.
    common::capsule(
        setup_data.port,
        &[
            "-c",
            "wtf_2",
            "-b",
            "s3",
            "-t",
            "yyy",
            "-o",
            output.to_str().unwrap(),
            "--",
            "/bin/bash",
            "-c",
            &command,
        ],
    );

    let _ = fs::remove_file(&output);

    assert_eq!(
        b"bar".to_vec(),
        common::get_object(setup_data.port, "capsule-objects", &key).unwrap()
    );
}
