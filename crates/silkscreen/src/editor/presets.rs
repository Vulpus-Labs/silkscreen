//! The current preset and the plugin's presets, as the editor state
//! keeps them (design/presets.md): loading, stepping, saving, importing and
//! exporting, and random sounds (design/presets.md).

use std::cell::RefCell;
use std::rc::Rc;

use silkscreen_bridge::ParamIndex;
use silkscreen_model::{AppVersion, FactoryPreset, Preset};

use super::EditorState;
use crate::browser::{Place, PresetBrowser};
use crate::library::{
    LibraryImport, check_library, export_library, import_library, is_library, parse_library,
};
use crate::presets::{
    FieldText, IoEvent, PresetIo, PresetPath, PresetStore, SharedStore, import_failed,
    is_import_file,
};
use crate::prompt::TextPrompt;
use crate::random::{RandomSound, Rng, Seed};

/// What the editor state keeps about presets.
#[derive(Default)]
pub(super) struct PresetSession {
    /// The current preset as preset fields show it: its name, whether
    /// it's modified, and any brief message. The one record of each.
    pub label: FieldText,
    /// The current preset's comment: a factory preset's line about the
    /// sound, a random sound's flavour and seed. The name field's tooltip
    /// shows it, and saving keeps it.
    pub comment: Option<String>,
    /// A preset is loading: its changes don't count as modifications.
    pub loading: bool,
    /// Where the current preset lives, if it's one of the bank's or the
    /// store's (Save overwrites only a user preset).
    pub place: Option<Place>,
    /// Where presets come from, if the plugin set them up.
    pub setup: Option<PresetSetup>,
    /// Export, Import (and on the web Share), where the host has them.
    pub io: Option<Rc<dyn PresetIo>>,
    /// Where names are typed, if not in the faceplate's own fields
    /// (plugins: the DAW takes keys).
    pub prompt: Option<Rc<dyn TextPrompt>>,
    /// The plugin and version every preset and library written is marked
    /// with (design/presets.md), if the plugin said.
    pub app: Option<AppVersion>,
    /// How the plugin builds a random sound (design/presets.md).
    pub randomiser: Option<Randomiser>,
    /// The user folder Save As offers for the current random sound.
    pub random_folder: Option<String>,
    /// The kind of sound a seed gives (a flavour), if the plugin says,
    /// and the last press's: a repeat is redrawn half the time.
    pub random_kind: Option<Box<dyn Fn(Seed) -> usize>>,
    pub last_random_kind: Option<usize>,
    /// Where the next random sound comes from: seeded by the view's
    /// clock, advanced by each use.
    pub random_seed: u64,
}

/// A plugin's random sound, from the generator, the values before and
/// the defaults.
pub(super) type Randomiser = Box<dyn Fn(&mut Rng, &[f32], &[f32]) -> RandomSound>;

/// A plugin's presets: the user's store and the factory set.
pub(super) struct PresetSetup {
    pub store: Rc<RefCell<Box<dyn PresetStore>>>,
    pub factory: &'static [FactoryPreset],
}

impl EditorState {
    /// The current values as a preset called `name`, with the current
    /// preset's comment.
    pub fn capture_preset(&self, name: &str) -> Preset {
        let specs = self.model.specs();
        Preset {
            product: self.presets.app.map(|a| a.product.to_owned()),
            version: self.presets.app.map(|a| a.version.to_owned()),
            comment: self.presets.comment.clone(),
            ..Preset::capture(
                name,
                specs,
                (0..specs.len()).map(|i| self.model.get(ParamIndex(i as u32))),
            )
        }
    }

    /// The current preset's comment, if it has one.
    pub fn preset_comment(&self) -> Option<&str> {
        self.presets.comment.as_deref()
    }

    /// Load `preset`: the engine hears a patch load
    /// ([`EngineEvent::PatchLoad`](silkscreen_bridge::EngineEvent::PatchLoad)), then every param it changes moves as a
    /// gesture, so the host and the engine follow, and the load is one
    /// undo step; params it doesn't name go to their defaults. Becomes the
    /// current preset.
    pub fn load_preset(&mut self, preset: &Preset) {
        self.presets.random_folder = None;
        self.presets.comment.clone_from(&preset.comment);
        let values = preset.values_for(self.model.specs());
        self.mark_patch_load();
        self.presets.loading = true;
        self.edit(|e| {
            e.group(|e| {
                for (i, v) in values.into_iter().enumerate() {
                    let p = ParamIndex(i as u32);
                    if e.get(p).to_bits() != v.to_bits() {
                        e.begin(p);
                        e.set(p, v);
                        e.end(p);
                    }
                }
            });
        });
        self.presets.loading = false;
        self.set_preset_name(Some(preset.name.clone()));
        self.set_preset_modified(false);
    }

