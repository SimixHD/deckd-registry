# Plugin API

What a plugin is allowed to see, call, and expect from deckd. Anyone who
wants to *write* a plugin should start with
[`plugin-authoring.md`](plugin-authoring.md) — that's where the path from
`cargo new` to a running key is. This document covers what applies after
that.

**The authoritative source is `plugin.wit`, the plugin interface
definition**, not this document: both the host and the SDK are generated
from it, while this document is maintained by hand. Wherever the two
disagree, `plugin.wit` wins. §2 below reproduces the current `plugin.wit`
in full — a copy, not the source itself.

**And the second rule, right on its heels: the SDK enforces nothing.** Every
capability check sits in the host, in exactly the function that would do the
work. What the SDK offers is convenience; what the host lets through is the
truth. A plugin can therefore get away with nothing by going around the
library — and a plugin author never has to guess whether a check actually
happened.

---

## 1. What a plugin is

A directory with two files:

```
<id>/
  plugin.wasm     a component for wasm32-wasip2
  plugin.toml     the manifest (section 5)
```

The directory name **is** the `id` from the manifest. If it diverges, the
plugin is rejected: the daemon builds the data directory from the id
(`<root>/<id>/data`), and having code and data in two different places would
be a long-lived bug.

**A plugin is found in exactly one place:**
`~/.local/share/deckd/plugins/<id>/`. The bundled ones live there too.

The second location, `/usr/share/deckd/plugins/<id>/`, is **not a second
search path**, but the source of a copy: at startup, `deckd` copies from
there every plugin for which the user directory **does not yet have a
directory** — and only after that does the host look at all. If the
directory already exists, nothing is overwritten, or a marketplace update
would lose to the shipped version on the next login.

That there is only **one** search location is the simplification everything
else rests on: a bundled plugin is, after that, simply an installed one — in
the same place, uninstallable without a special case, retrievable again
through the marketplace. During development of the daemon itself,
`DECKD_SYSTEM_PLUGIN_DIR` is pointed at the build output directory where the
main deckd project compiles its own bundled plugins.

