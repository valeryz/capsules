mod common;

#[test]
fn test1() {
    let _setup_data = common::setup();

    // Test goes here.
    common::capsule(&["capsule", "-c", "wtf", "--", "/bin/echo", "1", "2", "3"]);
    
}
