//! The web preset store's `IndexedDB` side (design/presets.md): open the
//! database, hydrate a [`JournaledStore`] from it, and write the journal
//! back after each change and when the page is hidden.
//!
//! Database `<plugin>-presets`, version 1:
//!
//! - `presets`: preset text by key (`name`, or `folder/name`);
//! - `folders`: each folder's name, by its name (empty folders too);
//! - `state`: the page's session.

use std::cell::Cell;
use std::rc::Rc;

use js_sys::{Array, Promise};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{IdbDatabase, IdbRequest, IdbTransaction, IdbTransactionMode};

use silkscreen::PresetStore as _;

use crate::store::{JournaledStore, Write};

const VERSION: u32 = 1;
const PRESETS: &str = "presets";
const FOLDERS: &str = "folders";
/// The session's store: the page's state, restored on load.
pub(crate) const STATE: &str = "state";

/// The user's presets for `plugin`, from `IndexedDB`. Await it before
/// building the editor. If the database can't be opened (a private
/// window, storage turned off) the store keeps presets in memory only and
/// says so in [`problem`](silkscreen::PresetStore::problem).
pub async fn open_preset_store(plugin: &str) -> JournaledStore {
    request_persistence();
    match open(plugin).await {
        Ok(db) => match hydrate(&db).await {
            Ok(store) => {
                attach(&store, db);
                store
            }
            Err(e) => memory_only(&e),
        },
        Err(e) => memory_only(&e),
    }
}

fn memory_only(e: &JsValue) -> JournaledStore {
    let store = JournaledStore::default();
    store.set_problem(Some(format!(
        "Presets can't be kept in this browser ({}); they last until the page closes",
        describe(e)
    )));
    store
}

