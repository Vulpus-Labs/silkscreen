//! Developer ID signing and notarization of macOS bundles (design/building-and-shipping.md).
//!
//! `cargo silkscreen bundle <package> --sign` signs the bundle with the identity
//! in `SILKSCREEN_SIGN_IDENTITY`, hardened runtime and a secure timestamp.
//! `--notarize` (implies `--sign`) zips the bundle, submits it to Apple's
//! notary service, waits, staples the ticket and asks Gatekeeper to assess
//! the result. The notary credentials come from either
//!
//! - `SILKSCREEN_NOTARY_PROFILE`: a keychain profile saved by
//!   `xcrun notarytool store-credentials` (the development Mac), or
//! - `SILKSCREEN_NOTARY_KEY`, `SILKSCREEN_NOTARY_KEY_ID` and
//!   `SILKSCREEN_NOTARY_ISSUER`: an App Store Connect API key's `.p8` path,
//!   key id and issuer id (CI).

use std::path::Path;
use std::process::Command;

pub const IDENTITY: &str = "SILKSCREEN_SIGN_IDENTITY";
pub const PROFILE: &str = "SILKSCREEN_NOTARY_PROFILE";
pub const KEY: &str = "SILKSCREEN_NOTARY_KEY";
pub const KEY_ID: &str = "SILKSCREEN_NOTARY_KEY_ID";
pub const ISSUER: &str = "SILKSCREEN_NOTARY_ISSUER";

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// The signing identity, from `SILKSCREEN_SIGN_IDENTITY`.
pub fn identity() -> Result<String, String> {
    identity_from(env)
}

fn identity_from(get: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    get(IDENTITY).ok_or_else(|| {
        format!(
            "--sign needs {IDENTITY}, e.g. \"Developer ID Application: <name> (<team id>)\"; \
             `security find-identity -v -p codesigning` lists the identities on this Mac"
        )
    })
}

/// `notarytool`'s credential arguments, from the environment.
pub fn notary_credentials() -> Result<Vec<String>, String> {
    notary_credentials_from(env)
}

fn notary_credentials_from(get: impl Fn(&str) -> Option<String>) -> Result<Vec<String>, String> {
    if let Some(profile) = get(PROFILE) {
        return Ok(vec!["--keychain-profile".into(), profile]);
    }
    match (get(KEY), get(KEY_ID), get(ISSUER)) {
        (Some(key), Some(id), Some(issuer)) => Ok(vec![
            "--key".into(),
            key,
            "--key-id".into(),
            id,
            "--issuer".into(),
            issuer,
        ]),
        (None, None, None) => Err(format!(
            "--notarize needs {PROFILE} (a profile saved by `xcrun notarytool store-credentials`) \
             or {KEY}, {KEY_ID} and {ISSUER} (an App Store Connect API key)"
        )),
        (key, id, issuer) => {
            let missing: Vec<_> = [(KEY, key), (KEY_ID, id), (ISSUER, issuer)]
                .into_iter()
                .filter(|(_, v)| v.is_none())
                .map(|(k, _)| k)
                .collect();
            Err(format!(
                "--notarize with an API key needs {KEY}, {KEY_ID} and {ISSUER}; {} not set",
                missing.join(", ")
            ))
        }
    }
}

/// Sign `bundle` with `identity`: hardened runtime, secure timestamp.
pub fn sign(bundle: &Path, identity: &str) -> Result<(), String> {
    run(Command::new("codesign")
        .args(["--force", "--options", "runtime", "--timestamp", "--sign"])
        .arg(identity)
        .arg(bundle))?;
    run(Command::new("codesign")
        .args(["--verify", "--strict", "--verbose=2"])
        .arg(bundle))
}

/// Notarize `bundle`, staple the ticket, and check Gatekeeper accepts it.
pub fn notarize(bundle: &Path, credentials: &[String]) -> Result<(), String> {
    let zip = bundle.with_extension("notarize.zip");
    run(Command::new("ditto")
        .args(["-c", "-k", "--keepParent"])
        .arg(bundle)
        .arg(&zip))?;
    let out = Command::new("xcrun")
        .args(["notarytool", "submit"])
        .arg(&zip)
        .args(credentials)
        .arg("--wait")
        .output()
        .map_err(|e| format!("running notarytool: {e}"));
    let _ = std::fs::remove_file(&zip);
    let out = out?;
    let text = String::from_utf8_lossy(&out.stdout);
    print!("{text}");
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    if !out.status.success() || !accepted(&text) {
        if let Some(id) = submission_id(&text) {
            let _ = Command::new("xcrun")
                .args(["notarytool", "log", id])
                .args(credentials)
                .status();
        }
        return Err(format!(
            "notarizing {} failed; the log above says why",
            bundle.display()
        ));
    }
    run(Command::new("xcrun")
        .args(["stapler", "staple"])
        .arg(bundle))?;
    run(Command::new("spctl")
        .args(["--assess", "--type", "install", "-vv"])
        .arg(bundle))
}

/// Whether `notarytool submit --wait`'s output reports acceptance.
fn accepted(text: &str) -> bool {
    text.lines().any(|l| l.trim() == "status: Accepted")
}

/// The submission id in `notarytool submit`'s output.
fn submission_id(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|l| l.trim().strip_prefix("id: "))
        .map(str::trim)
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let name = cmd.get_program().to_string_lossy().into_owned();
    let status = cmd.status().map_err(|e| format!("running {name}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} failed ({status})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(set: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            set.iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn a_missing_identity_names_the_variable_and_the_lookup() {
        let e = identity_from(vars(&[])).unwrap_err();
        assert!(e.contains(IDENTITY) && e.contains("find-identity"), "{e}");
        assert_eq!(identity_from(vars(&[(IDENTITY, "Dev")])).unwrap(), "Dev");
    }

    #[test]
    fn a_keychain_profile_wins() {
        let c = notary_credentials_from(vars(&[(PROFILE, "p"), (KEY, "k.p8")])).unwrap();
        assert_eq!(c, ["--keychain-profile", "p"]);
    }

    #[test]
    fn an_api_key_needs_all_three_parts() {
        let all = [(KEY, "k.p8"), (KEY_ID, "ID"), (ISSUER, "ISS")];
        assert_eq!(
            notary_credentials_from(vars(&all)).unwrap(),
            ["--key", "k.p8", "--key-id", "ID", "--issuer", "ISS"]
        );
        let e = notary_credentials_from(vars(&all[..1])).unwrap_err();
        assert!(e.contains(KEY_ID) && e.contains(ISSUER), "{e}");
        let e = notary_credentials_from(vars(&[])).unwrap_err();
        assert!(e.contains(PROFILE) && e.contains(KEY), "{e}");
    }

    #[test]
    fn submit_output_is_read_for_status_and_id() {
        let ok = "Conducting pre-submission checks...\n  id: 2efe2717-52ef\n  status: Accepted\n";
        assert!(accepted(ok));
        assert_eq!(submission_id(ok), Some("2efe2717-52ef"));
        assert!(!accepted("  id: x\n  status: Invalid\n"));
    }
}
