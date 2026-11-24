//! `cargo silkscreen bundle <package> [--debug] [--universal | --target <triple>] [--vst3 [--validate]] [--sign] [--notarize] [--install]`
//!
//! Builds a plugin package's `cdylib` and wraps it as a `.clap` in
//! `<target>/bundled/`: on macOS a bundle directory
//! (`Contents/MacOS/<exe>`, `Info.plist`, `PkgInfo`); elsewhere the shared
//! library renamed. `--universal` (macOS) builds for ARM64 and x86-64 and
//! joins them with `lipo`. `--target` builds for another triple:
//! `arm64ec-pc-windows-msvc` for Windows on Arm hosts, such as REAPER's,
//! which load x64 and ARM64EC plugins but not ARM64 ones. `--sign` and
//! `--notarize` (macOS) sign the bundle with a Developer ID and notarize
//! it ([`crate::sign`]). `--vst3` also builds `<bundle>.vst3` beside it
//! from clap-wrapper ([`crate::vst3`]), signed and notarized with it;
//! `--validate` runs Steinberg's validator on it. `--install` copies the
//! bundles to the user's CLAP (and VST3) folder; the installed copies are
//! the signed ones.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::files::{copy, remove, replace, write};
use crate::metadata::{Clap, Package, Section, Workspace};
use crate::{sign, usage, vst3};

/// The two halves of a universal macOS bundle.
const MACOS_TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

pub fn run(ws: &Workspace, package: &str, flags: &[String]) -> Result<(), String> {
    let p = ws.package(package, Section::Clap)?;
    let clap = p
        .clap
        .as_ref()
        .ok_or_else(|| Section::Clap.missing(package))?;
    let mut debug = false;
    let mut install = false;
    let mut universal = false;
    let mut signed = false;
    let mut notarized = false;
    let mut vst3 = false;
    let mut validate = false;
    let mut target = None;
    let mut flags = flags.iter();
    while let Some(f) = flags.next() {
        match f.as_str() {
            "--debug" => debug = true,
            "--target" => target = Some(flags.next().ok_or_else(usage)?.as_str()),
            "--install" => install = true,
            "--universal" => universal = true,
            "--sign" => signed = true,
            "--notarize" => (signed, notarized) = (true, true),
            "--vst3" => vst3 = true,
            "--validate" => validate = true,
            _ => return Err(usage()),
        }
    }
    if validate && !vst3 {
        return Err("--validate checks the VST3; add --vst3".into());
    }
    if universal && target.is_some() {
        return Err("--universal picks its own targets; drop --target".into());
    }
    if universal && !cfg!(target_os = "macos") {
        return Err("--universal builds a macOS bundle; run it on macOS".into());
    }
    if signed && !cfg!(target_os = "macos") {
        return Err("--sign and --notarize sign a macOS bundle; run them on macOS".into());
    }
    // Credentials are checked before the build, so a missing one fails fast.
    let identity = signed.then(sign::identity).transpose()?;
    let notary = notarized.then(sign::notary_credentials).transpose()?;
    let path = bundle(ws, p, clap, debug, universal, target)?;
    println!("bundled {}", path.display());
    let mut bundles = vec![(path, install_dir as fn() -> Result<PathBuf, String>)];
    if vst3 {
        let targets: Vec<&str> = if universal {
            MACOS_TARGETS.to_vec()
        } else {
            target.into_iter().collect()
        };
        let path = vst3::bundle(ws, p, clap, debug, &targets)?;
        println!("bundled {}", path.display());
        if validate {
            vst3::validate(ws, p, &path)?;
            println!("validated {}", path.display());
        }
        bundles.push((path, vst3::install_dir));
    }
    for (path, folder) in &bundles {
        if let Some(identity) = &identity {
            sign::sign(path, identity)?;
            println!("signed {}", path.display());
        }
        if let Some(notary) = &notary {
            sign::notarize(path, notary)?;
            println!("notarized {}", path.display());
        }
        if install {
            let dest = folder()?.join(path.file_name().unwrap_or_default());
            replace(path, &dest)?;
            println!("installed {}", dest.display());
        }
    }
    Ok(())
}