    /// Where the current preset lives, if it came from the factory bank
    /// or the store.
    pub fn preset_place(&self) -> Option<&Place> {
        self.presets.place.as_ref()
    }

    pub fn set_preset_place(&mut self, place: Option<Place>) {
        self.presets.place = place;
    }

    /// The preset last loaded or saved, if any.
    pub fn preset_name(&self) -> Option<&str> {
        self.presets.label.name.as_deref()
    }

    pub fn set_preset_name(&mut self, name: Option<String>) {
        self.presets.label.name = name;
    }

    /// Whether the user changed a param since the last load or save
    /// (preset fields mark the name with `*`).
    pub fn preset_modified(&self) -> bool {
        self.presets.label.modified
    }

    pub fn set_preset_modified(&mut self, modified: bool) {
        self.presets.label.modified = modified;
    }

    /// Show `message` briefly in place of preset fields' names ("Saved",
    /// or why not).
    pub fn flash_preset(&mut self, message: &str) {
        self.presets.label.flash = Some(message.to_owned());
    }

    /// The message preset fields are showing, if any.
    pub fn preset_flash(&self) -> Option<&str> {
        self.presets.label.flash.as_deref()
    }

    pub(super) fn clear_preset_flash(&mut self) {
        self.presets.label.flash = None;
    }

    /// Every preset in stepping order: the factory bank's (by category,
    /// then name), then the user's (Uncategorised, then each folder).
    fn preset_order(&self) -> Vec<(Place, String)> {
        let Some(setup) = &self.presets.setup else {
            return Vec::new();
        };
        let factory = setup
            .factory
            .iter()
            .map(|p| (Place::Factory(p.category), p.name.to_owned()));
        let tree = setup.store.borrow().tree();
        let user = tree.into_iter().flat_map(|f| {
            let folder = f.name;
            f.presets
                .into_iter()
                .map(move |n| (Place::User(folder.clone()), n))
        });
        factory.chain(user).collect()
    }

    /// Load the preset `name` in `place`, making it current.
    fn load_preset_from(&mut self, place: &Place, name: &str) -> Result<(), String> {
        let setup = self.presets.setup.as_ref().ok_or("no presets")?;
        let text = match place {
            Place::Factory(c) => setup
                .factory
                .iter()
                .find(|p| p.category == *c && p.name == name)
                .map(|p| p.text.to_owned()),
            Place::User(folder) => setup
                .store
                .borrow()
                .read(&PresetPath::new(folder.as_deref(), name)),
        }
        .ok_or_else(|| format!("\u{201c}{name}\u{201d} has gone"))?;
        let mut preset =
            Preset::parse(&text).map_err(|e| format!("\u{201c}{name}\u{201d}: {e}"))?;
        name.clone_into(&mut preset.name);
        self.load_preset(&preset);
        self.set_preset_place(Some(place.clone()));
        Ok(())
    }

    /// Load the next (or previous) preset in `preset_order`,
    /// wrapping; from no current preset, the first (or last).
    pub fn step_preset(&mut self, forward: bool) -> Result<(), String> {
        let order = self.preset_order();
        if order.is_empty() {
            return Err("No presets yet".into());
        }
        let n = order.len();
        let at = order.iter().position(|(place, name)| {
            Some(place) == self.presets.place.as_ref() && Some(name.as_str()) == self.preset_name()
        });
        let next = match (at, forward) {
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
            (None, true) => 0,
            (None, false) => n - 1,
        };
        let (place, name) = order[next].clone();
        self.load_preset_from(&place, &name)
    }

    /// Every param to its default, as one undo step; no preset is current.
    pub fn init_preset(&mut self) {
        self.load_preset(&Preset {
            name: "Init".to_owned(),
            ..Preset::default()
        });
        self.set_preset_name(None);
        self.set_preset_place(None);
    }

    /// Save the current sound as `name` in user `folder`, making it
    /// current and unmodified.
    pub fn write_user_preset(&mut self, name: &str, folder: Option<&str>) -> Result<(), String> {
        let setup = self.presets.setup.as_ref().ok_or("no presets")?;
        let text = self.capture_preset(name).to_text(self.model.specs());
        setup
            .store
            .borrow_mut()
            .write(&PresetPath::new(folder, name), &text)?;
        self.set_preset_name(Some(name.to_owned()));
        self.set_preset_place(Some(Place::User(folder.map(str::to_owned))));
        self.set_preset_modified(false);
        Ok(())
    }

    /// Save over the current preset if it's the user's: `None` if it
    /// isn't (the caller asks for a name instead).
    pub fn save_preset_in_place(&mut self) -> Option<Result<(), String>> {
        let Some(Place::User(folder)) = self.presets.place.clone() else {
            return None;
        };
        let name = self.presets.label.name.clone()?;
        Some(self.write_user_preset(&name, folder.as_deref()))
    }

