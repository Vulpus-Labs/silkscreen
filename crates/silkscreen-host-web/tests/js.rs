//! The JS modules' node tests (`js/*.test.mjs`), run by `cargo test` when
//! node is installed, and skipped with a note when it isn't.
#![cfg(not(target_arch = "wasm32"))]

use std::process::Command;

#[test]
fn node_tests_pass() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("node isn't installed: skipping the JS tests");
        return;
    }
    let output = Command::new("node")
        .args(["--test", "js/*.test.mjs"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "node --test failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
