//! `cargo silkscreen deploy-web <package> [--site <checkout>]`: publish a
//! web package's page into a static site's git checkout, for a host that
//! deploys on push and reads a root `_headers` file (Netlify, Cloudflare
//! Pages):
//!
//! 1. build the page (`cargo silkscreen web <package>`), release;
//! 2. mirror it into `static/<subpath>/`, leaving out its own `_headers`
//!    (such hosts read `_headers` at the root only) and any node tests;
//! 3. append the subpath's cross-origin isolation block (COOP/COEP, which
//!    a `SharedArrayBuffer` needs) to the root `static/_headers` if it's
//!    missing. That file may hold one block per page, so it's never
//!    rewritten: overwriting it would take the others off the air;
//! 4. commit only those two paths, as the checkout may hold other work,
//!    and push.
//!
//! The page's place is `[package.metadata.silkscreen.web]`'s `site`
//! (`title`, `subpath`). The checkout is `--site` or `SITE`; `SUBPATH`
//! overrides the subpath, `BRANCH` the branch (default `main`);
//! `NO_BUILD=1` reuses `<target>/web/<package>`, and `NO_PUSH=1` commits
//! without pushing.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::metadata::Workspace;
use crate::run_command;
use crate::web::{build, web_package};

/// An environment variable, if set and not empty.
fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// The block `_headers` needs for `subpath`, starting with a blank line.
fn headers_block(title: &str, subpath: &str) -> String {
    format!(
        "\n# Same cross-origin isolation for the {title} web demo (silkscreen).\n\
         /{subpath}/*\n  \
         Cross-Origin-Opener-Policy: same-origin\n  \
         Cross-Origin-Embedder-Policy: require-corp\n  \
         Cross-Origin-Resource-Policy: same-origin\n"
    )
}

/// Whether `headers` already has a block for `subpath`.
fn covers(headers: &str, subpath: &str) -> bool {
    let rule = format!("/{subpath}/*");
    headers.lines().any(|l| l.trim() == rule)
}

pub fn run(ws: &Workspace, package: &str, flags: &[String]) -> Result<(), String> {
    let (p, web) = web_package(ws, package)?;
    let site = web.site.as_ref().ok_or_else(|| {
        format!(
            "{package} has no [package.metadata.silkscreen.web] `site`: add `site = {{ title = \"…\", subpath = \"…\" }}` to its Cargo.toml"
        )
    })?;
    let checkout = match flags {
        [] => env("SITE").map(PathBuf::from),
        [flag, dir] if flag == "--site" => Some(PathBuf::from(dir)),
        _ => return Err(crate::usage()),
    }
    .ok_or("deploy-web needs the site's git checkout: --site <dir>, or SITE")?;
    let subpath = env("SUBPATH").unwrap_or_else(|| site.subpath.clone());
    let subpath = subpath.trim_matches('/');
    let branch = env("BRANCH").unwrap_or_else(|| "main".into());
    if !checkout.join(".git").exists() {
        return Err(format!(
            "{} is not a git checkout (--site or SITE)",
            checkout.display()
        ));
    }

    // 1. The bundle.
    let bundle = if env("NO_BUILD").is_some() {
        ws.target_dir.join("web").join(&p.name)
    } else {
        println!("==> building web bundle: cargo silkscreen web {}", p.name);
        build(ws, p, web, false)?
    };
    if !bundle.join("index.html").is_file() {
        return Err(format!(
            "no bundle at {} (run without NO_BUILD)",
            bundle.display()
        ));
    }

    // 2. A clean mirror in the site's static tree.
    let dest = checkout.join("static").join(subpath);
    println!("==> copying bundle → static/{subpath}/");
    std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    run_command(
        Command::new("rsync")
            .args([
                "-a",
                "--delete",
                "--exclude",
                "_headers",
                "--exclude",
                "*.test.mjs",
            ])
            .arg(format!("{}/", bundle.display()))
            .arg(format!("{}/", dest.display())),
    )?;

    // 3. The root _headers: appended to, never rewritten.
    let headers = checkout.join("static/_headers");
    let current = std::fs::read_to_string(&headers).unwrap_or_default();
    if covers(&current, subpath) {
        println!("==> static/_headers already covers /{subpath}/");
    } else {
        println!("==> appending static/_headers block for /{subpath}/");
        append(&headers, &headers_block(&site.title, subpath))?;
    }

    // 4. Commit our paths only, then push.
    let git = |args: &[&str]| -> Command {
        let mut c = Command::new("git");
        c.current_dir(&checkout).args(args);
        c
    };
    let static_path = format!("static/{subpath}");
    run_command(&mut git(&["add", &static_path, "static/_headers"]))?;
    let staged = git(&["diff", "--cached", "--quiet"])
        .status()
        .map_err(|e| format!("running git: {e}"))?;
    if staged.success() {
        println!("==> no changes to commit (site already up to date)");
        return Ok(());
    }
    let version = Command::new("git")
        .current_dir(&ws.root)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map_err(|e| format!("running git: {e}"))?;
    let version = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    run_command(&mut git(&[
        "commit",
        "-m",
        &format!(
            "deploy: {} web demo → /{subpath}/ (silkscreen @ {version})",
            site.title
        ),
        "-m",
        &format!(
            "Built with cargo silkscreen web {}; _headers sets COOP/COEP for SharedArrayBuffer.",
            p.name
        ),
    ]))?;
    if env("NO_PUSH").is_some() {
        println!("==> committed (NO_PUSH set: not pushing). Push manually to deploy.");
        return Ok(());
    }
    println!("==> pushing to {branch} (the host deploys on push)");
    run_command(&mut git(&["push", "origin", &branch]))?;
    println!("==> done. Verify once live:");
    println!("    curl -sI https://<site>/{subpath}/ | grep -i cross-origin");
    Ok(())
}

fn append(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_is_what_the_scripts_appended() {
        assert_eq!(
            headers_block("My Synth", "products/my-synth/web"),
            "\n# Same cross-origin isolation for the My Synth web demo (silkscreen).\n\
             /products/my-synth/web/*\n  \
             Cross-Origin-Opener-Policy: same-origin\n  \
             Cross-Origin-Embedder-Policy: require-corp\n  \
             Cross-Origin-Resource-Policy: same-origin\n"
        );
    }

    #[test]
    fn a_block_covers_its_own_subpath_only() {
        let headers = headers_block("My Synth", "products/my-synth/web");
        assert!(covers(&headers, "products/my-synth/web"));
        // A prefix or a longer path isn't the same rule.
        assert!(!covers(&headers, "products/my-synth"));
        assert!(!covers(&headers, "products/my-synth/web/x"));
        assert!(!covers("", "products/my-synth/web"));
    }
}
