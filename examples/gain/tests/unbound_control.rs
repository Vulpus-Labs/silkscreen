//! A control in the SVG with no binding must fail to compile, naming the
//! control. Builds a copy of this example with `knob.pan` added to the SVG.
//!
//! The first run compiles the dependency tree into
//! `target/compile-fail` (shared across runs), so it is slow once.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Not canonicalized: on Windows that gives a `\\?\` path, which the
/// manifest below would have to escape.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

#[test]
fn unbound_svg_control_is_a_compile_error() {
    let root = workspace_root();
    let example = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("target/compile-fail/gain-unbound");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();

    let svg = std::fs::read_to_string(example.join("gain.svg")).unwrap();
    let marker = r##"<circle id="knob.gain" cx="150" cy="110" r="50" fill="#ff00ff"/>"##;
    assert!(svg.contains(marker), "gain.svg changed; update this test");
    let svg = svg.replace(
        marker,
        &format!(r##"{marker}<circle id="knob.pan" cx="250" cy="150" r="20" fill="#ff00ff"/>"##),
    );
    std::fs::write(dir.join("gain.svg"), svg).unwrap();
    for f in ["build.rs", "src/lib.rs"] {
        std::fs::copy(example.join(f), dir.join(f)).unwrap();
    }
    for f in ["rust-toolchain.toml", "Cargo.lock"] {
        std::fs::copy(root.join(f), dir.join(f)).unwrap();
    }
    let crates = root.join("crates");
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "gain-unbound"
version = "0.0.0"
edition = "2024"

[workspace]

# Literal strings: Windows paths' backslashes are not escapes.
[dependencies]
silkscreen = {{ path = '{}' }}

[build-dependencies]
silkscreen-build = {{ path = '{}' }}
"#,
            crates.join("silkscreen").display(),
            crates.join("silkscreen-build").display()
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO"))
        .args(["check", "--lib", "--quiet", "--message-format=short"])
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", root.join("target/compile-fail/target"))
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .output()
        .expect("run cargo");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "expected a compile error:\n{stderr}"
    );
    assert!(
        stderr.contains("missing field `pan`"),
        "expected the error to name `pan`:\n{stderr}"
    );
}
