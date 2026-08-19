# Writing a plugin

From `cargo new` to a key that does something. The example below is
complete — it was built, installed, and loaded on a Stream Deck Original
exactly as shown.

What the individual functions guarantee is described in
[`plugin-api.md`](plugin-api.md); this document is the path there.

---

## Prerequisites

- **Rust** in the version named by `rust-version` (today 1.97).
  `cargo-component` and `wasm-tools` are **not** needed: Rust turns
  `wasm32-wasip2` straight into a component.
- **The `wasm32-wasip2` target.** Inside the main deckd project itself it is
  already set in `rust-toolchain.toml`, and so is there for every `cargo`
  invocation. A plugin built *outside* it needs this once:

  ```bash
  rustup target add wasm32-wasip2
  ```

  Or — more reliable, because it isn't tied to whatever toolchain happens to
  be active right now — a `rust-toolchain.toml` of the plugin's own, next to
  it:

  ```toml
  [toolchain]
  channel = "stable"
  targets = ["wasm32-wasip2"]
  ```
- **A running `deckd`.** It starts without a device too; a plugin only gets
  loaded once a key that uses it becomes visible.

---

## Step 1: Create the crate

```bash
cargo new --lib counter
cd counter
```

## Step 2: `Cargo.toml`

```toml
[package]
name    = "counter"
version = "1.0.0"
edition = "2024"

# This is the line that turns a library into a loadable module.
[lib]
crate-type = ["cdylib"]

[dependencies]
deck-plugin-sdk = { git = "https://github.com/SimixHD/deckd-plugins", tag = "sdk-v0.1.0" }
serde           = { version = "1", features = ["derive"] }

# Plugins travel through the marketplace; size matters more than the last
# few percent of performance.
[profile.release]
opt-level     = "s"
lto           = true
codegen-units = 1
strip         = true
```

There is one thing you'd otherwise get stuck on: **`crate-type =
["cdylib"]`.** Without it you get an `.rlib`, and never a `.wasm`.

## Step 3: `plugin.toml`

The manifest sits **next to** `Cargo.toml`, not inside `src/`. It says what
the plugin is called, what it can do, and what it's allowed to reach in
order to do it.

```toml
id      = "org.example.counter"
name    = "Counter"
version = "1.0.0"
api     = 1
author  = "Your Name"
license = "MIT OR Apache-2.0"

# No `[capabilities]` block: this plugin doesn't launch anything, doesn't
# read a file, and doesn't talk to anyone on the network. If the block is
# missing, every capability is denied — there is no value that means
# "allow everything".

[[actions]]
id          = "count"
name        = "Counter"
description = "Counts every press; a long press resets it to zero."
long_press  = true
fields = [
  { key = "count", type = "num", label = "Value", default = 0, min = 0 },
]

# Optional: the German sentences, keyed by the English source sentence.
[i18n.de]
"Counter" = "Zähler"
"Counts every press; a long press resets it to zero." = "Zählt jeden Druck; ein langer Druck setzt auf null zurück."
"Value" = "Stand"
```

The `id` is a reverse domain name **and, at the same time, the directory
name** the plugin is later installed under. If the two diverge, the plugin
is rejected. The complete schema description is in
[`plugin-api.md`](plugin-api.md) §5.

**Write the visible fields in English; the translations go below them.**
The reason is the same one that keeps a `.po` file's `msgid` in English:
whatever is missing from `[i18n.<language>]` falls back to the value in the
field — sentence by sentence, never with a gap and never with a placeholder.
So the value in the field has to be the language the most people can read.
Anyone who doesn't translate anything just leaves out `[i18n]`; a plugin
without the table is fully valid and shows its own words everywhere.

Everything a person sees belongs in the translation: the plugin's `name`,
each action's `name` and `description`, every `label` — and the **values of
a fixed `options` list**, since those are what appears in the selection
field. The values themselves stay untouched: they end up in the settings of
every key, and translating them would rewrite other people's profiles. The
rules and the lookup order are in [`plugin-api.md`](plugin-api.md) §5 under
"Translations".

## Step 4: `src/lib.rs`

A plugin is an implementation of the `Guest` trait, generated from the WIT
interface definition (see [`plugin-api.md`](plugin-api.md) §2 for the full
definition). **All nine functions have to be there**, even the empty ones —
the ABI is complete, or it isn't there at all.

```rust
//! A counter on a key: every press increments it, a long press resets it to
//! zero.

use deck_plugin_sdk::{KeyRef, LogLevel, OptionItem, host};

/// What a key of this plugin stores.
///
/// Exactly the keys that `plugin.toml` offers as fields — the profile holds
/// one such object per key, and it arrives here unchanged.
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct Settings {
    count: u32,
}

struct Counter;

impl deck_plugin_sdk::exports::streamdeck::plugin::guest::Guest for Counter {
    /// Runs once, before any key is reported.
    fn init() -> Result<(), String> {
        Ok(())
    }

    /// The key has become visible — the first moment it can display
    /// anything.
    fn on_appear(key: KeyRef) {
        show(&key);
    }

