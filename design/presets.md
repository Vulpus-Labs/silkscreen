# Presets and random sounds

A preset is a whole sound: one value for every parameter, keyed by each
param's stable id, with a little metadata. Presets carry values only,
never UI state. This document covers the text format, where presets are
stored, factory banks, the preset bar and browser, moving presets in and
out (files, libraries, share links), the version check on import, and
the framework for random sounds.

The format lives in `silkscreen_model::preset`. Stores, the browser,
libraries and random sounds are in the `silkscreen` crate; the native
folder store is in `silkscreen-host-native`; the web store and page are
in `silkscreen-host-web`. `examples/widgets` has a factory bank and a
preset bar to read alongside this.

## The text format

One preset per file, `<name>.preset`, as human-readable text:

```text
# silkscreen preset
format = 1
product = Tone
version = 0.1.0
name = Squelchy Bass
category = Bass
author = Someone
comment = A short line about the sound
p1 = 0.8  # Level
p3 = 2  # Wave: Pulse
```

- `p<id> = <value>` lines hold plain values, keyed by `ParamSpec::id`,
  so presets survive a reordered param table (see [model.md](model.md)).
  `Preset::to_text` comments each line with the param's name, and with
  the position's label for discrete params. Comments are for people and
  are ignored when read.
- **Every param is written**, not only those away from their defaults.
  A sparse file would change its sound if a default ever changed.
- Reading is forgiving about content: params a preset doesn't mention
  load at their defaults, values are clamped into range, and ids the
  table doesn't have are ignored (a preset from a later version, say).
  `Preset::values_for(specs)` gives the plain values in table order.
- Reading is strict about form: `Preset::parse` refuses a line that isn't
  `key = value`, an unknown key, a value that isn't a number, or a
  missing `name`, naming the line.
- **`format`** versions the layout. A file without it reads as format 1;
  a format this build doesn't know (`preset::FORMAT` is 1) is refused
  with an error naming it.
