//! The workspace as `cargo metadata` describes it, and each package's
//! `[package.metadata.silkscreen]`.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The workspace the tool runs in.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// The directory holding the workspace's `Cargo.toml` and `Cargo.lock`.
    pub root: PathBuf,
    /// Where cargo builds to (`CARGO_TARGET_DIR` or `target/`).
    pub target_dir: PathBuf,
    pub packages: Vec<Package>,
}

/// A workspace member.
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub version: String,
    /// The directory holding its `Cargo.toml`.
    pub dir: PathBuf,
    /// Its library target's name (underscored), if it has one.
    pub lib: Option<String>,
    pub clap: Option<Clap>,
    pub web: Option<Web>,
}

/// `[package.metadata.silkscreen.clap]`: how to bundle a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clap {
    /// The bundle's file name without `.clap`, and its executable's name.
    pub bundle: String,
    /// The macOS bundle id: the plugin's `SilkscreenPlugin::ID`.
    pub id: String,
    /// The display name: the plugin's `SilkscreenPlugin::NAME`.
    pub name: String,
}

/// `[package.metadata.silkscreen.web]`: how to build a web page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Web {
    /// The engine package, a C-ABI `cdylib` for the `AudioWorklet`.
    pub engine: String,
    /// The page's folder (`index.html` and its assets).
    pub page: PathBuf,
    /// Where `deploy-web` publishes it, if anywhere.
    pub site: Option<Site>,
}

/// A page's place in a static site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The name the site's commits and `_headers` use.
    pub title: String,
    /// Its path under the site's root, without slashes at either end.
    pub subpath: String,
}

impl Workspace {
    /// Runs `cargo metadata` in `dir` (or the current directory).
    pub fn load(dir: Option<&Path>) -> Result<Self, String> {
        let mut cmd = crate::cargo();
        cmd.args(["metadata", "--no-deps", "--format-version", "1"]);
        if let Some(dir) = dir {
            cmd.current_dir(dir);
        }
        let out = cmd
            .output()
            .map_err(|e| format!("running cargo metadata: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "cargo metadata failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Self::from_json(&String::from_utf8_lossy(&out.stdout))
    }

    /// The workspace from `cargo metadata`'s JSON.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let v: Value =
            serde_json::from_str(json).map_err(|e| format!("reading cargo metadata: {e}"))?;
        let path = |key: &str| {
            v[key]
                .as_str()
                .map(PathBuf::from)
                .ok_or_else(|| format!("cargo metadata has no {key}"))
        };
        let members: Vec<&str> = v["workspace_members"]
            .as_array()
            .map(|m| m.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut packages = Vec::new();
        for p in v["packages"].as_array().into_iter().flatten() {
            if p["id"].as_str().is_some_and(|id| members.contains(&id)) {
                packages.push(Package::from_json(p)?);
            }
        }
        Ok(Self {
            root: path("workspace_root")?,
            target_dir: path("target_directory")?,
            packages,
        })
    }

    /// The member called `name`. The error lists those that have the
    /// section `what` needs.
    pub fn package(&self, name: &str, what: Section) -> Result<&Package, String> {
        if let Some(p) = self.packages.iter().find(|p| p.name == name) {
            return Ok(p);
        }
        let offered: Vec<&str> = self
            .packages
            .iter()
            .filter(|p| what.present(p))
            .map(|p| p.name.as_str())
            .collect();
        Err(format!(
            "no package {name:?} in this workspace; packages with [package.metadata.silkscreen.{}]: {}",
            what.key(),
            if offered.is_empty() {
                "none".into()
            } else {
                offered.join(", ")
            }
        ))
    }
}

/// A part of `[package.metadata.silkscreen]`.
#[derive(Debug, Clone, Copy)]
pub enum Section {
    Clap,
    Web,
}

impl Section {
    fn key(self) -> &'static str {
        match self {
            Self::Clap => "clap",
            Self::Web => "web",
        }
    }

    fn present(self, p: &Package) -> bool {
        match self {
            Self::Clap => p.clap.is_some(),
            Self::Web => p.web.is_some(),
        }
    }

    /// The fix for a package without this section.
    pub fn missing(self, package: &str) -> String {
        match self {
            Self::Clap => format!(
                "{package} has no [package.metadata.silkscreen.clap]: add `bundle`, `id` and `name` to its Cargo.toml"
            ),
            Self::Web => format!(
                "{package} has no [package.metadata.silkscreen.web]: add `engine` (the engine package) and `page` (the page's folder) to its Cargo.toml"
            ),
        }
    }
}

impl Package {
    fn from_json(p: &Value) -> Result<Self, String> {
        let name = p["name"].as_str().unwrap_or_default().to_owned();
        let manifest = PathBuf::from(p["manifest_path"].as_str().unwrap_or_default());
        let dir = manifest.parent().unwrap_or(Path::new(".")).to_path_buf();
        let lib = p["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| {
                t["kind"].as_array().is_some_and(|k| {
                    k.iter()
                        .any(|k| matches!(k.as_str(), Some("lib" | "cdylib" | "rlib")))
                })
            })
            .and_then(|t| t["name"].as_str())
            .map(|n| n.replace('-', "_"));
        let ours = &p["metadata"]["silkscreen"];
        let clap = section(&name, ours, "clap", |s| {
            Ok(Clap {
                bundle: field(&name, s, "clap", "bundle")?,
                id: field(&name, s, "clap", "id")?,
                name: field(&name, s, "clap", "name")?,
            })
        })?;
        let web = section(&name, ours, "web", |s| {
            let site = match &s["site"] {
                Value::Null => None,
                site => Some(Site {
                    title: field(&name, site, "web.site", "title")?,
                    subpath: field(&name, site, "web.site", "subpath")?
                        .trim_matches('/')
                        .to_owned(),
                }),
            };
            Ok(Web {
                engine: field(&name, s, "web", "engine")?,
                page: dir.join(field(&name, s, "web", "page")?),
                site,
            })
        })?;
        Ok(Self {
            name,
            version: p["version"].as_str().unwrap_or_default().to_owned(),
            dir,
            lib,
            clap,
            web,
        })
    }
}

