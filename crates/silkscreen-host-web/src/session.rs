//! The page's session and preset files (design/presets.md), wasm only.
//!
//! - [`WebPresetIo`]: the browser footer's Export (a download of a preset
//!   or a library), Import (a file picker, or `.preset` and `.presetbank`
//!   files dropped on the editor) and Share (a `#patch=` link on the
//!   clipboard). design/presets.md.
//! - [`take_patch_fragment`]: a share link's preset, read from the
//!   address and stripped from it.
//! - [`load_session`] and [`autosave`]: the current sound (as preset
//!   text) and the editor's UI state, kept in the database's `state`
//!   store under `session`, written at most every 400 ms while it changes
//!   and when the page is hidden.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use js_sys::{Array, Object, Reflect};
use silkscreen::library::EXT as LIBRARY_EXT;
use silkscreen::presets;
use silkscreen::{EditorState, IoEvent, PresetIo};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    Blob, BlobPropertyBag, HtmlAnchorElement, HtmlInputElement, IdbTransactionMode, Url,
};

use crate::WebHost;
use crate::idb::{STATE, committed, describe, done, open};
use crate::share::{decode_patch, encode_patch};

const SESSION_KEY: &str = "session";
const AUTOSAVE_MS: i32 = 400;

type Inbox = Rc<RefCell<VecDeque<IoEvent>>>;

/// Export, Import and Share in a browser.
#[derive(Debug, Default)]
pub struct WebPresetIo {
    inbox: Inbox,
    /// The last link Share made, for checks and the console.
    last_link: RefCell<Option<String>>,
}

impl WebPresetIo {
    pub fn new() -> Self {
        Self::default()
    }

    /// The last link [`share`](PresetIo::share) made.
    pub fn last_link(&self) -> Option<String> {
        self.last_link.borrow().clone()
    }

    /// Read `file`'s text into the inbox.
    fn read(&self, file: web_sys::File) {
        let inbox = Rc::clone(&self.inbox);
        spawn_local(async move {
            let result = JsFuture::from(file.text())
                .await
                .map_err(|e| describe(&e))
                .and_then(|t| t.as_string().ok_or_else(|| "not text".to_owned()));
            inbox.borrow_mut().push_back(match result {
                Ok(text) => IoEvent::Imported(text),
                Err(e) => IoEvent::Failed(presets::import_failed(e)),
            });
        });
    }
}

fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

/// Download `text` as `file_name`, through a Blob URL.
fn download(file_name: &str, text: &str) -> Result<(), String> {
    let document = document().ok_or("no document")?;
    let options = BlobPropertyBag::new();
    options.set_type("text/plain");
    let parts = Array::of1(&JsValue::from_str(text));
    let blob =
        Blob::new_with_str_sequence_and_options(&parts, &options).map_err(|e| describe(&e))?;
    let url = Url::create_object_url_with_blob(&blob).map_err(|e| describe(&e))?;
    let a = document
        .create_element("a")
        .and_then(|e| e.dyn_into::<HtmlAnchorElement>().map_err(Into::into))
        .map_err(|e| describe(&e));
    if let Ok(a) = &a {
        a.set_href(&url);
        a.set_download(file_name);
        a.click();
    }
    let _ = Url::revoke_object_url(&url);
    a.map(|_| ())
}

impl PresetIo for WebPresetIo {
    fn export(&self, file_name: &str, text: &str) {
        let event = match download(file_name, text) {
            Ok(()) => IoEvent::Exported(file_name.to_owned()),
            Err(e) => IoEvent::Failed(presets::export_failed(e)),
        };
        self.inbox.borrow_mut().push_back(event);
    }

    fn import(&self) {
        let Some(document) = document() else { return };
        let Ok(input) = document
            .create_element("input")
            .and_then(|e| e.dyn_into::<HtmlInputElement>().map_err(Into::into))
        else {
            return;
        };
        input.set_type("file");
        input.set_accept(&format!(".{},.{LIBRARY_EXT},text/plain", presets::EXT));
        let inbox = Rc::clone(&self.inbox);
        let picked = input.clone();
        let on_change = Closure::once_into_js(move || {
            if let Some(file) = picked.files().and_then(|f| f.get(0)) {
                let io = WebPresetIo {
                    inbox,
                    last_link: RefCell::new(None),
                };
                io.read(file);
            }
        });
        input.set_onchange(Some(on_change.unchecked_ref()));
        input.click();
    }

    fn import_dropped(&self, handles: &[u32]) -> bool {
        let mut any = false;
        for &h in handles {
            if let Some(file) = crate::take_dropped_file(h)
                && presets::is_import_file(&file.name())
            {
                any = true;
                self.read(file);
            }
        }
        any
    }

    fn can_share(&self) -> bool {
        true
    }