- **`product` and `version`** say which plugin, at which version, wrote
  the preset (see [Versions](#versions)).
- **`name`, `category`, `author`, `comment`** take the rest of their
  line as is, `#` included. `category` is for factory presets and must
  match the preset's folder. Metadata with line breaks is written on one
  line.

The editor keeps the current preset's comment: the name field's tooltip
shows it, and saving keeps it. Save As asks only for a name and a
folder; the other fields are for people who edit the files.

## Factory banks

A plugin keeps its factory presets in
`presets/factory/<Category>/<Name>.preset`. `silkscreen-build` embeds
them as a generated table, sorted by category and then name:

```rust
// build.rs
silkscreen_build::FactoryBank::new("presets/factory").build();

// src/lib.rs
include!(concat!(env!("OUT_DIR"), "/factory_presets.rs"));
// pub static FACTORY: &[silkscreen::FactoryPreset] = &[...];
```

Each `FactoryPreset { category, name, text }` is read-only; the browser
offers no edit actions on them.

The build fails, naming the file (and the line, where there is one), if
a preset doesn't parse, has a `name` other than its file's, has a
`category` other than its folder's, sits outside a category folder or in
a nested one, or repeats another preset's name. A build script can't see
the plugin's param table, so the plugin's own tests check the rest with
`silkscreen_harness::assert_factory_bank(FACTORY, &PARAMS)`: unknown param
ids, out-of-range values, and a round trip through text, as well as
repeating the build's checks. The same checks are available without the
harness as `silkscreen::model::preset::check_factory`.

## User presets and stores

User presets live in a `PresetStore`, addressed by
`PresetPath { folder: Option<String>, name }`.

- Folders are one level deep. `folder: None` is the root, shown in the
  browser as "Uncategorised" (`presets::UNCATEGORISED`), a name no
  folder may take. Empty folders are kept.
- Names pass `presets::valid_name`: trimmed, non-empty, no `/`, `\` or
  `:`, no control characters and no leading dot. Folder names pass
  `valid_folder_name`, which adds the "Uncategorised" rule.
- The trait's required methods are `tree` (the root first, then every
  folder, each with its presets, in case-insensitive order), `read`,
  `write` (create or replace, making a missing folder), `remove`,
  `add_folder`, `rename_folder` and `remove_folder` (with its
  contents). Provided methods build `exists`, `rename`, `move_to` and
  `create_folder` on those; rename and move refuse a target that exists,
  and `create_folder` picks a free "New Folder", "New Folder 2", and so
  on. `problem` lets a store report why it can't keep presets right now,
  for the browser's status line.

Implementations:

- **`MemoryStore`** (`silkscreen`): a map, for tests and for plugins
  with a factory bank but no store of their own.
- **`DirStore`** (`silkscreen-host-native`): a folder of `.preset`
  files, subfolders as folders. Every mutating call resolves its path and
  refuses anything outside the store's folder, which rules out `..` and
  symlink escapes; symlinked subfolders aren't listed.
  `DirStore::default_for(vendor, plugin)` uses the platform's usual
  place:
  - macOS: `~/Library/Audio/Presets/<vendor>/<plugin>`
  - Windows: `%APPDATA%\<vendor>\<plugin>\Presets`
  - Linux and others: `$XDG_DATA_HOME/<plugin>/presets`, or
    `~/.local/share/<plugin>/presets`
- **`JournaledStore`** (`silkscreen-host-web`): the web store, below.

### Wiring

A plugin describes its editor side once, as a `silkscreen::PluginUi`:
`vendor`, `product`, `version`, `web_prefix`, `specs`, the `factory`
bank, the faceplate and a `state` constructor. Every host reads it. The
`state` constructor builds an `EditorState` without presets; each host
then calls `EditorState::set_app(ui.app())` and
`EditorState::set_presets(store, ui.factory)` with its own store.

A CLAP plugin opts into a `DirStore` under its vendor and product with
`SilkscreenPlugin::USER_PRESETS = true`, or overrides
`SilkscreenPlugin::preset_store()` to return any store. With no store
and a non-empty factory bank, the CLAP layer keeps user presets in a
`MemoryStore` for the session. See [hosts.md](hosts.md).

## The preset bar

A faceplate places preset controls wherever it likes (see
[faceplates.md](faceplates.md)):

- **The `preset.<name>` field**, bound with `PresetButton`, shows the
  current preset's name, "Init" before anything is loaded, and a `*`
  after the name once a param changes after a load or save. A click on
  the name opens the browser; a click on the ‹ or › at its ends steps to
  the previous or next preset. Stepping runs through the factory bank
  (by category, then name) and then the user's presets (Uncategorised,
  then each folder), wrapping at the ends.
- **`action.<name>` push areas**, bound with `PresetCommand::save()`,
  `save_as()`, `init()` or `randomise()`, or `PresetCommand::new` with a
  `PresetAction` (`Previous`, `Next`, `Browse` as well).

What the actions do:

- **Save** overwrites the current preset if it is the user's, and
  otherwise behaves as Save As.
- **Save As** is a dialog with a name field and a folder drop-down. It
  opens on the current preset's folder if that is a user folder, else on
  a random sound's suggested folder (offered even before it exists, and
  made on save), else Uncategorised. Saving onto a taken name asks
  "Replace?" first.
- **Init** sets every param to its default as one undo step, and leaves
  no preset current.

Errors and confirmations show in the browser's status line, or, with
the browser closed, briefly in place of the field's name
(`EditorState::flash_preset`).

Loading a preset (`EditorState::load_preset`) sends the engine a patch
load mark, then moves each param that changes as its own gesture, so the
host and the engine follow, all grouped as one undo step (see
[engine-bridge.md](engine-bridge.md) and [model.md](model.md)). Changes
made while loading don't set the modified mark.

The current preset is saved in the editor's UI state, which the host
keeps with the project: `preset.name`, `preset.comment`, `preset.place`
(`factory:<category>`, `user:` for Uncategorised, or `user:<folder>`)
and `preset.modified` (`1`), each present only when set. A reopened
project shows its preset's name and mark. Presets themselves carry no
UI state.

## The browser

`PresetBrowser` is a modal panel centred over the faceplate.

- **Header:** a search field and a close ×. While the search has text,
  the right pane lists matching presets from every place (a
  case-insensitive substring of the name), each labelled with where it
  lives ("Factory · Bass", "User · Leads").
- **Left pane:** FACTORY and its categories, then USER: Uncategorised,
  the folders, and "+ New Folder", which makes a folder and opens a name
  field on it.
- **Right pane:** the chosen place's presets. A click, or Enter on the
  selection, loads one; the current preset is highlighted. The browser
  opens on the current preset's place.
- **Edits**, for user presets and folders only: right-click, or the ⋯
  on a hovered row (for hosts and touch screens without a right button),
  opens Rename, Move to ▸ and Delete for a preset, or Rename and Delete
  for a folder. Deletes ask first; deleting a folder says how many
  presets go with it. A user preset dragged onto a user folder (or
  Uncategorised) moves there, and the browser follows it.
- **Footer:** Random, Init, Save and Save As…, and the status line.
  Where the host provides file services, Export, Import and (on the web)
  Share come first.
- **Keys:** Up, Down, Page Up, Page Down and Enter work the list; other
  keys type into the search. Esc closes the top layer: a dialog, then a
  menu, then the browser.

Overlays in general are covered in [overlays-and-text.md](overlays-and-text.md).

## Export, import and libraries

A host's file services implement `PresetIo` and are given to the editor
with `EditorState::set_preset_io`:

- `export(file_name, text)` offers a file to save (a download, a Save
  panel).
- `import()` asks the user for a file.
- `import_dropped(handles)` takes preset and library files dropped on
  the editor (the web host's file handles). Native hosts read dropped
  paths ending `.preset` or `.presetbank` directly.
- `can_share()` and `share(text)` make a link that loads a preset.
- `take_event()` returns the next outcome.

Files are chosen and read or written asynchronously, so outcomes arrive
as `IoEvent`s (`Imported(text)`, `Exported(file name)`, `Failed(reason)`)
that the editor drains each tick and flashes. A cancelled dialog reports
nothing.

- **Native** (`NativePresetIo` in `silkscreen-host-native`, installed
  whenever the host opens an editor window) uses `rfd`'s async dialogs
  parented to the editor's window. On macOS that is a sheet begun with a
  completion handler, so no nested modal run loop re-enters the editor
  from a DAW callback; Windows and Linux run the dialog on `rfd`'s own
  thread. The view polls the dialog's future each tick with a no-op
  waker.
- **Web** (`WebPresetIo`): Export downloads a file, Import opens a file
  picker, and Share copies a `#patch=` link.

**Export** is a menu: "This preset…" writes `<name>.preset`, and "All
user presets…" writes a library. Factory presets are never exported.
**Import** takes either kind and tells them apart by content, not
extension.

### The library file

A `.presetbank` holds a user's whole library, to move it between the
web, a plugin, or two machines:

```text
# silkscreen preset library
library = 1
product = Tone
version = 0.1.0

[Leads/Bright Saw]
# silkscreen preset
format = 1
...

[Init Copy]
...

[Empty Folder/]
```

- Text whose first non-blank line is `library::HEADER` is a library;
  anything else is a single preset (`library::is_library`).
- `library = 1` versions the wrapper (`library::VERSION`); an unknown
  version is refused, naming it.
- Each `[folder/name]` (or `[name]` at the root) heads a section whose
  body is the preset's text verbatim. `[folder/]` keeps an empty folder.
  Sections come in `tree()` order. A valid preset never has a line
  starting with `[`, so sections can't be confused with their bodies.
- The header carries the exporting plugin's product and version; each
  section keeps the version it was saved with.

**Importing a single preset** loads it as the current sound, saved
nowhere, so Save asks for a name. **Importing a library** merges into
the user's presets and loads nothing:

- Folders match existing ones ignoring case (the existing spelling
  wins); missing ones are created.
- A name clash keeps both: the imported preset takes the first free
  "Name 2", "Name 3", … in its folder, compared ignoring case, as macOS
  and Windows file systems do.
- A section that doesn't parse, isn't validly named, or fails the
  version check is skipped; the rest still import.
- The status line reports the outcome, for example "Imported 12 presets
  (3 renamed, 1 skipped: …)", with the first skip's reason.

### Versions

Every preset and library the editor writes (saved, exported, shared, or
kept as the web session) carries the plugin's `product` and `version`,
as given by `EditorState::set_app(AppVersion { product, version })`.
`PluginUi::app()` builds it from the plugin's `product` and `version`
(usually `env!("CARGO_PKG_VERSION")`), the same version the CLAP
descriptor reports.

Import refuses what this build can't read, through
`AppVersion::accepts(product, version)`:

- another product's preset (compared ignoring case);
- a version whose major or minor number differs from this build's,
  older or newer. Patch versions move freely: a patch release must not
  change what a preset means, and any release that does is at least a
  minor release.

A file with no version counts as `UNVERSIONED` ("0.1.0"). The error
names both sides: "it was made by Tone 0.3.0; this is 0.2.1, which reads
0.2.x". A library is refused whole by its header, and a section by its
own version. The check applies to imported and dropped files, share
links, and the restored web session; it does not apply to loading
presets from the user's own store or the factory bank. When a release
changes what presets mean, it is that release's job to add an upgrade
path from the versions it can convert, beside `accepts`.

## Presets on the web

`silkscreen_host_web::PresetPage::start(canvas, plugin_ui)` sets up
everything below for a plugin's page from its `PluginUi`. A plugin's
page is its `#[wasm_bindgen]` surface over one, and `PresetPage` also
carries the console methods used for hand checks.

- **Storage:** user presets live in IndexedDB, database
  `<web_prefix>-presets`, with stores `presets` (preset text keyed by
  `name` or `folder/name`, the same text as a native file), `folders`
  (including empty ones) and `state` (the session). The page asks the
  browser to persist the database so it isn't evicted under storage
  pressure.
- **A synchronous cache:** the editor needs answers immediately, so the
  page hydrates a `JournaledStore` (a `MemoryStore` plus a journal of
  touched keys) from the database before the editor starts. Changes go
  to the cache and the journal; the journal is flushed after each change
  and when the page is hidden (`visibilitychange`). Several changes to
  one key write once, and a rename is a put and a delete. A failed flush
  requeues its keys and shows in the browser's status line. A tab killed
  mid-flush can lose the last write; flushing on `visibilitychange`
  keeps that window small. If the database can't be opened, the store
  works in memory only.
- **Session:** the current sound (as preset text) and the editor's UI
  state are autosaved to `state` under `session`, at most every 400 ms
  while they change and when the page is hidden, and restored on load.
- **Share links:** a `#patch=<base64url of the preset text>` fragment
  wins over the session, and is stripped from the address after boot. A
  link that can't be read falls back to the session and says why in the
  preset field.
- The engine starts from the restored sound, so there is no glide from
  the defaults on load.

## Random sounds

The browser's Random button and `PresetCommand::randomise()` ask the
editor for a random sound (`EditorState::randomise_preset`).

### Seeds

Every random sound has a `random::Seed`: 25 bits, written as five
Crockford base-32 characters ("3F2AK"). `Seed::parse` accepts either
case and reads O as 0 and I or L as 1. The sound depends only on its
seed (and, if the plugin's randomiser says so, on values kept from
before, such as output levels). The editor writes the seed into the
sound's comment, "Random Pad, seed 3F2AK", which the name field's
tooltip shows and a saved preset keeps, so a sound can be reported and
made again with `EditorState::randomise_preset_seeded(seed)`.

The editor draws seeds from its own generator, which the view seeds
from its clock; nothing reads OS entropy or a wall clock, so this builds
for wasm. `random::Rng` is a small xorshift64* generator (seeded through
splitmix64) with `unit`, `below`, `range`, `chance` and weighted `pick`.

### The randomiser hook

A plugin's randomiser builds the whole sound:

```rust
state.set_randomiser(|rng, before, defaults| RandomSound {
    values: /* one plain value per param, in table order */,
    name: random::names::name(rng),
    folder: Some("Pads".into()),
    comment: Some("Random Pad".into()),
});
```

It gets the seed's generator, the values before the press and every
param's default, and returns a `RandomSound`: every value, a name, the
user folder Save As should offer, and a comment to which the editor
appends the seed. A sound built from a narrowed space of useful
combinations reads much better than uniform noise over every param, and
starting from `defaults` leaves anything the randomiser doesn't touch
at its Init value.

Without a randomiser the editor uses `random::uniform`: every param
uniformly random over its range (on a whole position for discrete
kinds), named "Random".

`EditorState::set_random_kind(f)` lets a plugin say what kind of sound a
seed gives (an index from the seed alone). When a press draws a seed of
the same kind as the last press, the editor redraws once with chance ½,
so repeated presses vary. Because this happens while drawing seeds, a
seed alone still reproduces its sound.

A random sound is loaded like any preset: one undo step, preceded by a
patch load mark, and saved nowhere until the user saves it.

### Names

`random::names::name(rng)` returns "[adjective] [noun]", such as
"Vexatious Artichoke", from two original word lists: `ADJECTIVES`, from
the plain to the recondite, and `NOUNS`, tending to the everyday. Each
word is one plain, title-cased word, and neither list repeats a word.
Drawing the name after the sound's values means a name never changes
the sound. A plugin that uses these names should test that every pair
fits its preset field whole: `presets::name_fits(rect, name, scale)`
measures a name as the field draws it, between its arrows and with room
for the modified mark.