    /// Give the editor the host's file services: Export and Import, and
    /// on the web Share (design/presets.md).
    pub fn set_preset_io(&mut self, io: Rc<dyn PresetIo>) {
        self.presets.io = Some(io);
    }

    pub fn preset_io(&self) -> Option<Rc<dyn PresetIo>> {
        self.presets.io.clone()
    }

    /// Give the editor the host's text prompt: name fields ask it for
    /// their text instead of taking keys (design/input.md).
    pub fn set_text_prompt(&mut self, prompt: Rc<dyn TextPrompt>) {
        self.presets.prompt = Some(prompt);
    }

    pub fn text_prompt(&self) -> Option<Rc<dyn TextPrompt>> {
        self.presets.prompt.clone()
    }

    /// The current sound as preset text, named for the current preset
    /// (or "Init").
    pub fn current_preset_text(&self) -> (String, String) {
        let name = self.preset_name().unwrap_or("Init").to_owned();
        let text = self.capture_preset(&name).to_text(self.model.specs());
        (name, text)
    }

    /// Load preset `text` from outside the store (an imported file, a
    /// shared link): it becomes current, saved nowhere, so Save asks for
    /// a name. Refused, saying why, if it is another product's or of a
    /// version this build can't read ([`AppVersion::accepts`], design/presets.md).
    ///
    pub fn import_preset_text(&mut self, text: &str) -> Result<String, String> {
        let preset = Preset::parse(text)?;
        if let Some(app) = self.presets.app {
            app.accepts(preset.product.as_deref(), preset.version.as_deref())?;
        }
        self.load_preset(&preset);
        self.set_preset_place(None);
        Ok(preset.name)
    }

    /// Merge library `text` (design/presets.md) into the user's presets,
    /// loading nothing.
    fn import_library_text(&mut self, text: &str) -> Result<LibraryImport, String> {
        let library = parse_library(text)?;
        if let Some(app) = self.presets.app {
            check_library(&library, app)?;
        }
        let setup = self
            .presets
            .setup
            .as_ref()
            .ok_or("this editor keeps no user presets")?;
        Ok(import_library(
            &mut **setup.store.borrow_mut(),
            &library,
            self.presets.app,
        ))
    }

    /// Every user preset as a library, and how many it holds.
    pub fn export_library_text(&self) -> Option<(String, usize)> {
        let setup = self.presets.setup.as_ref()?;
        Some(export_library(&**setup.store.borrow(), self.presets.app))
    }

    /// Import a file's `text`, a library or a preset, and say how it went
    /// in preset fields.
    pub(super) fn import_and_flash(&mut self, text: &str) {
        let message = if is_library(text) {
            match self.import_library_text(text) {
                Ok(report) => report.summary(),
                Err(e) => import_failed(e),
            }
        } else {
            match self.import_preset_text(text) {
                Ok(name) => format!("Imported \u{201c}{name}\u{201d}"),
                Err(e) => import_failed(e),
            }
        };
        self.flash_preset(&message);
    }

    /// Say how a host's file service went (design/presets.md).
    pub(super) fn handle_io(&mut self, event: IoEvent) {
        match event {
            IoEvent::Imported(text) => self.import_and_flash(&text),
            IoEvent::Exported(name) => {
                self.flash_preset(&format!("Exported \u{201c}{name}\u{201d}"));
            }
            IoEvent::Failed(e) => self.flash_preset(&e),
        }
    }

    /// Have `f` build random sounds (design/presets.md): it gets the
    /// generator, the values before and the defaults, and returns every
    /// value, a name and the user folder Save As offers.
    pub fn set_randomiser(
        &mut self,
        f: impl Fn(&mut Rng, &[f32], &[f32]) -> RandomSound + 'static,
    ) {
        self.presets.randomiser = Some(Box::new(f));
    }

    /// The random sound from `seed`: the plugin's randomiser's, else
    /// every param uniformly random ([`random::uniform`](crate::random::uniform)),
    /// named "Random". Its comment ends with the seed ("Random Pad, seed
    /// 3F2AK"), which the name field's tooltip shows, so the same sound
    /// can be made again (design/presets.md). Loaded as one undo step and saved
    /// nowhere.
    pub fn randomise_preset_seeded(&mut self, seed: Seed) {
        let rng = &mut seed.rng();
        let specs = self.model.specs();
        let sound = match &self.presets.randomiser {
            Some(f) => {
                let before: Vec<f32> = self.param_values().collect();
                let defaults: Vec<f32> = specs.iter().map(|s| s.default).collect();
                f(rng, &before, &defaults)
            }
            None => RandomSound {
                values: crate::random::uniform(rng, specs),
                name: "Random".to_owned(),
                folder: None,
                comment: None,
            },
        };
        debug_assert_eq!(
            sound.values.len(),
            specs.len(),
            "a randomiser returns one value per param"
        );
        let comment = match sound.comment {
            Some(c) => format!("{c}, seed {seed}"),
            None => format!("Random, seed {seed}"),
        };
        let preset = Preset {
            name: sound.name,
            comment: Some(comment),
            values: specs.iter().map(|s| s.id).zip(sound.values).collect(),
            ..Preset::default()
        };
        self.load_preset(&preset);
        self.set_preset_place(None);
        self.presets.random_folder = sound.folder;
    }