/// Build `p` and wrap it as `<target>/bundled/<bundle>.clap`.
pub fn bundle(
    ws: &Workspace,
    p: &Package,
    clap: &Clap,
    debug: bool,
    universal: bool,
    target: Option<&str>,
) -> Result<PathBuf, String> {
    let libs = if universal {
        MACOS_TARGETS
            .iter()
            .map(|t| build_lib(ws, p, debug, Some(t)))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![build_lib(ws, p, debug, target)?]
    };

    let out_dir = ws.target_dir.join("bundled");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let out = out_dir.join(format!("{}.clap", clap.bundle));
    remove(&out)?;

    if cfg!(target_os = "macos") {
        let contents = out.join("Contents");
        let macos = contents.join("MacOS");
        std::fs::create_dir_all(&macos).map_err(|e| format!("{}: {e}", macos.display()))?;
        if let [lib] = libs.as_slice() {
            copy(lib, &macos.join(&clap.bundle))?;
        } else {
            lipo(&libs, &macos.join(&clap.bundle))?;
        }
        write(&contents.join("Info.plist"), &info_plist(clap, &p.version))?;
        write(&contents.join("PkgInfo"), "BNDL????")?;
    } else {
        copy(&libs[0], &out)?;
    }
    Ok(out)
}

/// Build `p`'s cdylib, for `target` or the host, and return its path.
fn build_lib(
    ws: &Workspace,
    p: &Package,
    debug: bool,
    target: Option<&str>,
) -> Result<PathBuf, String> {
    let lib = p.lib.as_deref().ok_or_else(|| {
        format!(
            "{} has no library target to bundle: add a [lib] with crate-type = [\"cdylib\"]",
            p.name
        )
    })?;
    let mut cargo = crate::cargo();
    cargo
        .current_dir(&ws.root)
        .args(["build", "--package", &p.name, "--lib"]);
    if !debug {
        cargo.arg("--release");
    }
    if let Some(t) = target {
        cargo.args(["--target", t]);
    }
    let status = cargo.status().map_err(|e| format!("running cargo: {e}"))?;
    if !status.success() {
        return Err(format!("building {} failed", p.name));
    }

    let profile = if debug { "debug" } else { "release" };
    let mut dir = ws.target_dir.clone();
    if let Some(t) = target {
        dir.push(t);
    }
    dir.push(profile);
    let (prefix, ext) = match std::env::consts::OS {
        "macos" => ("lib", "dylib"),
        "windows" => ("", "dll"),
        _ => ("lib", "so"),
    };
    Ok(dir.join(format!("{prefix}{lib}.{ext}")))
}

/// Join single-architecture `libs` into one universal binary at `dest`.
pub(crate) fn lipo(libs: &[PathBuf], dest: &Path) -> Result<(), String> {
    let status = Command::new("lipo")
        .arg("-create")
        .args(libs)
        .arg("-output")
        .arg(dest)
        .status()
        .map_err(|e| format!("running lipo: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("lipo could not join {libs:?}"))
    }
}

fn info_plist(clap: &Clap, version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>English</string>
    <key>CFBundleExecutable</key><string>{exe}</string>
    <key>CFBundleIdentifier</key><string>{id}</string>
    <key>CFBundleName</key><string>{name}</string>
    <key>CFBundlePackageType</key><string>BNDL</string>
    <key>CFBundleVersion</key><string>{version}</string>
    <key>CFBundleShortVersionString</key><string>{version}</string>
    <key>CFBundleSupportedPlatforms</key>
    <array><string>MacOSX</string></array>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
</dict>
</plist>
"#,
        exe = clap.bundle,
        id = clap.id,
        name = clap.name,
    )
}

/// The user's CLAP folder on this platform.
pub fn install_dir() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match std::env::consts::OS {
        "macos" => Ok(home
            .ok_or("HOME not set")?
            .join("Library/Audio/Plug-Ins/CLAP")),
        "linux" => Ok(home.ok_or("HOME not set")?.join(".clap")),
        "windows" => std::env::var_os("LOCALAPPDATA")
            .map(|d| PathBuf::from(d).join("Programs/Common/CLAP"))
            .ok_or_else(|| "LOCALAPPDATA not set".into()),
        os => Err(format!("no CLAP install folder known for {os}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_names_the_executable_id_and_version() {
        let clap = Clap {
            bundle: "MyGain".into(),
            id: "com.example.gain".into(),
            name: "My Gain".into(),
        };
        let plist = info_plist(&clap, "0.2.0");
        assert!(plist.contains("<key>CFBundleExecutable</key><string>MyGain</string>"));
        assert!(plist.contains("<string>com.example.gain</string>"));
        assert!(plist.contains("<key>CFBundleVersion</key><string>0.2.0</string>"));
    }
}