/// Ask the browser not to evict the database under storage pressure. The
/// answer doesn't matter: without it, storage is merely best-effort.
fn request_persistence() {
    if let Some(window) = web_sys::window()
        && let Ok(promise) = window.navigator().storage().persist()
    {
        spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

pub(crate) async fn open(plugin: &str) -> Result<IdbDatabase, JsValue> {
    let factory = web_sys::window()
        .ok_or("no window")?
        .indexed_db()?
        .ok_or("IndexedDB is unavailable")?;
    let request = factory.open_with_u32(&format!("{plugin}-presets"), VERSION)?;
    let upgrade = Closure::<dyn FnMut(web_sys::Event)>::new({
        let request = request.clone();
        move |_| {
            if let Ok(db) = request.result().and_then(JsCast::dyn_into::<IdbDatabase>) {
                let names = db.object_store_names();
                for store in [PRESETS, FOLDERS, STATE] {
                    if !names.contains(store) {
                        let _ = db.create_object_store(store);
                    }
                }
            }
        }
    });
    request.set_onupgradeneeded(Some(upgrade.as_ref().unchecked_ref()));
    let db = done(&request).await;
    request.set_onupgradeneeded(None);
    drop(upgrade);
    db?.dyn_into::<IdbDatabase>()
}

async fn hydrate(db: &IdbDatabase) -> Result<JournaledStore, JsValue> {
    let tx = db.transaction_with_str_sequence(&names(&[PRESETS, FOLDERS]))?;
    let presets = tx.object_store(PRESETS)?;
    let keys = Array::from(&done(&presets.get_all_keys()?).await?);
    let texts = Array::from(&done(&presets.get_all()?).await?);
    let folders = Array::from(&done(&tx.object_store(FOLDERS)?.get_all_keys()?).await?);
    let presets = keys
        .iter()
        .zip(texts.iter())
        .filter_map(|(k, t)| Some((k.as_string()?, t.as_string()?)))
        .collect();
    let folders = folders.iter().filter_map(|f| f.as_string()).collect();
    Ok(JournaledStore::hydrate(folders, presets))
}

/// Flush `store` to `db` after each change and when the page is hidden.
fn attach(store: &JournaledStore, db: IdbDatabase) {
    let flusher = Rc::new(Flusher {
        store: store.clone(),
        db,
        busy: Cell::new(false),
    });
    let f = Rc::clone(&flusher);
    store.set_on_change(move || Flusher::flush(&f));
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        let f = Rc::clone(&flusher);
        let hidden = Closure::<dyn FnMut()>::new(move || Flusher::flush(&f));
        let _ = document
            .add_event_listener_with_callback("visibilitychange", hidden.as_ref().unchecked_ref());
        // The page's lifetime.
        hidden.forget();
    }
}

struct Flusher {
    store: JournaledStore,
    db: IdbDatabase,
    /// A flush is in flight; changes meanwhile wait for the next.
    busy: Cell<bool>,
}

impl Flusher {
    /// Write the journal out, unless a flush is already going (it flushes
    /// again when it ends if more changed).
    fn flush(this: &Rc<Self>) {
        if this.busy.get() || !this.store.is_dirty() {
            return;
        }
        this.busy.set(true);
        let this = Rc::clone(this);
        spawn_local(async move {
            let (keys, writes) = this.store.take_writes();
            match write_all(&this.db, &writes).await {
                Ok(()) => this.store.set_problem(None),
                Err(e) => {
                    let problem = format!("Couldn't save presets: {}", describe(&e));
                    web_sys::console::warn_1(&problem.clone().into());
                    this.store.requeue(keys, problem);
                }
            }
            this.busy.set(false);
            // A failure retries on the next change or when the page hides,
            // rather than spinning here.
            if this.store.problem().is_none() {
                Self::flush(&this);
            }
        });
    }
}

async fn write_all(db: &IdbDatabase, writes: &[Write]) -> Result<(), JsValue> {
    let tx = db.transaction_with_str_sequence_and_mode(
        &names(&[PRESETS, FOLDERS]),
        IdbTransactionMode::Readwrite,
    )?;
    let presets = tx.object_store(PRESETS)?;
    let folders = tx.object_store(FOLDERS)?;
    for w in writes {
        match w {
            Write::PutPreset { key, text } => {
                presets.put_with_key(&text.into(), &key.into())?;
            }
            Write::DeletePreset { key } => {
                presets.delete(&key.into())?;
            }
            Write::PutFolder { name } => {
                folders.put_with_key(&name.into(), &name.into())?;
            }
            Write::DeleteFolder { name } => {
                folders.delete(&name.into())?;
            }
        }
    }
    committed(&tx).await
}

fn names(stores: &[&str]) -> Array {
    stores.iter().map(|s| JsValue::from_str(s)).collect()
}

/// A request's result, once it succeeds.
pub(crate) async fn done(request: &IdbRequest) -> Result<JsValue, JsValue> {
    let promise = Promise::new(&mut |resolve, reject| {
        let ok = request.clone();
        let on_success = Closure::once_into_js(move |_: web_sys::Event| {
            let _ = resolve.call1(&JsValue::NULL, &ok.result().unwrap_or(JsValue::UNDEFINED));
        });
        let bad = request.clone();
        let on_error = Closure::once_into_js(move |_: web_sys::Event| {
            let error = bad.error().ok().flatten().map_or(JsValue::NULL, Into::into);
            let _ = reject.call1(&JsValue::NULL, &error);
        });
        request.set_onsuccess(Some(on_success.unchecked_ref()));
        request.set_onerror(Some(on_error.unchecked_ref()));
    });
    JsFuture::from(promise).await
}

/// Once `tx` commits (or why it didn't).
pub(crate) async fn committed(tx: &IdbTransaction) -> Result<(), JsValue> {
    let promise = Promise::new(&mut |resolve, reject| {
        let on_complete = Closure::once_into_js(move |_: web_sys::Event| {
            let _ = resolve.call0(&JsValue::NULL);
        });
        let failed = tx.clone();
        let reject2 = reject.clone();
        let failed2 = tx.clone();
        let on_error = Closure::once_into_js(move |_: web_sys::Event| {
            let error = failed.error().map_or(JsValue::NULL, Into::into);
            let _ = reject.call1(&JsValue::NULL, &error);
        });
        let on_abort = Closure::once_into_js(move |_: web_sys::Event| {
            let error = failed2.error().map_or_else(
                || JsValue::from_str("the transaction was aborted"),
                Into::into,
            );
            let _ = reject2.call1(&JsValue::NULL, &error);
        });
        tx.set_oncomplete(Some(on_complete.unchecked_ref()));
        tx.set_onerror(Some(on_error.unchecked_ref()));
        tx.set_onabort(Some(on_abort.unchecked_ref()));
    });
    JsFuture::from(promise).await.map(|_| ())
}

/// A JS error's message, or its text.
pub(crate) fn describe(e: &JsValue) -> String {
    e.dyn_ref::<web_sys::DomException>()
        .map(|d| format!("{}: {}", d.name(), d.message()))
        .or_else(|| {
            e.dyn_ref::<js_sys::Error>()
                .map(|e| String::from(e.message()))
        })
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"))
}
