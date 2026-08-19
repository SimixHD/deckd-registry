# Contributing a plugin

## The path

1. Build the plugin.
2. Put both files somewhere that answers durably — a release in your own
   repository is enough. Two files, `plugin.wasm` and `plugin.toml`, each at
   its own URL. No archive.
3. Compute both checksums and both sizes:

   ```bash
   sha256sum plugin.wasm plugin.toml
   stat -c '%s %n' plugin.wasm plugin.toml
   ```

4. Fork this repository.
5. Add your plugin's entry to `index.json` — one `plugin` object if this is
   a new plugin, or one more entry at the *top* of an existing plugin's
   `versions` list if this is a new release. See the format in
   [`README.md`](README.md#the-index-format).
6. Open a pull request.

The pull request is the review. CI runs the same checks a client would run
against the index before it trusts it, and a human reviews the one thing no
tool can — see below.

## What CI checks

Every check the CI runs is the same one this registry's own tools run, and
what follows is what those tools actually print, not a paraphrase. The
`{id}`, `{version}` and similar are stand-ins for your plugin's real values.

Checked from `index.json` alone, before anything is fetched:

| Rule | Message it produces |
|---|---|
| The schema number is one this tool understands | `index says schema {found}, this tool speaks 2` |
| No plugin `id` is listed twice | `{id}: listed twice` |
| `id` is a reverse domain: at least three parts separated by dots, none empty | `{id}: an id needs at least three parts separated by dots, none of them empty — the domain you own, reversed, then a name for the plugin, such as org.example.thing` |
| `id` holds only `a-z`, `0-9`, `-` and `_` between its dots | `{id}: an id holds only a-z, 0-9, - and _ between its dots; 'T' is none of those` |
| `id` does not start with the reserved namespace | `{id}: dev.simix.* is reserved for the registry's own plugins` |
| The plugin has at least one version | `{id}: no versions listed` |
| `homepage` is `https://` | `{id}: the homepage url is not https` |
| `version` is a semantic version | `{id} {version}: not a semantic version` |
| No `version` is listed twice for one plugin | `{id} {version}: listed twice` |
| Versions are sorted newest first | `{id}: {earlier} is listed above {later}; versions go newest first` |
| Each `sha256` is 64 hex characters | `{id} {version}: the module sha256 is not 64 hex characters` (or `manifest`) |
| Each `sha256` is lowercase, the way `sha256sum` prints it | `{id} {version}: the module sha256 has uppercase letters; write it the way sha256sum prints it, in lowercase` (or `manifest`) |
| Each artifact `url` is `https://` | `{id} {version}: the module url is not https` (or `manifest`) |

Checked by fetching the two files each version names — this is the part that
touches the network:

| Rule | Message it produces |
|---|---|
| The URL answers at all | `{id} {version}: the module could not be fetched: {why}` (or `manifest`) |
| The fetched bytes hash to the stated `sha256` | `{id} {version}: the module hashes to {actual}, the entry says {stated}` (or `manifest`) |
| The fetched bytes are the stated `bytes` size | `{id} {version}: the module is {actual} bytes, the entry says {stated}` (or `manifest`) |
| `plugin.toml` parses at all | `{id} {version}: the manifest could not be read: {why}` |
| `id` in `plugin.toml` matches the index entry | `{id} {version}: the index says id is {index}, the manifest says {manifest}` |
| `version` in `plugin.toml` matches the index entry | `{id} {version}: the index says version is {index}, the manifest says {manifest}` |
| `api` in `plugin.toml` matches the entry's `min_api` — the same number under two names | `{id} {version}: the index says min_api is {index}, the manifest says {manifest}` |
| Each of the five `capabilities` fields (`process`, `http`, `http_private`, `fs_read`, `timer`) in `plugin.toml` matches the index entry | `{id} {version}: the index says capabilities.{field} is {index}, the manifest says {manifest}` |

A capability mismatch names the one field that disagrees, not the whole
struct — an entry can understate what a plugin does (a permission the
manifest declares but the index hides) or overstate it (the reverse), and
either is reported the same way.

`--allow-reserved` lets an index use the `dev.simix.*` namespace; it exists
for this registry's own CI on pushes to `main` and on pull requests whose
branch already lives in this repository, so a pull request from your fork
runs without it and cannot claim that namespace. Do not pass it yourself —
your entry does not need it and it would only mislead about what the flag is
for.

## What a human checks

What no rule above can decide: whether the permissions a plugin asks for
match what it actually does. This is the one subjective gate, so it is
written plainly: *a plugin that controls the volume and asks for network
access will be rejected.*

## Identifiers

`id` is a reverse domain of a domain you own — `org.example.thing`, not
`thing`. `dev.simix.*` is reserved for this registry's own plugins; a pull
request from a fork is checked without `--allow-reserved`, so nobody outside
this repository can claim it.

The characters are exactly the ones a `plugin.toml` id may hold — `a-z`,
`0-9`, `-` and `_`, with dots between the parts, the rule in
[`docs/plugin-api.md`](docs/plugin-api.md#5-the-manifest). The registry adds
one thing on top of it: at least three parts, none of them empty. The daemon
would happily load a plugin called `audio`, because there the id is only a
directory name on one machine; here it is the key the whole marketplace is
indexed by, and the first person to ask for a word like that would be taking
it from everybody else.

## Versions

Newest first, semantic, no duplicate `version` string for the same plugin. A
published version is never edited — a change is a new version added at the
top of the list, never a rewrite of one already there.

## Withdrawing a version

Set `yanked` to `{ "reason": …, "since": … }` instead of deleting the entry.
A deleted entry reaches nobody who already installed it; a `yanked` one at
least tells a client not to offer it again.

## Before you open the pull request

Run the same check CI will run:

```bash
cargo run -p index-tools -- check index.json
```

This runs every rule in the two tables above: first everything that can be
decided from `index.json` alone, and — only if that comes back clean — a
fetch of every `module` and `manifest` URL in the file, to confirm each one
is reachable, hashes to its stated `sha256`, is the stated number of bytes,
and that the `plugin.toml` behind it agrees with the index entry on `id`,
`version`, `min_api` and every `capabilities` field. A clean run prints one
line naming
how many plugins the index holds — today, with no plugins listed yet, that
line reads:

```
index.json: 0 plugins, everything checks out
```

Your run will name however many plugins your `index.json` holds once your
entry is in it.
