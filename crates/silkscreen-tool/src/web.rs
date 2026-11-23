//! `cargo silkscreen web <package> [--debug] [--serve] [--port <n>]`
//!
//! Builds a web package's two wasm modules into `<target>/web/<package>/`:
//!
//! - the editor module (wasm-bindgen), post-processed by the `wasm-bindgen`
//!   CLI into `pkg/`;
//! - the engine module (raw C ABI), `[package.metadata.silkscreen.web]`'s
//!   `engine`, copied as `<engine_lib>.wasm`;
//!
//! with `silkscreen-host-web`'s JS (the worklet and `startAudio`; not its
//! node tests) and the package's page. `--serve` serves the result with
//! the COOP and COEP headers a `SharedArrayBuffer` needs; a `_headers` file
//! carries the same headers for static hosts that read one.

use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::files::{copy, copy_tree, remove, write};
use crate::metadata::{Package, Section, Web, Workspace};
use crate::{run_command, usage};

const TARGET: &str = "wasm32-unknown-unknown";

/// The cross-origin isolation a `SharedArrayBuffer` needs.
const ISOLATION_HEADERS: [(&str, &str); 2] = [
    ("Cross-Origin-Opener-Policy", "same-origin"),
    ("Cross-Origin-Embedder-Policy", "require-corp"),
];

pub fn run(ws: &Workspace, package: &str, flags: &[String]) -> Result<(), String> {
    let (p, web) = web_package(ws, package)?;
    let mut debug = false;
    let mut serve_on = None;
    let mut flags = flags.iter();
    while let Some(f) = flags.next() {
        match f.as_str() {
            "--debug" => debug = true,
            "--serve" => serve_on = Some(serve_on.unwrap_or(8080)),
            "--port" => {
                let port = flags
                    .next()
                    .and_then(|p| p.parse::<u16>().ok())
                    .ok_or("--port takes a port number")?;
                serve_on = Some(port);
            }
            _ => return Err(usage()),
        }
    }
    let dist = build(ws, p, web, debug)?;
    println!("built {}", dist.display());
    if let Some(port) = serve_on {
        serve(&dist, port)?;
    }
    Ok(())
}

/// The web package `name` and its section.
pub fn web_package<'a>(ws: &'a Workspace, name: &str) -> Result<(&'a Package, &'a Web), String> {
    let p = ws.package(name, Section::Web)?;
    let web = p.web.as_ref().ok_or_else(|| Section::Web.missing(name))?;
    Ok((p, web))
}

/// Build `p`'s page into `<target>/web/<package>/`; returns that folder.
pub fn build(ws: &Workspace, p: &Package, web: &Web, debug: bool) -> Result<PathBuf, String> {
    check_wasm_bindgen(&ws.root)?;
    let engine = ws.package(&web.engine, Section::Web).map_err(|_| {
        format!(
            "{}: [package.metadata.silkscreen.web] names engine {:?}, which isn't a member of this workspace",
            p.name, web.engine
        )
    })?;
    let lib = |q: &Package| {
        q.lib.clone().ok_or_else(|| {
            format!(
                "{} has no library target: add a [lib] with crate-type = [\"cdylib\"]",
                q.name
            )
        })
    };
    let (ui_lib, engine_lib) = (lib(p)?, lib(engine)?);

    let mut cargo = crate::cargo();
    cargo.current_dir(&ws.root).args([
        "build",
        "--message-format=json-render-diagnostics",
        "--target",
        TARGET,
        "-p",
        &p.name,
        "-p",
        &engine.name,
    ]);
    if !debug {
        cargo.arg("--release");
    }
    let messages = cargo
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|err| format!("running cargo: {err}"))?;
    if !messages.status.success() {
        return Err(format!("cargo build failed: {}", messages.status));
    }
    let out_dirs = build_script_out_dirs(&String::from_utf8_lossy(&messages.stdout));
    let profile = if debug { "debug" } else { "release" };
    let out = ws.target_dir.join(TARGET).join(profile);

    let dist = ws.target_dir.join("web").join(&p.name);
    remove(&dist)?;
    std::fs::create_dir_all(&dist).map_err(|err| err.to_string())?;

    run_command(
        Command::new("wasm-bindgen")
            .args(["--target", "web", "--out-dir"])
            .arg(dist.join("pkg"))
            .arg(out.join(format!("{ui_lib}.wasm"))),
    )?;
    copy(
        &out.join(format!("{engine_lib}.wasm")),
        &dist.join(format!("{engine_lib}.wasm")),
    )?;
    let mut assets = 0;
    for out_dir in &out_dirs {
        assets += copy_assets(out_dir, &dist)?;
    }
    println!("{assets} fetched PNGs");
    let host_js = host_js_dir(&String::from_utf8_lossy(&messages.stdout))
        .ok_or("cargo built no silkscreen-host-web: is the page's crate a web host?")?;
    copy_tree(&host_js, &dist, &|f| !is_node_test(f))?;
    copy_tree(&web.page, &dist, &|_| true)?;
    let mut headers = String::from("/*\n");
    for (k, v) in ISOLATION_HEADERS {
        let _ = writeln!(headers, "  {k}: {v}");
    }
    write(&dist.join("_headers"), &headers)?;
    Ok(dist)
}

