//! What a page starts from (design/presets.md): a `#patch=` share link's
//! preset, else the last session; a bad link falls back to the session
//! and says why in the preset field.

use silkscreen::model::ParamSpec;
use silkscreen::{EditorState, Preset};

/// Where a page's sound comes from at load.
#[derive(Debug)]
pub enum Restore {
    /// A share link's preset text.
    Shared(String),
    /// The last session, and what was wrong with a share link, if one
    /// was given.
    #[cfg(target_arch = "wasm32")]
    Session(crate::session::Session, Option<String>),
    /// A share link that couldn't be read, and no session.
    BadLink(String),
}

impl Restore {
    /// The preset text the sound is restored from, if any.
    pub fn preset_text(&self) -> Option<&str> {
        match self {
            Restore::Shared(text) => Some(text),
            #[cfg(target_arch = "wasm32")]
            Restore::Session(s, _) => Some(&s.preset),
            Restore::BadLink(_) => None,
        }
    }

    /// Restore `state`, saying in its preset field what happened.
    pub fn apply(self, state: &mut EditorState) {
        match self {
            Restore::Shared(text) => match state.import_preset_text(&text) {
                Ok(name) => state.flash_preset(&format!("Shared: \u{201c}{name}\u{201d}")),
                Err(e) => state.flash_preset(&format!("Couldn't load the link: {e}")),
            },
            #[cfg(target_arch = "wasm32")]
            Restore::Session(session, problem) => {
                if let Err(e) = session.restore(state) {
                    state.flash_preset(&format!("Couldn't restore the session: {e}"));
                } else if let Some(p) = problem {
                    state.flash_preset(&p);
                }
            }
            Restore::BadLink(problem) => state.flash_preset(&problem),
        }
    }
}

/// The values an engine starts from: `restore`'s sound, else every
/// param's default.
pub fn initial_values(restore: Option<&Restore>, specs: &[ParamSpec]) -> Vec<f32> {
    restore
        .and_then(Restore::preset_text)
        .and_then(|text| Preset::parse(text).ok())
        .map_or_else(
            || specs.iter().map(|p| p.default).collect(),
            |p| p.values_for(specs),
        )
}

/// The share link's preset if the address has one, else plugin
/// `plugin`'s last session.
#[cfg(target_arch = "wasm32")]
pub async fn restore(plugin: &str) -> Option<Restore> {
    use crate::session::{load_session, take_patch_fragment};
    let problem = match take_patch_fragment() {
        Some(Ok(text)) if Preset::parse(&text).is_ok() => return Some(Restore::Shared(text)),
        Some(Ok(_)) => Some("Couldn't load the link: it isn't a preset".to_owned()),
        Some(Err(e)) => Some(format!("Couldn't load the link: {e}")),
        None => None,
    };
    match load_session(plugin).await {
        Some(session) => Some(Restore::Session(session, problem)),
        None => problem.map(Restore::BadLink),
    }
}

/// Off the web there is no address or session to restore.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::unused_async)]
pub async fn restore(_plugin: &str) -> Option<Restore> {
    None
}