    fn share(&self, text: &str) -> Result<(), String> {
        let window = web_sys::window().ok_or("no window")?;
        let location = window.location();
        let base = format!(
            "{}{}{}",
            location.origin().map_err(|e| describe(&e))?,
            location.pathname().map_err(|e| describe(&e))?,
            location.search().map_err(|e| describe(&e))?
        );
        let link = format!("{base}{}", encode_patch(text));
        let clipboard = window.navigator().clipboard();
        let promise = clipboard.write_text(&link);
        spawn_local(async move {
            if let Err(e) = JsFuture::from(promise).await {
                web_sys::console::warn_1(
                    &format!("couldn't copy the link: {}", describe(&e)).into(),
                );
            }
        });
        *self.last_link.borrow_mut() = Some(link);
        Ok(())
    }

    fn take_event(&self) -> Option<IoEvent> {
        self.inbox.borrow_mut().pop_front()
    }
}

/// A share link's preset text, if the address carries one; the fragment
/// is stripped from the address either way.
pub fn take_patch_fragment() -> Option<Result<String, String>> {
    let window = web_sys::window()?;
    let location = window.location();
    let hash = location.hash().ok()?;
    let patch = decode_patch(&hash)?;
    if let (Ok(history), Ok(path), Ok(search)) =
        (window.history(), location.pathname(), location.search())
    {
        let _ =
            history.replace_state_with_url(&JsValue::NULL, "", Some(&format!("{path}{search}")));
    }
    Some(patch)
}

/// A saved session: the sound as preset text, and the editor's UI state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub preset: String,
    pub ui: Vec<(String, String)>,
}

impl Session {
    /// `state`'s session now.
    pub fn of(state: &EditorState) -> Self {
        Self {
            preset: state.current_preset_text().1,
            ui: state.ui_state(),
        }
    }

    /// Put this session into `state`: its sound (one load) and UI state.
    pub fn restore(&self, state: &mut EditorState) -> Result<(), String> {
        state.import_preset_text(&self.preset)?;
        state.restore_ui_state(&self.ui);
        Ok(())
    }

    fn to_js(&self) -> JsValue {
        let object = Object::new();
        let ui: Array = self
            .ui
            .iter()
            .map(|(k, v)| JsValue::from(Array::of2(&k.into(), &v.into())))
            .collect();
        let _ = Reflect::set(&object, &"preset".into(), &self.preset.as_str().into());
        let _ = Reflect::set(&object, &"ui".into(), &ui);
        object.into()
    }

    fn from_js(value: &JsValue) -> Option<Self> {
        let preset = Reflect::get(value, &"preset".into()).ok()?.as_string()?;
        let ui = Array::from(&Reflect::get(value, &"ui".into()).ok()?)
            .iter()
            .filter_map(|pair| {
                let pair = Array::from(&pair);
                Some((pair.get(0).as_string()?, pair.get(1).as_string()?))
            })
            .collect();
        Some(Self { preset, ui })
    }
}

/// The session saved for `plugin`, if there is one.
pub async fn load_session(plugin: &str) -> Option<Session> {
    let db = open(plugin).await.ok()?;
    let tx = db.transaction_with_str(STATE).ok()?;
    let value = done(&tx.object_store(STATE).ok()?.get(&SESSION_KEY.into()).ok()?)
        .await
        .ok()?;
    Session::from_js(&value)
}

/// Keep `host`'s session for `plugin`: every 400 ms, and when the page
/// is hidden, write it if it changed. Runs for the page's lifetime.
pub async fn autosave(plugin: String, host: Rc<WebHost>) {
    let db = match open(&plugin).await {
        Ok(db) => Rc::new(db),
        Err(e) => {
            web_sys::console::warn_1(&format!("no session autosave: {}", describe(&e)).into());
            return;
        }
    };
    let last: Rc<RefCell<Option<Session>>> = Rc::new(RefCell::new(None));
    let save = Rc::new(move || {
        let now = host.with_editor(|e| Session::of(&e.state));
        if last.borrow().as_ref() == Some(&now) {
            return;
        }
        *last.borrow_mut() = Some(now.clone());
        let db = Rc::clone(&db);
        spawn_local(async move {
            let result = async {
                let tx = db.transaction_with_str_and_mode(STATE, IdbTransactionMode::Readwrite)?;
                tx.object_store(STATE)?
                    .put_with_key(&now.to_js(), &SESSION_KEY.into())?;
                committed(&tx).await
            }
            .await;
            if let Err(e) = result {
                web_sys::console::warn_1(
                    &format!("couldn't keep the session: {}", describe(&e)).into(),
                );
            }
        });
    });
    let Some(window) = web_sys::window() else {
        return;
    };
    let tick = Rc::clone(&save);
    let interval = Closure::<dyn FnMut()>::new(move || tick());
    let _ = window.set_interval_with_callback_and_timeout_and_arguments_0(
        interval.as_ref().unchecked_ref(),
        AUTOSAVE_MS,
    );
    interval.forget();
    if let Some(document) = document() {
        let hidden = Closure::<dyn FnMut()>::new(move || save());
        let _ = document
            .add_event_listener_with_callback("visibilitychange", hidden.as_ref().unchecked_ref());
        hidden.forget();
    }
}