**The lifecycle in four sentences.** At startup, the daemon only reads the
manifests. A plugin is only instantiated once the **first key that uses it
becomes visible**; then `init` runs, then `on-appear` for every visible key.
It stays loaded until the daemon ends — a page or profile switch reports
`on-disappear`, but does **not** tear the instance down (otherwise
`sysmon`'s history graph would start back at zero on every switch). A plugin
that gets newly installed while the daemon is running is **only noticed
after a restart** (section 9).

---

## 2. The interface

The current definition of `plugin.wit`, in full:

```wit
package streamdeck:plugin@1.0.0;

interface types {
  /// Which key an event or call refers to.
  record key-ref {
    device:  string,
    profile: string,
    page:    string,
    index:   u8,
    action:  string,
  }

  enum log-level { error, warn, info, debug }

  record http-header    { name: string, value: string }
  record http-req       { method: string, url: string,
                          headers: list<http-header>, body: option<list<u8>> }
  record http-resp      { status: u16, headers: list<http-header>, body: list<u8> }
  record process-output { status: s32, stdout: list<u8>, stderr: list<u8> }
  record option-item    { value: string, label: string }
}

interface host {
  use types.{key-ref, log-level, http-req, http-resp, process-output};

  set-title:    func(key: key-ref, text: string);
  set-image:    func(key: key-ref, png: list<u8>);
  set-state:    func(key: key-ref, state: u8);
  show-alert:   func(key: key-ref);
  get-settings: func(key: key-ref) -> string;
  set-settings: func(key: key-ref, json: string);
  log:          func(level: log-level, message: string);

  http-request: func(req: http-req) -> result<http-resp, string>;
  run-process:  func(cmd: string, args: list<string>) -> result<process-output, string>;
  read-file:    func(path: string) -> result<list<u8>, string>;
  write-file:   func(path: string, data: list<u8>) -> result<_, string>;
  set-timer:    func(interval-ms: u32, id: u32) -> result<_, string>;
  clear-timer:  func(id: u32) -> result<_, string>;
}

interface guest {
  use types.{key-ref, option-item};

  init:                func() -> result<_, string>;
  on-appear:           func(key: key-ref);
  on-disappear:        func(key: key-ref);
  on-key-down:         func(key: key-ref);
  on-key-up:           func(key: key-ref);
  on-long-press:       func(key: key-ref);
  on-timer:            func(id: u32);
  on-settings-changed: func(key: key-ref);
  list-options:        func(source: string) -> list<option-item>;
}

world plugin {
  import host;
  export guest;
}
```

**`key-ref` is a complete address**, not a key index: device (serial
number), profile, page, index, and the action id from the manifest. All five
parts are needed. The daemon checks the first four before it executes a
command (section 3), and `action` is how a plugin with multiple actions
knows which one was pressed — `audio` has five.

**A plugin only ever sees its own keys.** The daemon sends an event
exclusively to the plugin that, according to the profile, sits on that key,
and conversely discards any command aimed at a key that has since come to
belong to someone else.

---

## 3. Host functions

| Function | Capability | Costs a violation? | Response |
|---|---|---|---|
| `set-title` | none | no | — |
| `set-image` | none | **no**, but limited (section 7) | — |
| `set-state` | none | no | — |
| `show-alert` | none | no | — |
| `get-settings` | none | no | JSON text, `{}` if the key has none |
| `set-settings` | none | no | — |
| `log` | none | no | — |
| `http-request` | `http` | no | `result<http-resp, string>` |
| `run-process` | `process` | no | `result<process-output, string>` |
| `read-file` | `fs_read` (only outside the plugin's own data directory) | no | `result<list<u8>, string>` |
| `write-file` | none — it only ever reaches the plugin's own data directory anyway | no | `result<_, string>` |
| `set-timer` · `clear-timer` | `timer` | no | `result<_, string>` |

**A denied capability does not end a plugin.** The call fails with a
message, the message goes into the log **and** over IPC to any open
interface, `deckctl plugins` shows it as *Last error* — and the plugin keeps
running. Violations in the sense of section 8 are something else: a trap, a
deadline overrun, a full queue.

**Every denial ends in the same sentence**, so it looks the same everywhere
and stays machine-recognizable:

```
running wpctl is not allowed by this plugin's manifest
reaching nas.local:8443 is not allowed by this plugin's manifest
reading /etc/passwd is not allowed by this plugin's manifest
setting a timer is not allowed by this plugin's manifest
```

A failure that is **not** a denial ("too many timers: no more than 32 at a
time", "connection refused") does not carry this sentence — otherwise the
manifest would get blamed for something it never decided.

### Drawing: `set-title`, `set-image`, `set-state`, `show-alert`

All four put a **command** into the return stream and return immediately;
drawing happens on the next pass of the device loop, so at the latest after
about 20 ms plus rendering and USB writes (~4 ms per key image).

What a plugin draws is a **layer per key** and survives the next command
from the same plugin: a `set-title` after a `set-image` does not throw the
image away. A page switch clears the layers — a new page means new keys.

- **`set-image` replaces the icon layer, not the key.** The profile's
  background tile stays underneath, the title set in the profile stays on
  top. A PNG with an alpha channel lets the background show through.
- **`set-title` replaces the text, not the style.** Font, size, alignment,
  and color stay the profile's — a plugin that writes a number shouldn't
  incidentally discard the user's styling.
- **`set-state`** picks among the states **the profile** lists for this
  key — not among the ones the action declares in the manifest. The
  `states = 2` in the manifest is information for the interface; what
  actually gets drawn is whatever is under `states` in the profile's key.
  If the profile doesn't describe the requested state, **the daemon draws
  nothing of its own**: a key's appearance belongs to the profile, and a
  daemon that invents one for an undescribed state would become a second
  owner of it. The key then stays at that key's icon and title — muted and
  loud look pixel-identical.

  **It still gets said**, both ways: a `warn!` line in the daemon's log,
  with the plugin, the key, the requested state, and the number the profile
  actually offers. Anyone who uses `set-state` should ship the states in
  their example profile snippet, or the action will be ineffective for the
  user.
- **`show-alert`** paints an amber tile and **ends on its own** after
  600 ms. It's the one piece of feedback that reaches the user standing at
  the device right now; the *reason* belongs in `log`.

The image is a PNG of any size and gets scaled like a profile icon —
limited to 4 MiB of file size and 1,048,576 pixels (section 7). If either
limit is exceeded, the image is discarded and the reason is reported; it is
not a violation.

### Settings: `get-settings` and `set-settings`

**The daemon is the sole owner of the settings.** The profile on disk is the
truth, and only the daemon writes it. What a plugin holds is a copy.

- **`get-settings`** returns the copy the daemon last sent — with no round
  trip, no waiting. The alternative — a question to the device loop and
  waiting for its answer — would be the **only** place in the whole design
  where a plugin waits on it.
- **`set-settings` is a request, not a transfer of ownership.** The call
  puts an ordinary command into the stream. Only once the daemon has
  written it into the profile (atomically) has the change actually
  happened — and it comes back as **`on-settings-changed`**. Until then,
  `get-settings` keeps returning the old value unchanged, and deliberately
  so: a guest that could read back its own not-yet-approved write would end
  up permanently sitting on a value nobody ever saved, the moment the
  command gets rejected.

From this follows the one rule a plugin author has to remember: **Draw on
`on-settings-changed`, not on `set-settings`.**

Settings that aren't valid JSON change nothing.

### `log`

Goes to the daemon's `tracing` subscriber (`journalctl`, terminal) **and**
as `Event::PluginLog` to every open IPC connection. Four levels: `error`,
`warn`, `info`, `debug`.

If the return channel loses a diagnostic message because a plugin talks
faster than it's being collected, `deckctl plugins` counts that as *Dropped
diagnostics* — **separate from the state**. A plugin that has only lost a
log line hasn't lied to anyone; its key is still correct.

### `http-request`

The only way out of the sandbox and onto the network: `wasi:sockets` isn't
in the linker, so there is no second one. Three checks, not one:

1. **Hostname and port**, before anything is opened — and **again at every
   redirect hop** (at most 5). Without that, one allowed host that redirects
   would be enough to turn the whole list into decoration.
2. **Where the name actually resolved to** — using the exact same resolver
   that the connection is then made with. Private, loopback, and link-local
   addresses are forbidden, unless the manifest says `http_private = true`.
3. **What the call is allowed to cost** — 10 s for the whole call including
   every hop, response at most 4 MiB.

Anything whose target isn't unambiguous is rejected rather than guessed at:
whatever doesn't parse as a URL, every scheme except `https`, a
`user:password@` in front of the host (in `https://example.org@evil.test/`,
the naive reading names the wrong one of the two), and an **IP address
instead of a name** — a list of names and an address can't be honestly
compared.

No proxy: `HTTP_PROXY` from the daemon's environment is deliberately not
honored (section 9).

### `run-process`

Starts a child, waits for it to end, and returns the status, `stdout`, and
`stderr`. Four properties, each of them enforced:

- **Allowlist, exact name.** `wpctl` permits neither `/usr/bin/wpctl` nor
  `wpctl; rm -rf /`.
- **No shell.** `Command::new(cmd)` with separate arguments. An argument can
  therefore never turn into a second command.
- **A curated environment.** First `env_clear()`, then exactly four
  variables: `PATH`, `HOME`, `XDG_RUNTIME_DIR`, `WAYLAND_DISPLAY`. The
  daemon's environment can carry session secrets; a child in the sandbox has
  no business seeing any of that.
- **A deadline enforced by killing.** 5 s, after which the child's entire
  process group gets the signal. Epochs interrupt WebAssembly, not a
  process in the kernel — without this deadline, a plugin could stall
  itself with a permitted command.

Both output streams are drained by their own dedicated thread from the
moment the process starts, and capped at 1 MiB. Reading only after the
child ends led to a deadlock as soon as a child wrote more than one pipe
buffer's worth.

### `read-file` and `write-file`

**Writing needs no capability and can only ever hit one location:** the
plugin's own data directory, `~/.local/share/deckd/plugins/<id>/data/`. The
path must be **relative**, `..` is rejected, symlinks are resolved before
the decision is made. No manifest can widen this. At most 4 MiB per call,
checked **before** anything is opened — so an oversized write leaves behind
no truncated file and no new directories.

**Reading** is split in two: a **relative** path reads from that same data
directory and also needs nothing. An **absolute** one has to fall under a
prefix from `fs_read`.

- A prefix is a list of **path components**, never a piece of text: `/proc`
  permits `/proc/stat` and **not** `/procurement/secrets`.
- Symlinks are resolved **before** the check — otherwise the decision would
  be made about the link instead of the file.
- It's opened with `O_NONBLOCK`, and anything that is **not a regular
  file** is refused — checked on the open descriptor. `File::open` on a
  FIFO with no writer *blocks*, and it does so before any check runs and
  without the daemon being able to cancel it. A consequence worth stating: a
  plugin **cannot** read from a character device or a pipe (section 9).
- At most 4 MiB per call. The size is asked for first, then the read is
  capped anyway: procfs reports a size of zero for files that do have
  content, and a file can grow between the two calls.

**There is no directory listing.** Anyone who needs to find zones or
devices walks an index instead — that's what `sysmon` does with
`/sys/class/thermal/thermal_zone<n>/temp`.

### `set-timer` and `clear-timer`

`set-timer` is **periodic** and is called again with the same id to change
the interval. `clear-timer` ends it — that's what a plugin does in
`on-disappear` to go quiet.

Two limits are decided by the host, not the plugin: the interval never goes
below **50 ms** (too small a request gets raised, not rejected), and an
instance holds at most **32** timers. The timers live in the **daemon's**
memory, outside the sandbox's 16 MiB — a loop over `set-timer` with a fresh
id each time would otherwise grow the host instead of the plugin.

Both functions are gated on `timer = true`, `clear-timer` included:
answering a plugin without the capability with a cheerful `ok` would amount
to telling it that it has it.

---

## 4. Guest entry points

| Function | When |
|---|---|
| `init` | once, right after instantiation, **before** every `on-appear` |
| `on-appear` | a key of this plugin has become visible — when the page appears, after a profile switch, and after the instance is rebuilt |
| `on-disappear` | the key is no longer visible. Shutting down timers belongs here |
| `on-key-down` | a real key edge, at the moment of pressing |
| `on-key-up` | a real key edge, at the moment of release |
| `on-long-press` | held for 500 ms — **additionally**, between `down` and `up` |
| `on-timer` | a timer armed with `set-timer` is due |
| `on-settings-changed` | the daemon has written new settings for this key |
| `list-options` | a dynamic source from the manifest is being queried |

**The order of a press.** A short press: `on-key-down`, `on-key-up`. A long
press: `on-key-down`, then `on-long-press` when the threshold is reached
(not only on release), then `on-key-up`. A **short** press is not
additionally reported as a gesture — down and up describe it completely,
and two versions of the same press would be one too many.

The edges are what set `on-key-down`/`on-key-up` apart from the IPC view:
push-to-talk and hold-to-repeat need pressing and releasing exactly as they
happen. The `long_press = true` in the manifest is a **declaration for the
interface**, not a switch: `on-long-press` is delivered whether or not it's
set.

**If the first press wakes the deck from dimming** (`wake_on_press`), that
press never reaches a plugin — not as an edge and not as a gesture.

That applies to the **press**, not to the interval: the guard remembers
which key woke the device, and doesn't let go of it until it is itself
released. Two consequences a plugin author should know:

- A key that is **held across a dimming event** did not do the waking. It
  is therefore reported through to the end — `on-key-up` and, if it was
  held long enough, `on-long-press` — even though the display was dimmed at
  that moment, or has only just come back on.
- Conversely, the waking press gets **neither**: no `on-key-down` and no
  `on-key-up` either. A plugin never gets to see half a gesture.

**`init` is allowed to say no.** An `Err` shuts the plugin down
**immediately** (not through the violation counter): a plugin that says no
hasn't misbehaved, and the refusal doesn't change on the next attempt. A
*trap* during startup is something else — that gets counted and retried
after 5 s.

**`list-options` has no caller in the daemon today.** It's in the ABI
anyway, because exports can't be retrofitted in a backward-compatible way:
once the marketplace is live, extending the guest side after the fact would
break every plugin already published. The Property Inspector from
sub-project 3 will call it. `audio` already answers it.

---

## 5. The manifest

`plugin.toml`, TOML, `deny_unknown_fields` — a typo in a field name is a
named error, not a silently ignored field.

```toml
id      = "org.example.audio"         # reverse domain name, = directory name
name    = "Audio (PipeWire)"
version = "1.0.0"
api     = 1                            # currently supported: 1
author  = "Simon Plomer"
license = "MIT OR Apache-2.0"

[capabilities]
process      = ["wpctl", "pw-link", "pw-dump"]
http         = ["example.org", "nas.local:8443"]
http_private = false
fs_read      = ["/proc", "/sys"]
timer        = true

[[actions]]
id          = "fader"
name        = "Volume"
description = "Changes the volume; a long press fades in the key's direction."
states      = 1        # how many appearances the action cycles through
long_press  = true     # declaration for the interface
icon        = "lib:lucide/volume-2"   # optional, see below
fields = [
  { key = "targets",  type = "multi", label = "Targets", options = "audio.aliases" },
  { key = "step_pct", type = "num",   label = "Step (%)", default = -5, min = -100, max = 100 },
]

# Optional, see "Translations" below.
[i18n.de]
"Volume" = "Lautstärke"
"Changes the volume; a long press fades in the key's direction." = "Ändert die Lautstärke; ein langer Druck blendet weich in die Richtung der Taste."
"Targets" = "Ziele"
"Step (%)" = "Schritt (%)"
```

**The visible fields are written in English.** That's not a style
preference: the fallback is always the value from the manifest, so it has
to be the language the most people can read — the same reasoning that keeps
a `.po` file's `msgid` in English.

| Field | Rule |
|---|---|
| `id` | Reverse domain name from `a-z`, `0-9`, `.`, `-`, `_`, with no leading dot. **Allowlist, not a blocklist**: the id becomes the directory name, and `PathBuf::join` with an absolute path *replaces* the root instead of appending to it — an unchecked id like `/home/user/.config/systemd/user` would move the one folder a plugin without a capability may write to, to an arbitrary place on disk. Rejected rather than sanitized |
| `api` | Higher than the supported version → the manifest is rejected, by name |
| `actions` | At least one; ids must be unique |
| `fs_read` | Absolute paths only, and none of them containing `..` |
| `http` | A hostname, optionally with `:port`. Without a port, 443 applies; with one, exactly that port. **ASCII-only:** an internationalized name has to be written as punycode (section 9) |
| `actions[].icon` | **Optional**, `#[serde(default)]` — a manifest without this field keeps loading unchanged, `Manifest::api` stays `1`. When set: a `deck_core::IconRef` identifier, always a library reference (`"lib:<set>/<name>"`), never a path — a path would need a second resolution root (the plugin directory) that doesn't exist here. Meant to appear on a key that doesn't choose an identifier of its own; how and where this gets resolved and drawn hasn't been built yet (the icon library, sub-project 3). The grammar and the three forms an identifier can take are documented in the daemon's profile format reference, under "Identifier instead of path" |

Field types: `text`, `num`, `bool`, `select`, `multi`, `group`, `keys`,
`json`. `options` takes either a fixed list or the name of a dynamic source
that `list-options` answers.

### Translations: `[i18n.<language>]`

Optional. One table per language, **keyed by the source sentence** —
exactly the model `po/de.po` follows for the interface:

```toml
[i18n.de]
"Volume" = "Lautstärke"
"Targets" = "Ziele"
"direction" = "in Tastenrichtung"

[i18n.de_AT]
"Volume" = "Lautstärkn"
```

| Question | Answer |
|---|---|
| Who looks it up? | The **interface**, not the daemon. `ListPlugins` passes the manifest on exactly as read: two editors can run in two languages, the daemon only knows one — and a manifest already translated over IPC would look different from the file it came from |
| Lookup order | The same one GNU gettext uses to search its catalog — **measured**, not approximated (see below), with the value from the manifest last. If the language list has more than one entry (`LANGUAGE=fr:de`), the list is worked through in order, and each entry goes all the way down before the next one gets a turn |
| Missing language, missing sentence | Not an error. Out comes the value from the manifest, so **never an empty label** |
| An empty entry (`"Volume" = ""`) | Counts as *not translated*, like an empty `msgstr` in a `.po` file. Otherwise, leaving a line blank would be the one way to produce an empty label after all |
| What belongs in it? | Every sentence the interface shows: the plugin's `name`, each action's `name` and `description`, every `label` — **and the values of a fixed `options` list**, since those appear as the labels of a selection list |
| What doesn't? | Ids, field keys, `version`, `author`, `license`, and the **name of a dynamic source** (`options = "audio.aliases"`) — that's an identifier `list-options` answers, not a label |

**The lookup order, spelled out.** What the environment names is a **locale
name** (`de_DE.UTF-8`), not a language tag (`de_DE`) — which is why "exact
key, then bare language" isn't enough. Measured on 2026-08-18, with a
catalog present under each of the twelve spellings and the winner deleted
after every run (`LANGUAGE=de_DE.UTF-8@euro`, `LC_ALL=en_US.utf8`):

```text
 1. de_DE.UTF-8@euro     5. de.utf8@euro      9. de_DE
 2. de_DE.utf8@euro      6. de@euro          10. de.UTF-8
 3. de_DE@euro           7. de_DE.UTF-8      11. de.utf8
 4. de.UTF-8@euro        8. de_DE.utf8       12. de
```

Three nested loops: the **modifier** drops off last, the region before
that, and innermost the encoding — first as written, then in glibc's own
spelling (`UTF-8` → `utf8`), then not at all. In practice that means:
`[i18n.de_DE]` is reached by an ordinary German machine, `[i18n.de]` by
every German locale, and a manifest that only carries `[i18n.de]` is served
exactly right.

**A hyphen is not a region.** `de-DE` is the spelling used outside POSIX;
glibc doesn't recognize it and then searches exclusively under `de-DE`
itself (measured, the same day). A table key `[i18n."de-DE"]` is therefore
never reached — write `[i18n.de_DE]`. Whatever a person types into
`general.language` with a hyphen, deckd converts **once**, before the
window and the manifest go looking, so both end up speaking the same
language.

**The values of a fixed selection list stay values.** `options =
["direction", "silence"]` sits exactly like that in the settings of every
key a user has created; only what the selection field **displays** gets
translated. Translating the value itself would rewrite other people's
profiles.

**Why not `name.de` next to `name`?** Because TOML can't do that: `name`
would be both a string and a table in the same table at once. Verified on
2026-08-18 with `toml` 1.1.4 — `cannot extend value of type string with a
dotted key`, and the other way round, `duplicate key`. The proof of that
lives in the daemon's own manifest-parsing code.

**`api` stays 1.** The field is optional (`#[serde(default)]`), a manifest
without `[i18n]` loads unchanged. **But:** an **older** deckd that doesn't
know the table rejects a manifest carrying `[i18n]` with ``unknown field
`i18n` `` — not with a version message. That is the price of leaving `api`
alone, and it's paid deliberately: `api` is checked for **equality**, so
bumping it to 2 would reject every existing manifest, including every one
that carries no translation at all. The same rule applies to every new
manifest field; this is where it's written down for the first time.

A profile points at all of this with three fields on a key:

```json
"11": {
  "plugin": "org.example.audio",
  "action": "fader",
  "settings": { "targets": ["music"], "step_pct": -5 }
}
```

`settings` is free-form JSON and gets passed through unchanged — the schema
only knows about the plugin. A manifest that names an unknown capability,
carries an unknown field, or points at too high an `api` is **reported by
name and reason** at startup and skipped, not swallowed — the same rule a
malformed profile already follows.

---

## 6. Capabilities

| Capability | What gets enforced |
|---|---|
| `process` | An allowlist of command names, exact match. No shell, a curated environment, a deadline |
| `http` | An allowlist of hosts with an optional port, `https` only, every redirect hop checked again, the address range checked after resolution |
| `http_private` | Lets the declared hosts point into a private network. Without this opt-in, private, loopback, and link-local addresses are rejected **after** name resolution |
| `fs_read` | A prefix list of absolute paths, compared component by component, symlinks resolved beforehand |
| `timer` | Without it, both `set-timer` **and** `clear-timer` fail |

**If the `[capabilities]` block is missing, everything is denied.** There is
deliberately no value that means "allow everything": every line is
something someone had to write down, and that a user can read before
installing. `deckctl plugins` shows every one of the capabilities, including
the empty ones — "none" is an answer, a missing line would not be.

**Two layers of sandbox, not one.** A `wasm32-wasip2` component built from
Rust imports WASI because `std` needs it. This layer is set up **closed**:
nothing is preopened, `wasi:sockets` never makes it into the linker, and
neither the environment nor stdio is inherited. Only above that sit the
checks from this table. So `std::fs` reaches nothing — a test proves that
from the inside.

**A subdomain is a different host.** `example.org` does not let
`evil.example.org` through: an entry that silently covered its subdomains
too would be a wildcard nobody actually wrote down — one that anyone able
to register beneath the declared name could walk right into.

---

## 7. Limits

The numbers from §7 of the design spec, matching what the code actually
enforces:

| What | Value | Why |
|---|---|---|
| Linear memory per instance | **16 MiB** | `sysmon`'s graph needs 20 KB; generous and still a limit. **Summed per instance**, not per memory region: a component may create several, and then their sum is what counts |
| Table elements per instance | **65,536**, also summed | Table elements live in the **host's** memory, outside the 16 MiB. The largest honest guest in this tree asks for 117; the limit is 560 times that. It only touches a component that runs `table.grow` at runtime — guests built from Rust don't |
| Compute time per guest call | **500 ms** | breaks infinite loops |
| Deadline resolution | 10 ms (one epoch-ticker thread for all stores) | also the overrun a runaway guest can get away with |
| `run-process` | **5 s**, then the process group is killed | epochs don't stop a child in the kernel |
| A child's output | **1 MiB per stream** | otherwise `cat /dev/zero` fills up the daemon |
| `http-request` | **10 s** for the whole call, response ≤ **4 MiB**, ≤ **5** redirects | |
| `read-file` · `write-file` | **4 MiB** per call | |
| `set-image` | **4 MiB** file **and** **1,048,576 pixels** (1024×1024) | two rules, because there are two costs: bytes tie up memory, pixels tie up time. Checked on the PNG header, **before** decoding — decoding itself *is* the attack (measured: a 470 kB file, 11,000×11,000, one pass took 47.95 s and +486 MB) |
| Smallest timer interval | **50 ms** (smaller requests get raised) | a plugin shouldn't be able to stall itself |
| Timers per instance | **32** | they live in the daemon's memory, outside the 16 MiB |
| Job queue | **64** slots | at ~4 events per press, sixteen presses of backlog |
| Return channel per plugin | **256** commands, **one per slot** | one slot per *command*, not per batch: a guest call once handed over 100,000 commands at once, and the pass that executed them took 128 s |
| Commands per pass | **64** per plugin, **128** across all of them | the device loop works through a backlog in bounded portions instead of draining it all at once — round-robin, so one loud plugin can't spend the budget before its quiet neighbor gets a turn. Nothing is lost in the process, it just takes a few more passes |
| Shutdown threshold | **3 violations in 60 s** | a trap, a deadline overrun, and a full queue all count the same |
| Failed build attempts | **3**, spaced 5 s apart | its **own** counter, not the violation budget — see section 8 |
| Daemon exit deadline | **2 s** for all threads together | after that, it gives up, with logging |

**Compute time is compute time, not wall-clock time.** Time the host spends
on the plugin's behalf — `run-process`, `http-request`, `read-file` — is
refunded, otherwise a plugin would die from a call its own manifest allows.
The flip side of that is in section 9.

---

## 8. States, errors, and the error tile

The `deckctl` column is what the tool actually prints today — its output is
not yet localized to English, regardless of the reader's own language:

| State | `deckctl` | What triggers it | How it ends |
|---|---|---|---|
| `Loaded` | geladen | running and keeping up | — |
| `NotLoaded` | nicht geladen | no key visible yet — or not installed at all; the same from the device loop's point of view | the first visible key |
| `Disabled` | deaktiviert | 3 violations in 60 s, or `init` refused | a daemon restart, or an explicit re-activation: `deckctl restart-plugin <id>` |
| `Broken` | defekt | the component fails to compile three times | **on its own**, as soon as the file changes — the thread reads and hashes it every 5 s |
| `Stuck` | hängt | jobs or its own commands are getting lost | as soon as the plugin catches up again |

**The error tile** is dark gray (`[40, 40, 40]`) with the title **"Plugin
fehlt"** (also not yet localized), and thereby distinguishable from the
dark-red tile of an icon that fails to render. It appears immediately for
`NotLoaded`, `Disabled`, and `Broken` — and for `Stuck` **only after 2 s**: a
single full pass should stay invisible, a plugin that keeps losing
shouldn't. A tile that flickers at every bit of congestion teaches a user to
ignore it.

The tile doesn't say *which* plugin is missing — on 72×72 pixels, "Plugin
fehlt" fits and little else. The id is in the log and in `deckctl
plugins`.

**The assignment stays in the profile.** A plugin that's missing doesn't
lose its keys; they come back to life as soon as it's back.

**A trap is not the end.** The instance is thrown away and a fresh one
built immediately, which also finds out again which keys are visible. The
violation counter survives this — otherwise the shutdown threshold could
never be reached.

**Build failures and guest failures have separate budgets**, and that
matters more than it sounds like it should: back when a failed build still
counted as a violation, a single `pacman -U` that caught the component
mid-write permanently disabled a plugin that had had two legitimate traps in
the last minute — with no way back.

**The way back is `deckctl restart-plugin <id>`.** It throws away any
existing thread, gives the replacement a fresh violation counter, and
reports the state before and after. Two properties worth knowing:

- It **starts** a plugin even when none of its keys are visible. That's the
  honest reading of "restart"; a thread and a compiled component come into
  being, and nothing cleans them up again before the daemon ends.
- What it reports is the state **at that moment**. Compiling happens on the
  plugin's own thread, so `Loaded` here means "a thread exists and has
  gotten its keys", not "the component has finished compiling". That
  verdict arrives seconds later, through `deckctl plugins`.

For `Broken` it isn't needed: a plugin whose file gets fixed comes back on
its own (§11 of the design spec) — the thread reads and hashes it every
5 s.

---

## 9. Known limitations

Everything here is **a decision, not an oversight**. Anyone building on this
ABI needs to know it.

- **Path resolution is not atomic.** Resolving and opening are two steps; a
  symlink can change in between (TOCTOU). The attacker would have to be a
  plugin that already has read access to the declared prefixes anyway — an
  acceptable cost, rather than hand-building `openat2` and path resolution.
  The type check on open still happens on the **open descriptor** though, so
  swapping a regular file for a FIFO doesn't get through.
- **`read-file` reads only regular files.** A deliberate narrowing of the
  ABI: it's correct for `/proc` and `/sys` (both report `S_IFREG`), but a
  plugin cannot read from a character device or a pipe. The reason is in
  section 3.
- **The thread of a stuck plugin leaks until the daemon ends.** A host call
  that hangs without a child process cannot be killed from Rust. The answer
  is to rebuild on a fresh thread, plus a generation counter that discards
  the old one's commands; the old thread is left sitting there. That price
  is lower than letting every plugin die together.
- **A child process can outlive the daemon.** If a plugin thread is given up
  on after the 2-s deadline while its `run-process` is still running, the
  child does not end along with the daemon. That would be covered by
  `KillMode=control-group` in the systemd unit — **which does not exist
  yet**, it's coming in sub-project 6. This is described here, not
  guaranteed.
- **Hostnames in the manifest have to be punycode.** `url` punycodes while
  parsing; a Unicode host would never match. So it's rejected as
  `ManifestError::BadHttpHost` already at load time, rather than failing
  inexplicably at runtime.
- **`VmSize` sits at around 12.7 GB, and isn't one.** Wasmtime reserves a
  guard region per instance; none of it is actually resident. Anyone reading
  `ps` or a system monitor sees this as `VIRT` and is right to be alarmed.
  The measured *resident* value is documented in the daemon's architecture
  reference (§6), part of the main deckd project: 43.6 MB with three plugins
  loaded.
- **A newly installed plugin is only noticed after a restart.** The host
  reads the user directory **once, at build time**; there is no runtime
  rescan. Until the marketplace exists (sub-project 5), there's also no way
  to install anything at runtime in the first place — that will need one.
- **No quota on the data directory.** `write-file` is limited to 4 MiB per
  call, but a plugin can repeat the call as often as it likes and fill up
  the disk that way. The limits in section 7 cover memory and compute time
  inside the process, not space on disk.
- **A call can stretch its wall-clock time out over host calls.** Because
  host time is refunded (section 7), a guest that calls `read-file` on a
  large file in a loop can hold its thread for an arbitrary length of
  time — its *compute time* is capped, its wall-clock time isn't. This is
  exactly why a full queue is itself a violation.
- **A burst that fills the queue several times counts several times.**
  Counting is edge-triggered: full → one violation, room again → reset. In
  theory, three violations could be triggered within one second this way;
  in practice the device loop sends a handful of jobs per pass, so it
  doesn't happen.
- **No proxy for `http-request`.** Honoring `HTTP_PROXY` from the daemon's
  environment would be the wrong path — an environment variable that
  appears in no manifest would send a checked host's traffic through an
  unchecked one. If this comes, it belongs in `settings.toml`.
- **One agent per call.** `http-request` builds its own `ureq::Agent` per
  request, meaning one TLS handshake per request. Uncritical for key
  presses; one agent per plugin would be the optimization once something is
  polled every second.
- **The compiled-artifact cache never cleans up.** Its key is the content
  digest of the `.wasm`; nothing is ever removed from it.

---

## Further reading

| Resource | Content |
|---|---|
| [`plugin-authoring.md`](plugin-authoring.md) | From `cargo new` to a running key, with a complete example |
| The daemon's architecture reference (§4), part of the main deckd project | Why one thread per plugin, and how commands flow |
| The daemon's profile format reference, part of the main deckd project | How a key points at a plugin |
| The internal design spec, part of the main deckd project | All the reasoning behind the decisions in this document |