/// `ours.key`, read by `read` if it is there.
fn section<T>(
    package: &str,
    ours: &Value,
    key: &str,
    read: impl FnOnce(&Value) -> Result<T, String>,
) -> Result<Option<T>, String> {
    match &ours[key] {
        Value::Null => Ok(None),
        s if s.is_object() => read(s).map(Some),
        _ => Err(format!(
            "{package}: [package.metadata.silkscreen.{key}] must be a table"
        )),
    }
}

/// A string field, or an error naming the package, the section and the fix.
fn field(package: &str, section: &Value, path: &str, key: &str) -> Result<String, String> {
    section[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "{package}: [package.metadata.silkscreen.{path}] needs `{key}`, a string: add it to the package's Cargo.toml"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(packages: &str) -> String {
        format!(
            r#"{{"packages":[{packages}],"workspace_members":["a","b"],"workspace_root":"/ws","target_directory":"/ws/target"}}"#
        )
    }

    const PLUGIN: &str = r#"{"id":"a","name":"my-plugin","version":"0.2.0","manifest_path":"/ws/my-plugin/Cargo.toml",
        "targets":[{"kind":["cdylib","rlib"],"name":"my_plugin"}],
        "metadata":{"silkscreen":{"clap":{"bundle":"MyPlugin","id":"com.example.my","name":"My Plugin"}}}}"#;
    const WEB: &str = r#"{"id":"b","name":"my-web","version":"0.2.0","manifest_path":"/ws/my-web/Cargo.toml",
        "targets":[{"kind":["cdylib","rlib"],"name":"my_web"}],
        "metadata":{"silkscreen":{"web":{"engine":"my-web-engine","page":"web","site":{"title":"Mine","subpath":"/x/web/"}}}}}"#;

    #[test]
    fn sections_are_read_from_members() {
        let ws = Workspace::from_json(&metadata(&format!("{PLUGIN},{WEB}"))).unwrap();
        assert_eq!(ws.root, PathBuf::from("/ws"));
        assert_eq!(ws.target_dir, PathBuf::from("/ws/target"));
        let p = ws.package("my-plugin", Section::Clap).unwrap();
        assert_eq!(p.lib.as_deref(), Some("my_plugin"));
        assert_eq!(
            p.clap,
            Some(Clap {
                bundle: "MyPlugin".into(),
                id: "com.example.my".into(),
                name: "My Plugin".into()
            })
        );
        let w = ws
            .package("my-web", Section::Web)
            .unwrap()
            .web
            .clone()
            .unwrap();
        assert_eq!(w.page, PathBuf::from("/ws/my-web/web"));
        assert_eq!(w.site.unwrap().subpath, "x/web", "slashes trimmed");
    }

    #[test]
    fn a_missing_field_names_the_package_section_and_key() {
        let bad = PLUGIN.replace(r#""id":"com.example.my","#, "");
        let e = Workspace::from_json(&metadata(&bad)).unwrap_err();
        assert!(
            e.contains("my-plugin")
                && e.contains("[package.metadata.silkscreen.clap]")
                && e.contains("`id`"),
            "{e}"
        );
    }

    #[test]
    fn an_unknown_package_lists_the_ones_that_would_do() {
        let ws = Workspace::from_json(&metadata(&format!("{PLUGIN},{WEB}"))).unwrap();
        let e = ws.package("nope", Section::Clap).unwrap_err();
        assert!(e.contains("my-plugin") && !e.contains("my-web"), "{e}");
    }
}
