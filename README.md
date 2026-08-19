# deckd-registry

The plugin marketplace for deckd. One Git repository holding one
index file, `index.json`. No server, no database, no account.

The registry half described here is live today: the index, its schema, the
checks that guard it, and the published site. The client half — a deckd
release that actually reads this index and installs a plugin from it — is
Phase B of the marketplace plan and does not exist yet. Anywhere this README
describes what a client does, it is describing what a later release will do,
and it says so.

## How an install will work

No shipped version of deckd installs plugins from this registry yet; this is
the plan a later release implements. Once it does: the client fetches
`index.json` over HTTPS, picks the newest version whose `min_api` its own API
level reaches and that is not `yanked`, downloads both `plugin.wasm` and
`plugin.toml`, checks both against their `sha256`, and compares the manifest
it just downloaded against the index entry that pointed at it — the same
comparison the registry's own CI already runs on every pull request (see
[What CI checks](CONTRIBUTING.md#what-ci-checks) in `CONTRIBUTING.md`).

## The index format

Schema 2. One entry per plugin, one entry per released version inside it,
newest version first:

```jsonc
{
  "schema": 2,
  "updated": "2026-08-19",
  "plugins": [{
    "id":          "dev.simix.audio",
    "name":        "Audio (PipeWire)",
    "description": "Volume, mute and routing via PipeWire.",
    "author":      "Simon Plomer",
    "homepage":    "https://github.com/SimixHD/deckd-plugins",
    "categories":  ["audio"],
    "i18n": { "de": { "Audio (PipeWire)": "Audio (PipeWire)",
                      "Volume, mute and routing via PipeWire.":
                      "Lautstärke, Stumm und Routing über PipeWire." } },

    "versions": [{
      "version":      "1.0.0",
      "min_api":      1,
      "license":      "MIT OR Apache-2.0",
      "capabilities": { "process": ["wpctl", "pw-link", "pw-dump"],
                        "timer": true },
      "module":   { "url": "https://…/audio-v1.0.0/plugin.wasm",
                    "sha256": "…", "bytes": 148992 },
      "manifest": { "url": "https://…/audio-v1.0.0/plugin.toml",
                    "sha256": "…", "bytes": 4212 },
      "yanked":    null,
      "signature": null
    }]
  }]
}
```

| Field | Level | Why |
|---|---|---|
| `id` | plugin | Unique across the whole marketplace; a reverse domain of a domain the author owns (see [Identifiers](CONTRIBUTING.md#identifiers)) |
| `categories` | plugin | Sorting only: `audio`, `system`, `media`, `smarthome`, `development`, `other` |
| `i18n` | plugin | Keyed by the **English source sentence**, the same way `plugin.toml` and `po/de.po` are |
| `version` | version | Semantic versioning |
| `min_api` | **version** | It changes between versions — that is exactly why it lives here and not on the plugin. It is the `api` from that version's own `plugin.toml` under the index's name, and CI compares the two |
| `license` | **version** | SPDX identifier; a licence change is a version event |
| `capabilities` | **version** | Same reasoning, and comparing two versions' capabilities is the basis of the update rule |
| `module`, `manifest` | version | Each an URL, `sha256` and `bytes` — two files, no archive |
| `yanked` | version | `null`, or `{ "reason": …, "since": … }` |
| `signature` | version | Always `null` today, deliberately (see [Why there is no signature](#why-there-is-no-signature)) |

`bytes` is available before a download starts, on purpose: a size a person
can see before they click matters more than one they find out after.

## Why capabilities live in the index

So a client can show them in plain text **before** it downloads anything. The
index entry is the announcement; the daemon that eventually runs the plugin
is the enforcement — two separate steps, on purpose, so that what the entry
promised and what actually gets enforced can be compared instead of assumed
equal.

## Why there is no signature

HTTPS to the index covers the path a byte travels. The `sha256` in each
entry covers the file itself, so an author cannot swap a `.wasm` without
breaking the checksum that names it. The one attack neither covers — someone
taking over this repository's GitHub account and changing entries along with
their checksums — a signing key would not have stopped either, because kept
anywhere this repository's own CI could reach it, such as its Actions
secrets, that key would have been exactly as reachable to that attacker as
the index already is.

`signature` stays in the schema and stays `null`, so that adding one later is
an addition rather than a breaking schema change; nothing reads it today.

## Running your own registry

The format is host-neutral, and this much is true today: any repository that
serves an `index.json` of this shape is everything a third-party registry
needs to be.

Choosing that source from deckd is not yet possible — no released version of
deckd reads more than one registry. The planned configuration looks like
this, but it does not work yet:

```toml
# planned, not yet read by any released version of deckd — today's
# settings.toml has a single [registry] url, not a list of sources
[[marketplace]]
name = "Example"
url  = "https://example.org/deckd/index.json"
```

## Foreign sources are not vetted

Once a client supports adding a registry other than this one (also Phase B),
deckd will not check anything about that source beyond the shape of its
`index.json` — the same authenticity chain described above, HTTPS and
`sha256`, and nothing about who runs the server behind it. Adding a foreign
source will ask for a confirmation that says so.

## Contributing a plugin

See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the path from a built plugin to
an entry in `index.json`. Writing the plugin itself is covered by
[`docs/plugin-authoring.md`](docs/plugin-authoring.md); what each function of
the plugin API promises is in [`docs/plugin-api.md`](docs/plugin-api.md).

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option.
