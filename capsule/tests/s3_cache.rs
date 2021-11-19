mod common;

#[test]
#[ignore]
fn test_s3_cache_miss() {
    let setup_data = common::setup(); // RAII - clean up on destruction.
    let path = setup_data.path("output.txt");
    let command = format!("echo 'wtf' > {}", path.to_str().unwrap());
    common::capsule(&["-c", "wtf", "-b", "s3", "--", "/bin/bash", "-c", &command]);
    println!("Checking file {:?}", path);
    assert!(path.exists());
}

#[test]
#[ignore]
fn test_s3_cache_hit() {
    let setup_data = common::setup(); // RAII - clean up on destruction.
    let input = setup_data.path("input.txt");

    let side_effect = setup_data.path("side_effect.txt");
    let command = format!("echo 'hello!' > {}", side_effect.to_str().unwrap());
    // Run it first time.
    common::capsule(&[
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
    ]);
    // Creating
    println!("Checking file {:?}", side_effect);
    assert!(side_effect.exists());

    // Run it second time.
    let side_effect = setup_data.path("side_effect_2.txt");
    let command = format!("echo 'wtf' > {}", side_effect.to_str().unwrap());
    common::capsule(&[
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
    ]);
    println!("Checking file {:?}", side_effect);
    // Verify that the second time the side effect is absent.
    assert!(!side_effect.exists());
}

#[test]
#[ignore]
fn test_cache_expiration() {
    let _setup_data = common::setup(); // RAII - clean up on destruction.

    let setup_data = common::setup(); // RAII - clean up on destruction.
    let input = setup_data.path("input.txt");

    let side_effect = setup_data.path("side_effect.txt");
    let command = format!("echo 'hello!' > {}", side_effect.to_str().unwrap());
    // Run it first time.
    common::capsule(&[
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
    ]);
    // Creating
    println!("Checking file {:?}", side_effect);
    assert!(side_effect.exists());

    // TODO: Inbetween, clean the cache.
    assert!(false);

    // Run it second time.
    let side_effect = setup_data.path("side_effect_2.txt");
    let command = format!("echo 'wtf' > {}", side_effect.to_str().unwrap());
    common::capsule(&[
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
    ]);
    println!("Checking file {:?}", side_effect);
    // Verify that the second time the side effect is absent.
    assert!(side_effect.exists());
}
