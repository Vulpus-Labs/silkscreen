# Licensing hook

`silkscreen-clap` can keep a plugin locked until the user enters a licence
code. It provides the scheme-neutral parts: where the code is saved, the
"Unregistered" strip and unlock dialog, the soft lock while locked, and
unlocking every instance at once. **silkscreen ships no code format and no
code issuing.** The plugin brings its own check: a function that says
whether a code unlocks it. The check could verify a signature, look up a
hash, or anything else.

The lock exists only in the CLAP host (see [hosts.md](hosts.md)). Web pages
and standalone apps are never locked.

## Opting in

`SilkscreenPlugin` has an associated constant:

```rust
pub type CheckCode = fn(&str) -> Result<(), String>;

const LICENCE: Option<CheckCode> = None;
```

`None`, the default, means the plugin is never locked and its engine output
is bit for bit the plain engine's. To opt in, give it a check:

```rust
fn check(code: &str) -> Result<(), String> {
    let code = code.trim();
    if my_scheme::verify(code) {
        Ok(())
    } else {
        Err("That isn't a licence code for this plugin.".into())
    }
}

impl SilkscreenPlugin for MyPlugin {
    // ...
    const LICENCE: Option<silkscreen_clap::CheckCode> = Some(check);
}
```

The check's contract:

- It receives the code as typed, pasted or read from the saved file,
  surrounding spaces and newlines included. Normalise as your scheme needs.
- `Ok(())` unlocks the plugin. `Err(reason)` refuses the code. The reason is
  shown in the unlock dialog as written, so make it a sentence for the user,
  for example "That code is for a different product."
- It runs on the main thread only, never on the audio thread, so it may
  allocate and take its time.

`examples/gain-plugin/tests/licence.rs` shows a stand-in check (`GAIN-<n>`
unlocks, `OTHER-<n>` is "for a different product") and tests the whole lock
through the test host.

## The saved code

A good code is saved as `licence.txt` (the constant `LICENCE_FILE`). By
default it sits in the plugin's user-preset folder, named by `PluginUi`'s
`vendor` and `product` (see [presets.md](presets.md)):

| Platform | Default location |
|---|---|
| macOS | `~/Library/Audio/Presets/<vendor>/<product>/licence.txt` |
| Windows | `%APPDATA%\<vendor>\<product>\Presets\licence.txt` |
| Linux | `$XDG_DATA_HOME/<product>/presets/licence.txt` (or `~/.local/share/…`) |

Override `SilkscreenPlugin::licence_file()` to put it elsewhere. Returning
`None` means there is nowhere to save. The code is per user, not per
project, so every project on the machine opens unlocked once it has been
entered.

The file is read and checked once, on the main thread, when the first
instance of the plugin is created. A missing, unreadable or refused file
leaves the plugin locked but never stops it loading. A refused file's reason
is printed to stderr. Saving writes through a temporary file and renames it,
so a failed write never leaves half a code behind. The saved text is the
trimmed code and a newline.

## One flag per plugin id

Whether a plugin is unlocked is a process-wide `AtomicBool`, one per plugin
`ID`, found by id on the main thread when an instance is created. Every
instance of the plugin in the host process shares it, so unlocking one
unlocks all of them. Each open editor sees the change on its next tick and
hides its strip. The audio thread only does a relaxed load of the flag.
Different plugins, even from the same vendor, have separate flags and
separate `licence.txt` files, so unlocking one leaves the others locked.

## The soft lock: `Gated`

While locked, the plugin is fully usable but awkward to release music with.
The shell wraps the engine in `Gated`, which every 60 s of processed audio
fades the output to silence, holds silence for 1.5 s, and fades back in:

| Constant | Value | Meaning |
|---|---|---|
| `GAP_PERIOD_S` | 60.0 | Seconds of audio between gaps. The first gap starts after one full period. |
| `GAP_FADE_S` | 0.010 | Each raised-cosine fade, out and in. |
| `GAP_HOLD_S` | 1.5 | Silence between the fades. |

The details:

- **Time is counted in samples** since activation, so the gate needs only a
  counter and the flag load. It doesn't allocate or take a lock, and blocks
  that touch no gap pass through untouched.
- **The engine keeps running underneath.** Only its output is scaled, so
  nothing jumps when the sound returns.
- **Telemetry follows the gate.** The engine's scope samples and meters are
  scaled to match, so the editor shows what the host hears (see
  [overlays-and-text.md](overlays-and-text.md) for scopes and meters).
- **Unlocking mid-gap** lets a gap already under way finish, so the sound
  doesn't jump back, and then no new gap starts.
- **The lock never touches parameters, presets or saved state.** A project
  saved while locked is byte for byte what an unlicensed build would save,
  and it opens unchanged once the plugin is unlocked.

## The strip and the unlock dialog

The editor itself knows nothing of plugins or codes. The host gives the
`EditorState` a `silkscreen::Licence`:

```rust
pub trait Licence {
    fn locked(&self) -> bool;                          // asked every tick: keep it cheap
    fn unlock(&self, code: &str) -> Result<(), String>;
}
```

With `LICENCE` set, the CLAP shell installs one backed by the plugin's flag
(`EditorState::set_licence`). While `locked()` is true, the framework draws
a small strip, "Unregistered — Unlock…" (`silkscreen::licence::STRIP_TEXT`), in the
faceplate's bottom-right corner. It mirrors the version badge in the
opposite corner, is drawn in the theme's lettering and colours, and is not
part of the faceplate art. It is an overlay below any open panel.

Clicking the strip opens the `silkscreen::Unlock` dialog: a "Licence Code"
field with Paste beside it, and Cancel and Unlock buttons. Enter unlocks and
Esc cancels.

- **Paste** reads the host's `silkscreen::Clipboard`. The native host
  provides one on macOS and Windows, which also gives text fields
  Cmd/Ctrl-V. With no clipboard text, the dialog says so.
- **In a DAW** a click on the field opens the platform's text prompt,
  because the DAW may take some keys first. Enter in the prompt unlocks.
- **A refused code** stays in the field, with the check's reason shown
  under it.
- **A good code** closes the dialog, and the strip disappears from every
  open editor of the plugin.

There is no "licensed to" line. The hook carries no owner information, and
a scheme that wants one has to show it itself.

## Calling it directly

The same operations are public for plugins that want their own UI or
tests:

- `silkscreen_clap::is_unlocked::<P>() -> bool` is always `true` for a
  plugin without `LICENCE`.
- `silkscreen_clap::try_unlock::<P>(code) -> Result<(), UnlockError>` checks
  the code. A good code sets the flag (unlocking every instance) and saves
  the trimmed code to `licence_file()`. Main thread only.

`UnlockError` says why `try_unlock` did not unlock for good:

| Variant | Meaning |
|---|---|
| `Invalid(String)` | The check refused the code, for the reason given. Still locked. |
| `NotSaved(String)` | The code is good and the plugin is unlocked for this session, but saving failed (or there is no user folder), so it will be locked again next time. |
| `NotLicensed` | The plugin has no `LICENCE`. |

Its `Display` text is what the dialog shows. `Invalid` shows the check's
own sentence. `NotSaved` shows "Unlocked, but the code could not be saved
(…). Enter it again next time."

## What the hook doesn't do

There is no server, activation or machine binding, and nothing counts
installs. A user who reinstalls pastes the same code again. Anyone who
patches the binary can bypass the flag or the gate. The hook is meant to
keep honest users honest. How codes are made, sold, revoked or tied to a
product is entirely up to the plugin's check.