    /// Nothing to shut down: this plugin has no timers.
    fn on_disappear(_key: KeyRef) {}

    fn on_key_down(key: KeyRef) {
        let mut settings = match read(&key) {
            Some(settings) => settings,
            None => return,
        };
        settings.count = settings.count.saturating_add(1);
        store(&key, &settings);
    }

    fn on_key_up(_key: KeyRef) {}

    /// Back to zero. A long press arrives **in addition to**, between
    /// `on-key-down` and `on-key-up`.
    fn on_long_press(key: KeyRef) {
        store(&key, &Settings { count: 0 });
    }

    fn on_timer(_id: u32) {}

    /// This — and only this — is where drawing happens.
    ///
    /// `set-settings` is a request, not a transfer of ownership: only once
    /// the daemon has written it into the profile does it come back as
    /// `on-settings-changed`. A plugin that sets the title already in
    /// `on-key-down` may display a number that never actually ends up on
    /// disk.
    fn on_settings_changed(key: KeyRef) {
        show(&key);
    }

    /// No dynamic source; no field refers to one.
    fn list_options(_source: String) -> Vec<OptionItem> {
        Vec::new()
    }
}

/// The key's settings, or a reported failure.
fn read(key: &KeyRef) -> Option<Settings> {
    match deck_plugin_sdk::settings(key) {
        Ok(settings) => Some(settings),
        Err(why) => {
            // Hand-written profiles exist; an `unwrap` here would be a
            // trap, and a trap is a violation.
            host::show_alert(key);
            host::log(
                LogLevel::Error,
                &format!("key {}: settings unreadable: {why}", key.index),
            );
            None
        }
    }
}

/// Request the new state.
fn store(key: &KeyRef, settings: &Settings) {
    if let Err(why) = deck_plugin_sdk::set_settings(key, settings) {
        host::log(LogLevel::Error, &format!("could not save state: {why}"));
    }
}

/// Write the stored state onto the key.
fn show(key: &KeyRef) {
    let count = deck_plugin_sdk::settings::<Settings>(key)
        .map(|settings| settings.count)
        .unwrap_or_default();
    host::set_title(key, &count.to_string());
}

deck_plugin_sdk::export!(Counter with_types_in deck_plugin_sdk);
```

The last line is mandatory: `export!` wires the type up to the generated
bindings. Without it, everything compiles and the component exports
nothing.

## Step 5: Build

```bash
cargo build --target wasm32-wasip2 --release
```

The result lands under `target/wasm32-wasip2/release/counter.wasm` — the
file name follows the crate name, with `-` turned into `_`. For this
example: about 100 kB.

## Step 6: Install

A directory named after the id, with exactly two files in it:

```bash
mkdir -p ~/.local/share/deckd/plugins/org.example.counter
cp target/wasm32-wasip2/release/counter.wasm \
   ~/.local/share/deckd/plugins/org.example.counter/plugin.wasm
cp plugin.toml \
   ~/.local/share/deckd/plugins/org.example.counter/plugin.toml
```

The `.wasm` file **must** be named `plugin.wasm`, and the manifest
`plugin.toml`.

## Step 7: Point a key at it

In a profile under `~/.config/deckd/profiles/`:

```json
{
  "schema": 1,
  "name": "Normal",
  "default": true,
  "default_page": "main",
  "pages": {
    "main": {
      "buttons": {
        "0": {
          "plugin": "org.example.counter",
          "action": "count",
          "settings": { "count": 0 }
        }
      }
    }
  }
}
```

`settings` is free-form JSON; the schema only knows about the plugin. The
rest of a key's fields (title style, background, icon) still apply — they
are documented in the daemon's profile format reference.

## Step 8: Restart the daemon and check

**A newly installed plugin is only noticed after a restart** — there is no
runtime rescan (see [`plugin-api.md`](plugin-api.md) §9).

`deckctl`'s own output is not yet localized to English — what it prints
today is German, regardless of the reader's own language, so that is what
is shown here too:

```console
$ deckctl plugins
Plugins:
  Counter [org.example.counter]
    Zustand: geladen
    Version: 1.0.0
    Aktionen: Counter
    Capabilities:
      Darf ausführen: keine
      Darf per HTTPS erreichen: keine
      Darf zusätzlich lesen: keine
      Darf Timer nutzen: nein
