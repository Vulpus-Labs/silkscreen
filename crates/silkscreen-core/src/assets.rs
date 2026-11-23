//! PNGs fetched at run time ([design/hosts.md]), for faceplates built in fetch
//! mode: [`EmbeddedPngs::fetched`](crate::EmbeddedPngs::fetched) names each
//! PNG by URL, and the host delivers the bytes here.
//!
//! The store is per thread (the UI thread). A raster that needs a PNG the
//! store lacks asks for it ([`take_wanted`]) and draws from the best one it
//! has meanwhile; the host fetches it, calls [`provide`], and repaints. Each
//! `provide` moves the [`generation`] on, which tells rasters that their
//! stand-ins are out of date.
//!
//! [design/hosts.md]: https://github.com/Vulpus-Labs/silkscreen/blob/main/design/hosts.md

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

#[derive(Debug, Default)]
struct Store {
    loaded: HashMap<String, Rc<[u8]>>,
    /// Asked for and not yet taken by the host.
    wanted: Vec<&'static str>,
    /// Ever asked for: each URL is fetched once, even if it fails.
    asked: HashSet<&'static str>,
    generation: u64,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

/// Deliver the bytes fetched for `url`.
pub fn provide(url: &str, bytes: Vec<u8>) {
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        s.loaded.insert(url.to_owned(), bytes.into());
        s.generation += 1;
    });
}

/// The URLs rasters have asked for since the last call, each once.
pub fn take_wanted() -> Vec<&'static str> {
    STORE.with(|s| std::mem::take(&mut s.borrow_mut().wanted))
}

/// Moves on with every [`provide`].
pub fn generation() -> u64 {
    STORE.with(|s| s.borrow().generation)
}

/// The bytes for `url`, if delivered.
pub(crate) fn get(url: &str) -> Option<Rc<[u8]>> {
    STORE.with(|s| s.borrow().loaded.get(url).cloned())
}

/// Ask the host for `url`, unless it was asked for before.
pub(crate) fn want(url: &'static str) {
    STORE.with(|s| {
        let mut s = s.borrow_mut();
        if s.asked.insert(url) {
            s.wanted.push(url);
        }
    });
}
