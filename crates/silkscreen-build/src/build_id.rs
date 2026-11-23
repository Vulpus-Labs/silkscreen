//! A build's identity for an editor's version badge: the short git hash,
//! `+` if the tree is dirty, and the build time.

use std::path::PathBuf;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// From a build script: set `SILKSCREEN_BUILD_ID` ("1a2b3c4+ 14:05") for
/// `env!`, and re-run on commits and staging as well as on changes to
/// the crate's `src`. Outside a git checkout the hash is "nogit".
pub fn stamp_build_id() {
    let hash = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "nogit".into());
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let time = Command::new("date")
        .arg("+%H:%M")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    let id = format!("{hash}{} {time}", if dirty { "+" } else { "" });
    println!("cargo::rustc-env=SILKSCREEN_BUILD_ID={id}");
    // The checkout's own HEAD and index (a worktree has its own), and the
    // branches, which every worktree shares.
    let dir = |arg| git(&["rev-parse", arg]).map(PathBuf::from);
    if let Some(git_dir) = dir("--absolute-git-dir") {
        for file in ["HEAD", "index"] {
            println!("cargo::rerun-if-changed={}", git_dir.join(file).display());
        }
    }
    if let Some(common) = dir("--git-common-dir") {
        let common = std::fs::canonicalize(&common).unwrap_or(common);
        println!(
            "cargo::rerun-if-changed={}",
            common.join("refs/heads").display()
        );
    }
    println!("cargo::rerun-if-changed=src");
}