    /// The user folder Save As offers for the current random sound, if
    /// it is one and its randomiser named one (design/presets.md).
    pub fn random_folder(&self) -> Option<&str> {
        self.presets.random_folder.as_deref()
    }

    /// Tell the editor what kind of sound each seed gives (design/presets.md),
    /// from the seed alone: a press whose kind repeats the last one's is
    /// redrawn half the time, so repeated presses vary.
    pub fn set_random_kind(&mut self, kind: impl Fn(Seed) -> usize + 'static) {
        self.presets.random_kind = Some(Box::new(kind));
    }

    /// A random sound, its seed from the editor's own generator, redrawn
    /// half the time when its kind repeats the last press's.
    pub fn randomise_preset(&mut self) {
        let mut rng = Rng::new(self.presets.random_seed);
        self.presets.random_seed = rng.next_u64();
        let mut seed = Seed::from_bits(rng.next_u64());
        if let Some(kind) = &self.presets.random_kind {
            if Some(kind(seed)) == self.presets.last_random_kind && rng.chance(0.5) {
                seed = Seed::from_bits(rng.next_u64());
            }
            self.presets.last_random_kind = Some(kind(seed));
        }
        self.randomise_preset_seeded(seed);
    }

    /// Seed the editor's generator (the view does, from its clock).
    pub fn seed_random(&mut self, seed: u64) {
        self.presets.random_seed ^= seed;
    }

    /// A store shared with the preset browser and dialogs, and the bank.
    pub(crate) fn preset_setup(&self) -> Option<(Box<dyn PresetStore>, &'static [FactoryPreset])> {
        let setup = self.presets.setup.as_ref()?;
        Some((
            Box::new(SharedStore(Rc::clone(&setup.store))),
            setup.factory,
        ))
    }

    /// Give the editor `store` for the user's presets and the read-only
    /// `factory` presets (a bank `silkscreen-build` generates, design/presets.md),
    /// for its preset fields' browser.
    /// Mark every preset and library this editor writes (saved, exported,
    /// shared, kept as the session) as `app`'s (design/presets.md), so a later
    /// version can tell what needs upgrading. Each plugin's editor state
    /// does this from its [`PluginUi`](crate::PluginUi).
    pub fn set_app(&mut self, app: AppVersion) {
        self.presets.app = Some(app);
    }

    /// The plugin and version presets are marked with, if set.
    pub fn app(&self) -> Option<AppVersion> {
        self.presets.app
    }

    pub fn set_presets(&mut self, store: Box<dyn PresetStore>, factory: &'static [FactoryPreset]) {
        self.presets.setup = Some(PresetSetup {
            store: Rc::new(RefCell::new(store)),
            factory,
        });
    }

    /// What preset fields show.
    pub(crate) fn preset_text(&self) -> &FieldText {
        &self.presets.label
    }
}

/// The preset browser (design/presets.md), centred over the faceplate, if the
/// plugin set presets up.
pub(super) fn browser_for(state: &EditorState) -> Option<PresetBrowser> {
    let setup = state.presets.setup.as_ref()?;
    Some(PresetBrowser::new(
        PresetBrowser::centred(state.layout.size),
        Box::new(SharedStore(Rc::clone(&setup.store))),
        setup.factory,
        state,
    ))
}

/// Import dropped preset and library files, if `payload` holds any:
/// paths ending `.preset` or `.presetbank` (native hosts, read here), or
/// browser files the host's [`PresetIo`] takes. True if the drop was
/// theirs.
pub(super) fn import_drop(state: &mut EditorState, payload: &silkscreen_core::DropPayload) -> bool {
    use silkscreen_core::DropPayload;
    match payload {
        DropPayload::Paths(paths) => {
            let mut any = false;
            for path in paths.iter().filter(|p| {
                p.file_name()
                    .is_some_and(|n| is_import_file(&n.to_string_lossy()))
            }) {
                any = true;
                match std::fs::read_to_string(path) {
                    Ok(text) => state.import_and_flash(&text),
                    Err(e) => state.flash_preset(&import_failed(e)),
                }
            }
            any
        }
        DropPayload::Files(handles) => state
            .preset_io()
            .is_some_and(|io| io.import_dropped(handles)),
    }
}
