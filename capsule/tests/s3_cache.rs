mod common;

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
    let input = setup_data.path("input.txt");

    let side_effect = setup_data.path("side_effect.txt");
    let command = format!("echo 'hello!' > {}", side_effect.to_str().unwrap());
    // Run it first time.
    common::capsule(
        setup_data.port,
        &[
            "-c",
            "wtf",
            "-t",
            "foo",
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

    // Inbetween, clean the cache.
    common::remove_bucket(setup_data.port, "capsule-test");

    // Run it second time.
    let side_effect = setup_data.path("side_effect_2.txt");
    let command = format!("echo 'wtf' > {}", side_effect.to_str().unwrap());
    common::capsule(
        setup_data.port,
        &[
            "-c",
            "wtf",
            "-t",
            "foo",
            "-i",
            input.to_str().unwrap(),
            "--",
            "/bin/bash",
            "-c",
            &command,
        ],
    );
    println!("Checking file {:?}", side_effect);
    // Verify that the second time the side effect is present
    assert!(side_effect.exists());
}