```

**"geladen"** means: manifest accepted, component compiled, `init` has run,
a key is visible. If it says **"nicht geladen"**, no key of this plugin is
on the active page yet — or no device is attached. The rest of the states
are in [`plugin-api.md`](plugin-api.md) §8.

Now key 0 shows a `0`; every press increments it, a long hold resets it —
and the value lives in the profile, so it survives a restart.

---

## The four rules you learn on your first plugin

**1. Draw on `on-settings-changed`, not on `set-settings`.** The daemon owns
the settings. `set-settings` is a *request*: only once it has been written
into the profile does it come back. A plugin that draws right after writing
may display a number that exists nowhere — and until then, `get-settings`
keeps returning the old value unchanged.

**2. Never panic.** An `unwrap` on something that comes from a file or from
a user is a trap; three traps in 60 seconds shut the plugin down. An error
belongs on the key (`show-alert`) **and** in the log — one is seen by
whoever is standing in front of the device, the other says why.

**3. The manifest is the security boundary, not the SDK.** Every check sits
in the host. Widening a capability just to make a settings field look nicer
pays for cosmetics with the boundary — the bundled `system` plugin does
without an application list for exactly this reason and takes a text field
instead.

**4. Check arguments even though there is no shell.** The allowlist names
command *names* and says nothing about arguments. `xdg-open` also opens
`file://`, and `gtk-launch` reads a leading `-` as an option. Not a hole in
the sandbox — but a way a settings field can mean something other than what
it looks like.

---

## Adding a capability

Example: a timer, for a display that refreshes itself.

```toml
[capabilities]
timer = true
```

```rust
fn on_appear(key: KeyRef) {
    // The id is free to choose, the interval is in milliseconds. The host
    // raises anything under 50 ms; it refuses more than 32 timers per
    // instance.
    if let Err(why) = host::set_timer(1000, TICK) {
        host::log(LogLevel::Error, &format!("no timer: {why}"));
    }
    remember(&key);
}

fn on_disappear(_key: KeyRef) {
    // Otherwise it keeps ticking while the page has long since changed.
    let _ = host::clear_timer(TICK);
}

fn on_timer(_id: u32) {
    // `on-timer` only gets the id — which key is meant is something the
    // plugin has to have remembered from `on-appear`.
}
```

The same pattern applies to the rest: `process = ["wpctl"]` and then
`host::run_process("wpctl", &args)`, `fs_read = ["/proc"]` and then
`host::read_file("/proc/stat")`, `http = ["example.org"]` and then
`host::http_request(&req)`. What each of these enforces is in
[`plugin-api.md`](plugin-api.md) §3 and §6.

**A denial is not a crash.** The call returns `Err` with a sentence that
ends in `is not allowed by this plugin's manifest`, and the plugin keeps
running. **Passing this sentence on verbatim** is the best thing a plugin
can do — then the user reads exactly what the host actually said.

---

## Testing without a device

The pattern of the three bundled plugins, and it's more than cosmetic:
**everything that can be decided without a runtime environment lives in
modules the SDK never touches.** Only the rest — the functions that call the
host — lives in a module scoped to `wasm32-wasip2`.

```toml
# Cargo.toml: tie the dependency to the target. This *enforces* the split
# instead of merely writing it down — when building for the host target,
# the SDK isn't even in the dependency graph.
[target.'cfg(target_family = "wasm")'.dependencies]
deck-plugin-sdk = { git = "https://github.com/SimixHD/deckd-plugins", tag = "sdk-v0.1.0" }
serde           = { version = "1", features = ["derive"] }
```

```rust
// src/lib.rs
pub mod rules;          // plain Rust, tested on the host

#[cfg(target_family = "wasm")]
mod guest;              // everything that calls the host
```

`cargo test` then checks `rules` the ordinary way, without WASM and without
a deck. In the main deckd project's own bundled plugins, this is
`hotkey.rs` and its three argument-checking functions for the `system`
plugin, and the parsing of `/proc/stat` for the `sysmon` plugin.

---

## When it doesn't work

| Symptom | Likely cause |
|---|---|
| `deckctl plugins` doesn't list the plugin at all | Directory name ≠ `id`, the manifest isn't named `plugin.toml`, or the manifest is malformed — the daemon names every rejected plugin, with a reason, at startup |
| State **defekt** | The `.wasm` fails to compile. Either `crate-type = ["cdylib"]` is missing, or it was built for the wrong target. After three attempts it goes dormant and comes back on its own once the file changes |
| State **deaktiviert** | `init` returned `Err` (once is enough), or three violations in 60 s. The reason is in the log |
| State **hängt** | Jobs or commands are getting lost; usually a guest call that's taking too long |
| Key shows "Plugin fehlt" | The plugin isn't loaded, is disabled, broken — or has been stuck for more than 2 s |
| Key stays blank | Not an error: nothing was drawn. `on-appear` is the place for that |
| A call returns "is not allowed by this plugin's manifest" | The capability is missing. The sentence names exactly what was requested |

As with `deckctl`'s own output above, none of this is localized to English
yet — the state names and the "Plugin fehlt" tile title are German in
today's build, whichever language the reader speaks. Everything a plugin
sends through `log` ends up in the daemon's journal; to follow it live, use
`deckctl watch`.

---

## Further reading

| Resource | Content |
|---|---|
| [`plugin-api.md`](plugin-api.md) | Every function, every boundary, every known limitation |
| The daemon's profile format reference, part of the main deckd project | How a key is described |
| The three bundled plugins (`system`, `audio`, `sysmon`), part of the main deckd project | `system` is the smallest, `sysmon` shows timers and drawn images |
| The `testkit` plugin, part of the main deckd project | Deliberately calls every gated function; the denial tests load it twice, with and without capabilities |
