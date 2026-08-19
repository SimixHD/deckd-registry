//! The shape of `index.json`, and how it is read.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The schema number this crate speaks. An index carrying any other number
/// is refused rather than guessed at.
pub const SCHEMA: u32 = 2;

/// One marketplace index, exactly as `index.json` carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    /// Format number; see [`SCHEMA`].
    pub schema: u32,
    /// The day the index last changed, `YYYY-MM-DD`.
    pub updated: String,
    /// Every plugin on offer.
    pub plugins: Vec<Plugin>,
}

/// One plugin, with everything that does not change between its versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plugin {
    /// Reverse-domain id of a domain the author owns. Unique across the index.
    pub id: String,
    /// Display name, in English.
    pub name: String,
    /// One sentence, in English.
    pub description: String,
    /// Author of the plugin.
    pub author: String,
    /// Homepage URL.
    pub homepage: String,
    /// Sorting only. Conventional values: `audio`, `system`, `media`,
    /// `smarthome`, `development`, `other`.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Translations of this plugin's visible text, keyed by language and
    /// then by the **English source sentence** — the same model
    /// `plugin.toml` and `po/de.po` use.
    #[serde(default)]
    pub i18n: BTreeMap<String, BTreeMap<String, String>>,
    /// Newest first. See `Plugin::pick`.
    pub versions: Vec<Version>,
}

/// One released version of a plugin.
///
/// `min_api`, `license` and `capabilities` live here rather than on
/// [`Plugin`] because they are exactly what changes between two releases —
/// and the comparison of two versions' capabilities is what decides whether
/// an update may install itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    /// Semantic version.
    pub version: String,
    /// The lowest plugin-API version this module runs against.
    pub min_api: u32,
    /// SPDX identifier.
    pub license: String,
    /// Capabilities declared by this version.
    #[serde(default)]
    pub capabilities: Capabilities,
    /// The `.wasm`.
    pub module: Artifact,
    /// The `plugin.toml`. Without it there is nothing to install — the
    /// actions, their fields and their translations all live there.
    pub manifest: Artifact,
    /// `Some` once this version has been withdrawn.
    #[serde(default)]
    pub yanked: Option<Yanked>,
    /// Reserved. Always `null` today, and nothing reads it.
    ///
    /// It is here so that adding signatures later is an addition and not a
    /// schema break — a field a client must understand cannot be retrofitted
    /// on the day it is needed.
    #[serde(default)]
    pub signature: Option<String>,
}

/// One downloadable file, pinned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// URL where the artifact can be downloaded.
    pub url: String,
    /// Lowercase hex, 64 characters.
    pub sha256: String,
    /// Size in bytes.
    pub bytes: u64,
}

/// Why a version was withdrawn, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Yanked {
    /// Reason for withdrawal.
    pub reason: String,
    /// Date of withdrawal, `YYYY-MM-DD`.
    pub since: String,
}

/// What a plugin declared it may do.
///
/// The field list mirrors `deck_core::Capabilities` one for one. It has to:
/// the client compares what the index announces against what the installed
/// manifest declared, and a field that exists on one side only would be a
/// permission nobody ever compares.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Command names this plugin may run.
    #[serde(default)]
    pub process: Vec<String>,
    /// Hosts it may reach, `name` or `name:port`.
    #[serde(default)]
    pub http: Vec<String>,
    /// Whether those hosts may resolve to a private address.
    #[serde(default)]
    pub http_private: bool,
    /// Path prefixes it may read outside its own data directory.
    #[serde(default)]
    pub fs_read: Vec<String>,
    /// Whether it may run periodic timers.
    #[serde(default)]
    pub timer: bool,
}

impl Index {
    /// Read an index from JSON.
    ///
    /// Unknown fields are ignored on purpose; see the note on
    /// [`Version::signature`].
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE_PLUGIN: &str = include_str!("../tests/fixtures/one-plugin.json");

    #[test]
    fn an_index_with_one_plugin_reads_back_every_field() {
        let index = Index::from_json(ONE_PLUGIN).expect("the fixture is valid");
        assert_eq!(index.schema, 2);
        assert_eq!(index.plugins.len(), 1);

        let plugin = &index.plugins[0];
        assert_eq!(plugin.id, "dev.simix.audio");
        assert_eq!(plugin.categories, ["audio"]);
        assert_eq!(plugin.i18n["de"]["Audio (PipeWire)"], "Audio (PipeWire)");

        let version = &plugin.versions[0];
        assert_eq!(version.min_api, 1);
        assert_eq!(version.capabilities.process, ["wpctl"]);
        assert!(version.capabilities.timer);
        assert_eq!(version.module.bytes, 10);
        assert_eq!(version.manifest.sha256, "bb");
        assert!(version.yanked.is_none());
    }

    /// The whole reason `signature` exists today: a client that has never
    /// heard of a field must still read the index that carries it. An
    /// index with `deny_unknown_fields` would make every later addition a
    /// breaking change for every installed copy.
    #[test]
    fn a_field_nobody_knows_yet_does_not_stop_the_parser() {
        let json = ONE_PLUGIN.replace(
            r#""signature": null"#,
            r#""signature": null, "attestation": "whatever a later year invents""#,
        );
        let index = Index::from_json(&json).expect("unknown fields are ignored");
        assert_eq!(index.plugins[0].versions[0].version, "1.0.0");
    }

    /// Optional fields are optional. A plugin without categories or
    /// translations is an ordinary plugin, not a broken entry.
    #[test]
    fn the_optional_fields_may_be_left_out_entirely() {
        let json = r#"{
          "schema": 2, "updated": "2026-08-19",
          "plugins": [{
            "id": "org.example.thing", "name": "Thing",
            "description": "d", "author": "a", "homepage": "https://example.org",
            "versions": [{
              "version": "1.0.0", "min_api": 1, "license": "MIT",
              "module":   { "url": "https://example.org/w", "sha256": "aa", "bytes": 1 },
              "manifest": { "url": "https://example.org/t", "sha256": "bb", "bytes": 2 }
            }]
          }]
        }"#;
        let index = Index::from_json(json).expect("only the required fields are required");
        let version = &index.plugins[0].versions[0];
        assert!(index.plugins[0].categories.is_empty());
        assert!(index.plugins[0].i18n.is_empty());
        assert_eq!(version.capabilities, Capabilities::default());
        assert!(version.signature.is_none());
    }

    /// A missing *required* field is an error and not a default, because
    /// the fields that carry the security promise are all required.
    #[test]
    fn a_missing_checksum_is_an_error_and_not_an_empty_string() {
        let json = ONE_PLUGIN.replace(r#""sha256": "aa","#, "");
        assert!(Index::from_json(&json).is_err());
    }
}