/// Whether `file` is one of the host JS's node tests, which pages don't
/// serve.
fn is_node_test(file: &Path) -> bool {
    file.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".test.mjs"))
}

/// `silkscreen-host-web`'s `js/` folder, wherever cargo took the crate
/// from (this workspace, git or a registry), from its build messages.
fn host_js_dir(messages: &str) -> Option<PathBuf> {
    messages
        .lines()
        .filter(|l| l.contains(r#""reason":"compiler-artifact""#))
        .find(|l| json_string(l, "name").as_deref() == Some("silkscreen_host_web"))
        .and_then(|l| json_string(l, "manifest_path"))
        .and_then(|m| Some(Path::new(&m).parent()?.join("js")))
}

/// Each build script's `OUT_DIR`, from cargo's JSON messages.
fn build_script_out_dirs(messages: &str) -> Vec<PathBuf> {
    messages
        .lines()
        .filter(|l| l.contains(r#""reason":"build-script-executed""#))
        .filter_map(|l| json_string(l, "out_dir"))
        .map(PathBuf::from)
        .collect()
}

/// The string value of `key` in a line of JSON, unescaped.
fn json_string(line: &str, key: &str) -> Option<String> {
    let start = line.find(&format!(r#""{key}":""#))? + key.len() + 4;
    let mut value = String::new();
    let mut chars = line[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(value),
            '\\' => value.push(chars.next()?),
            c => value.push(c),
        }
    }
    None
}

/// Copy every PNG a fetch-mode faceplate in `out_dir` lists in its
/// `*.assets` manifest to its URL under `dist`. Returns how many.
fn copy_assets(out_dir: &Path, dist: &Path) -> Result<usize, String> {
    let Ok(entries) = std::fs::read_dir(out_dir) else {
        return Ok(0);
    };
    let mut copied = 0;
    for entry in entries.flatten() {
        let manifest = entry.path();
        if manifest.extension().is_none_or(|x| x != "assets") {
            continue;
        }
        let text = std::fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
        for line in text.lines() {
            let (url, file) = line
                .split_once('\t')
                .ok_or_else(|| format!("{}: malformed line {line:?}", manifest.display()))?;
            let target = dist.join(url);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            copy(Path::new(file), &target)?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// The UI module's glue must come from the CLI matching the `wasm-bindgen`
/// crate in `Cargo.lock`.
fn check_wasm_bindgen(root: &Path) -> Result<(), String> {
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).map_err(|e| e.to_string())?;
    let pinned = locked_version(&lock, "wasm-bindgen").ok_or("Cargo.lock has no wasm-bindgen")?;
    let install = format!("cargo install wasm-bindgen-cli --version {pinned} --locked");
    let output = Command::new("wasm-bindgen")
        .arg("--version")
        .output()
        .map_err(|_| format!("the wasm-bindgen CLI isn't installed: {install}"))?;
    let have = String::from_utf8_lossy(&output.stdout);
    if have.split_whitespace().nth(1) != Some(pinned) {
        return Err(format!(
            "the wasm-bindgen CLI is {}, but the workspace uses wasm-bindgen {pinned}: {install}",
            have.trim()
        ));
    }
    Ok(())
}

/// The version of `package` in a `Cargo.lock`.
fn locked_version<'a>(lock: &'a str, package: &str) -> Option<&'a str> {
    let name = format!("name = \"{package}\"");
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line == name {
            return lines
                .next()?
                .strip_prefix("version = \"")?
                .strip_suffix('"');
        }
    }
    None
}

fn serve(dist: &Path, port: u16) -> Result<(), String> {
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    println!(
        "serving {} at http://localhost:{port}/ (Ctrl-C to stop)",
        dist.display()
    );
    for stream in listener.incoming().flatten() {
        let dist = dist.to_path_buf();
        std::thread::spawn(move || {
            let _ = respond(stream, &dist);
        });
    }
    Ok(())
}

fn respond(mut stream: TcpStream, dist: &Path) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    // Drain the headers; nothing in them matters here.
    let mut line = String::new();
    while reader.read_line(&mut line)? > 2 {
        line.clear();
    }
    let mut parts = request.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let (status, body, kind) = match (method, resolve(dist, target)) {
        ("GET" | "HEAD", Some(path)) => match std::fs::read(&path) {
            Ok(body) => ("200 OK", body, content_type(&path)),
            Err(_) => ("404 Not Found", b"not found".to_vec(), "text/plain"),
        },
        ("GET" | "HEAD", None) => ("404 Not Found", b"not found".to_vec(), "text/plain"),
        _ => (
            "405 Method Not Allowed",
            b"method not allowed".to_vec(),
            "text/plain",
        ),
    };
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in ISOLATION_HEADERS {
        let _ = write!(head, "{k}: {v}\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if method != "HEAD" {
        stream.write_all(&body)?;
    }
    println!("{status} {target}");
    Ok(())
}

/// The file under `dist` a request target names: `/` is `index.html`; the
/// query is ignored; anything climbing out of `dist` is refused.
fn resolve(dist: &Path, target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next()?.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let relative = Path::new(path);
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let full = dist.join(relative);
    full.is_file().then_some(full)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("png") => "image/png",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_versions_are_found_by_exact_name() {
        let lock = "[[package]]\nname = \"wasm-bindgen-macro\"\nversion = \"9.9.9\"\n\n[[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.129\"\n";
        assert_eq!(locked_version(lock, "wasm-bindgen"), Some("0.2.129"));
        assert_eq!(locked_version(lock, "js-sys"), None);
    }

    #[test]
    fn requests_resolve_inside_dist_only() {
        let dist = std::env::temp_dir().join(format!("silkscreen-web-{}", std::process::id()));
        std::fs::create_dir_all(dist.join("pkg")).unwrap();
        std::fs::write(dist.join("index.html"), "").unwrap();
        std::fs::write(dist.join("pkg/a.js"), "").unwrap();
        assert_eq!(resolve(&dist, "/"), Some(dist.join("index.html")));
        assert_eq!(resolve(&dist, "/pkg/a.js?v=1"), Some(dist.join("pkg/a.js")));
        assert_eq!(resolve(&dist, "/../Cargo.toml"), None);
        assert_eq!(resolve(&dist, "/pkg/../../x"), None);
        assert_eq!(resolve(&dist, "/missing.js"), None);
        std::fs::remove_dir_all(&dist).unwrap();
    }

    #[test]
    fn out_dirs_come_from_build_script_messages() {
        let messages = concat!(
            r#"{"reason":"compiler-artifact","package_id":"x"}"#,
            "\n",
            r#"{"reason":"build-script-executed","package_id":"gain","linked_libs":[],"out_dir":"/t/build/gain-1/out"}"#,
            "\n",
            r#"{"reason":"build-script-executed","out_dir":"C:\\t\\out"}"#,
        );
        assert_eq!(
            build_script_out_dirs(messages),
            [
                PathBuf::from("/t/build/gain-1/out"),
                PathBuf::from(r"C:\t\out")
            ]
        );
    }

    #[test]
    fn the_host_js_is_found_beside_the_crates_manifest() {
        let messages = concat!(
            r#"{"reason":"compiler-artifact","package_id":"a","manifest_path":"/x/gain/Cargo.toml","target":{"kind":["lib"],"name":"gain"}}"#,
            "\n",
            r#"{"reason":"compiler-artifact","package_id":"git+https://example.com/silkscreen#silkscreen-host-web@0.2.0","manifest_path":"/cargo/git/checkouts/silkscreen/abc/crates/silkscreen-host-web/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"silkscreen_host_web"}}"#,
        );
        assert_eq!(
            host_js_dir(messages),
            Some(PathBuf::from(
                "/cargo/git/checkouts/silkscreen/abc/crates/silkscreen-host-web/js"
            ))
        );
        assert_eq!(host_js_dir(""), None);
    }

    #[test]
    fn node_tests_stay_out_of_pages() {
        assert!(is_node_test(Path::new("js/silkscreen-page.test.mjs")));
        assert!(!is_node_test(Path::new("js/silkscreen-page.mjs")));
    }

    #[test]
    fn wasm_is_served_as_wasm() {
        assert_eq!(content_type(Path::new("a.wasm")), "application/wasm");
        assert_eq!(
            content_type(Path::new("w.js")),
            "text/javascript; charset=utf-8"
        );
    }
}
