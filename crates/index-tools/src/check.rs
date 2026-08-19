//! The checks that need the files themselves.

use std::fmt;

use deck_index::Index;
use sha2::{Digest, Sha256};

use crate::fetch::Fetcher;

/// No artifact in this registry is anywhere near this large. The limit is
/// here so that a mistyped URL pointing at somebody's disk image fails in a
/// second rather than filling the runner.
const MAX_ARTIFACT_BYTES: u64 = 32 * 1024 * 1024;

/// Something wrong between an entry and the file it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The artifact's URL could not be fetched at all.
    Unreachable {
        /// The plugin the artifact belongs to.
        id: String,
        /// The version the artifact belongs to.
        version: String,
        /// Which artifact — `"module"` or `"manifest"`.
        artifact: &'static str,
        /// Why the fetch failed.
        why: String,
    },
    /// The fetched bytes do not hash to what the entry states.
    ChecksumMismatch {
        /// The plugin the artifact belongs to.
        id: String,
        /// The version the artifact belongs to.
        version: String,
        /// Which artifact — `"module"` or `"manifest"`.
        artifact: &'static str,
        /// The checksum the entry states.
        stated: String,
        /// The checksum the fetched bytes actually hash to.
        actual: String,
    },
    /// The fetched bytes are not the size the entry states.
    WrongSize {
        /// The plugin the artifact belongs to.
        id: String,
        /// The version the artifact belongs to.
        version: String,
        /// Which artifact — `"module"` or `"manifest"`.
        artifact: &'static str,
        /// The size in bytes the entry states.
        stated: u64,
        /// The size in bytes the fetched bytes actually are.
        actual: u64,
    },
    /// The manifest's bytes matched their checksum but could not be parsed
    /// as `plugin.toml`.
    ManifestUnreadable {
        /// The plugin the manifest belongs to.
        id: String,
        /// The version the manifest belongs to.
        version: String,
        /// Why the manifest could not be read.
        why: String,
    },
    /// The manifest disagrees with the index entry on a field the two are
    /// supposed to share.
    ManifestDisagrees {
        /// The plugin the manifest belongs to.
        id: String,
        /// The version the manifest belongs to.
        version: String,
        /// Which field disagreed — `"id"`, `"version"`, `"min_api"`, or one
        /// of the five `"capabilities.*"` sub-fields, so a capability
        /// mismatch names the one permission that differs rather than
        /// dumping the whole struct on both sides. `"min_api"` is the
        /// index's name for it; the manifest calls the same number `api`.
        field: &'static str,
        /// What the index entry says.
        index: String,
        /// What the manifest says.
        manifest: String,
    },
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable {
                id,
                version,
                artifact,
                why,
            } => {
                write!(
                    f,
                    "{id} {version}: the {artifact} could not be fetched: {why}"
                )
            }
            Self::ChecksumMismatch {
                id,
                version,
                artifact,
                stated,
                actual,
            } => {
                write!(
                    f,
                    "{id} {version}: the {artifact} hashes to {actual}, the entry says {stated}"
                )
            }
            Self::WrongSize {
                id,
                version,
                artifact,
                stated,
                actual,
            } => {
                write!(
                    f,
                    "{id} {version}: the {artifact} is {actual} bytes, the entry says {stated}"
                )
            }
            Self::ManifestUnreadable { id, version, why } => {
                write!(f, "{id} {version}: the manifest could not be read: {why}")
            }
            Self::ManifestDisagrees {
                id,
                version,
                field,
                index,
                manifest,
            } => write!(
                f,
                "{id} {version}: the index says {field} is {index}, the manifest says {manifest}"
            ),
        }
    }
}

/// Just enough of `plugin.toml` to hold it against the index entry.
///
/// Deliberately not `deck_core::Manifest`: that type lives in the private
/// repository and cannot be depended on from here. What is compared is
/// exactly the four things the index carries itself — everything else in a
/// manifest is the client's business, and the client will parse it in full
/// before it installs anything.
#[derive(serde::Deserialize)]
struct ManifestExcerpt {
    /// The plugin id, as the manifest names it.
    id: String,
    /// The version, as the manifest names it.
    version: String,
    /// The manifest API this module was built against — what the index
    /// calls `min_api`. Not `#[serde(default)]`: the daemon's own manifest
    /// type requires the field, so a manifest without it is one no client
    /// can load, and the honest report for that is "the manifest could not
    /// be read" rather than a silent 0 compared against the entry.
    api: u32,
    /// The capabilities the manifest declares.
    #[serde(default)]
    capabilities: deck_index::Capabilities,
}

/// Fetch every artifact this index names and hold it against its entry.
pub fn check_artifacts(index: &Index, net: &dyn Fetcher) -> Vec<Fault> {
    let mut faults = Vec::new();

    for plugin in &index.plugins {
        for version in &plugin.versions {
            for (artifact, what) in [(&version.module, "module"), (&version.manifest, "manifest")] {
                let bytes = match net.fetch(&artifact.url, MAX_ARTIFACT_BYTES) {
                    Ok(bytes) => bytes,
                    Err(why) => {
                        faults.push(Fault::Unreachable {
                            id: plugin.id.clone(),
                            version: version.version.clone(),
                            artifact: what,
                            why,
                        });
                        continue;
                    }
                };

                let actual = format!("{:x}", Sha256::digest(&bytes));
                let actual_matches_entry = actual == artifact.sha256;
                if !actual_matches_entry {
                    faults.push(Fault::ChecksumMismatch {
                        id: plugin.id.clone(),
                        version: version.version.clone(),
                        artifact: what,
                        stated: artifact.sha256.clone(),
                        actual,
                    });
                }
                if bytes.len() as u64 != artifact.bytes {
                    faults.push(Fault::WrongSize {
                        id: plugin.id.clone(),
                        version: version.version.clone(),
                        artifact: what,
                        stated: artifact.bytes,
                        actual: bytes.len() as u64,
                    });
                }

                if what == "manifest" && actual_matches_entry {
                    match toml::from_str::<ManifestExcerpt>(&String::from_utf8_lossy(&bytes)) {
                        Err(err) => faults.push(Fault::ManifestUnreadable {
                            id: plugin.id.clone(),
                            version: version.version.clone(),
                            why: err.to_string(),
                        }),
                        Ok(manifest) => {
                            if manifest.id != plugin.id {
                                faults.push(Fault::ManifestDisagrees {
                                    id: plugin.id.clone(),
                                    version: version.version.clone(),
                                    field: "id",
                                    index: plugin.id.clone(),
                                    manifest: manifest.id.clone(),
                                });
                            }
                            if manifest.version != version.version {
                                faults.push(Fault::ManifestDisagrees {
                                    id: plugin.id.clone(),
                                    version: version.version.clone(),
                                    field: "version",
                                    index: version.version.clone(),
                                    manifest: manifest.version.clone(),
                                });
                            }
                            // The field `pick` decides on, and the only
                            // reason the index carries a list of versions
                            // at all. An entry claiming `min_api: 1` for a
                            // module built against API 2 is offered to
                            // every API-1 client, downloaded by all of
                            // them, and then refused by their own daemon —
                            // a fault that costs a download to discover
                            // and looks like a broken client rather than a
                            // wrong entry.
                            if manifest.api != version.min_api {
                                faults.push(Fault::ManifestDisagrees {
                                    id: plugin.id.clone(),
                                    version: version.version.clone(),
                                    field: "min_api",
                                    index: version.min_api.to_string(),
                                    manifest: manifest.api.to_string(),
                                });
                            }
                            if manifest.capabilities.process != version.capabilities.process {
                                faults.push(Fault::ManifestDisagrees {
                                    id: plugin.id.clone(),
                                    version: version.version.clone(),
                                    field: "capabilities.process",
                                    index: format!("{:?}", version.capabilities.process),
                                    manifest: format!("{:?}", manifest.capabilities.process),
                                });
                            }
                            if manifest.capabilities.http != version.capabilities.http {
                                faults.push(Fault::ManifestDisagrees {
                                    id: plugin.id.clone(),
                                    version: version.version.clone(),
                                    field: "capabilities.http",
                                    index: format!("{:?}", version.capabilities.http),
                                    manifest: format!("{:?}", manifest.capabilities.http),
                                });
                            }
                            if manifest.capabilities.http_private
                                != version.capabilities.http_private
                            {
                                faults.push(Fault::ManifestDisagrees {
                                    id: plugin.id.clone(),
                                    version: version.version.clone(),
                                    field: "capabilities.http_private",
                                    index: format!("{:?}", version.capabilities.http_private),
                                    manifest: format!("{:?}", manifest.capabilities.http_private),
                                });
                            }
                            if manifest.capabilities.fs_read != version.capabilities.fs_read {
                                faults.push(Fault::ManifestDisagrees {
                                    id: plugin.id.clone(),
                                    version: version.version.clone(),
                                    field: "capabilities.fs_read",
                                    index: format!("{:?}", version.capabilities.fs_read),
                                    manifest: format!("{:?}", manifest.capabilities.fs_read),
                                });
                            }
                            if manifest.capabilities.timer != version.capabilities.timer {
                                faults.push(Fault::ManifestDisagrees {
                                    id: plugin.id.clone(),
                                    version: version.version.clone(),
                                    field: "capabilities.timer",
                                    index: format!("{:?}", version.capabilities.timer),
                                    manifest: format!("{:?}", manifest.capabilities.timer),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    faults
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use deck_index::Index;

    use super::{Fault, check_artifacts};
    use crate::fetch::Fetcher;

    /// Every byte these tests fetch comes from this map. Nothing here opens
    /// a socket: a test that needs the network is a test that goes red on a
    /// bad day without anybody having broken anything.
    #[derive(Default)]
    struct Canned(HashMap<String, Vec<u8>>);

    impl Canned {
        fn with(mut self, url: &str, body: &[u8]) -> Self {
            self.0.insert(url.to_owned(), body.to_vec());
            self
        }
    }

    impl Fetcher for Canned {
        fn fetch(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
            let body = self.0.get(url).cloned().ok_or_else(|| "404".to_owned())?;
            if body.len() as u64 > max {
                return Err(format!(
                    "body is {} bytes, over the {max} byte limit",
                    body.len()
                ));
            }
            Ok(body)
        }
    }

    /// A manifest that agrees with the fixture `index_for` builds — used
    /// wherever a test's point is the *module*, not the manifest, so the
    /// manifest check has nothing to say and stays out of the way.
    const OK_MANIFEST: &str = "id = \"org.example.thing\"\nversion = \"1.0.0\"\napi = 1\n";

    fn index_for(module_sha: &str, bytes: u64) -> Index {
        Index::from_json(&format!(
            r#"{{"schema":2,"updated":"2026-08-19","plugins":[{{
              "id":"org.example.thing","name":"N","description":"d","author":"a",
              "homepage":"https://example.org","versions":[{{
                "version":"1.0.0","min_api":1,"license":"MIT",
                "module":{{"url":"https://example.org/w","sha256":"{module_sha}","bytes":{bytes}}},
                "manifest":{{"url":"https://example.org/t","sha256":"{}","bytes":{}}}
              }}]}}]}}"#,
            sha256_hex(OK_MANIFEST.as_bytes()),
            OK_MANIFEST.len()
        ))
        .expect("the fixture parses")
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes))
    }

    #[test]
    fn an_index_whose_files_match_their_entries_reports_nothing() {
        let index = index_for(&sha256_hex(b"wasm"), 4);
        let net = Canned::default()
            .with("https://example.org/w", b"wasm")
            .with("https://example.org/t", OK_MANIFEST.as_bytes());
        assert!(check_artifacts(&index, &net).is_empty());
    }

    #[test]
    fn a_url_that_answers_with_nothing_is_reported() {
        let index = index_for(&sha256_hex(b"wasm"), 4);
        let net = Canned::default().with("https://example.org/t", OK_MANIFEST.as_bytes());
        assert!(matches!(
            check_artifacts(&index, &net).as_slice(),
            [Fault::Unreachable {
                artifact: "module",
                ..
            }]
        ));
    }

    /// The one check the whole security story rests on.
    #[test]
    fn a_file_that_does_not_hash_to_its_entry_is_reported() {
        let index = index_for(&sha256_hex(b"wasm"), 4);
        let net = Canned::default()
            .with("https://example.org/w", b"something else entirely")
            .with("https://example.org/t", OK_MANIFEST.as_bytes());
        assert!(
            check_artifacts(&index, &net)
                .iter()
                .any(|fault| matches!(fault, Fault::ChecksumMismatch { .. }))
        );
    }

    /// `bytes` is what the interface shows before anyone clicks. A wrong
    /// number is not dangerous, it is dishonest — and it is free to catch.
    #[test]
    fn a_size_that_does_not_match_the_file_is_reported() {
        let index = index_for(&sha256_hex(b"wasm"), 999);
        let net = Canned::default()
            .with("https://example.org/w", b"wasm")
            .with("https://example.org/t", OK_MANIFEST.as_bytes());
        assert!(matches!(
            check_artifacts(&index, &net).as_slice(),
            [Fault::WrongSize {
                stated: 999,
                actual: 4,
                ..
            }]
        ));
    }

    #[test]
    fn an_empty_index_needs_no_network_at_all() {
        let index =
            Index::from_json(r#"{"schema":2,"updated":"2026-08-19","plugins":[]}"#).unwrap();
        assert!(check_artifacts(&index, &Canned::default()).is_empty());
    }

    /// `MAX_ARTIFACT_BYTES` is a safeguard against a mistyped URL, not a
    /// number that only ever gets read by a human. `Canned` enforces it the
    /// same way the real `Http` fetcher's `ureq` limit reader does, so this
    /// is the one test in the crate that proves the cap actually bites
    /// rather than being silently truncated or ignored.
    #[test]
    fn a_body_over_the_limit_is_reported_unreachable_and_names_the_size() {
        let index = index_for(&sha256_hex(b"wasm"), 4);
        let oversized = vec![0u8; (super::MAX_ARTIFACT_BYTES + 1) as usize];
        let net = Canned::default()
            .with("https://example.org/w", &oversized)
            .with("https://example.org/t", OK_MANIFEST.as_bytes());
        assert!(matches!(
            check_artifacts(&index, &net).as_slice(),
            [Fault::Unreachable { artifact: "module", why, .. }]
                if why.contains(&(super::MAX_ARTIFACT_BYTES + 1).to_string())
        ));
    }

    /// An index whose manifest entry describes `manifest` exactly: the
    /// `sha256` and the `bytes` are computed from that very text, so the
    /// fetched bytes match their entry and the field-by-field comparison —
    /// which runs only on bytes that already matched — is the only thing
    /// left that can report anything.
    fn index_with_manifest_text(caps: &str, manifest: &str) -> Index {
        Index::from_json(&format!(
            r#"{{"schema":2,"updated":"2026-08-19","plugins":[{{
              "id":"org.example.thing","name":"N","description":"d","author":"a",
              "homepage":"https://example.org","versions":[{{
                "version":"1.0.0","min_api":1,"license":"MIT",
                "capabilities":{caps},
                "module":{{"url":"https://example.org/w","sha256":"{}","bytes":4}},
                "manifest":{{"url":"https://example.org/t","sha256":"{}","bytes":{}}}
              }}]}}]}}"#,
            sha256_hex(b"wasm"),
            sha256_hex(manifest.as_bytes()),
            manifest.len()
        ))
        .expect("the fixture parses")
    }

    fn index_with_manifest(caps: &str) -> Index {
        index_with_manifest_text(caps, MANIFEST)
    }

    const MANIFEST: &str = r#"id = "org.example.thing"
name = "Thing"
version = "1.0.0"
api = 1

[capabilities]
process = ["wpctl"]
timer = true
"#;

    /// The capabilities `MANIFEST` declares, spelled the way an index entry
    /// spells them — so a test whose point is some *other* field has
    /// nothing to say about capabilities.
    const OK_CAPS: &str = r#"{"process":["wpctl"],"timer":true}"#;

    fn net_with_manifest() -> Canned {
        Canned::default()
            .with("https://example.org/w", b"wasm")
            .with("https://example.org/t", MANIFEST.as_bytes())
    }

    #[test]
    fn a_manifest_that_agrees_with_its_entry_reports_nothing() {
        let index = index_with_manifest(OK_CAPS);
        assert!(check_artifacts(&index, &net_with_manifest()).is_empty());
    }

    /// The whole point of checking twice. An entry that advertises fewer
    /// permissions than the module actually declares would show the user
    /// one thing and let the daemon enforce another. Here the entry omits
    /// `timer`, so `timer` is exactly the field that must be named.
    #[test]
    fn an_entry_that_understates_the_permissions_is_reported() {
        let index = index_with_manifest(r#"{"process":["wpctl"]}"#);
        assert!(
            check_artifacts(&index, &net_with_manifest())
                .iter()
                .any(|fault| matches!(
                    fault,
                    Fault::ManifestDisagrees {
                        field: "capabilities.timer",
                        ..
                    }
                ))
        );
    }

    /// The entry claims an extra `process` entry (`"rm"`) the manifest
    /// never declared, so `process` is exactly the field that must be
    /// named.
    #[test]
    fn an_entry_that_overstates_the_permissions_is_reported_too() {
        let index = index_with_manifest(r#"{"process":["wpctl","rm"],"timer":true}"#);
        assert!(
            check_artifacts(&index, &net_with_manifest())
                .iter()
                .any(|fault| matches!(
                    fault,
                    Fault::ManifestDisagrees {
                        field: "capabilities.process",
                        ..
                    }
                ))
        );
    }

    /// Pins all five `Capabilities` sub-fields as independently detected:
    /// each entry differs from `net_with_manifest`'s manifest
    /// (`process=["wpctl"], timer=true`, all else default) in exactly one
    /// field, and the fault names that one field, not the whole struct.
    #[test]
    fn each_capability_sub_field_disagreeing_alone_is_named() {
        let cases: [(&str, &str); 5] = [
            (
                r#"{"process":["other"],"timer":true}"#,
                "capabilities.process",
            ),
            (
                r#"{"process":["wpctl"],"timer":true,"http":["example.com"]}"#,
                "capabilities.http",
            ),
            (
                r#"{"process":["wpctl"],"timer":true,"http_private":true}"#,
                "capabilities.http_private",
            ),
            (
                r#"{"process":["wpctl"],"timer":true,"fs_read":["/etc"]}"#,
                "capabilities.fs_read",
            ),
            (
                r#"{"process":["wpctl"],"timer":false}"#,
                "capabilities.timer",
            ),
        ];
        for (caps, expected_field) in cases {
            let index = index_with_manifest(caps);
            let faults = check_artifacts(&index, &net_with_manifest());
            assert!(
                faults
                    .iter()
                    .any(|fault| matches!(fault, Fault::ManifestDisagrees { field, .. } if *field == expected_field)),
                "expected a fault naming {expected_field:?} for capabilities {caps}, got {faults:?}"
            );
        }
    }

    /// Two fields disagreeing at once must produce two faults, not one —
    /// the same "every problem is reported" rule the `id`/`version` checks
    /// already follow.
    #[test]
    fn two_capability_fields_disagreeing_produces_two_faults() {
        let index = index_with_manifest(r#"{"process":["other"],"timer":false}"#);
        let faults = check_artifacts(&index, &net_with_manifest());
        let disagreements: Vec<&Fault> = faults
            .iter()
            .filter(|fault| matches!(fault, Fault::ManifestDisagrees { .. }))
            .collect();
        assert_eq!(disagreements.len(), 2, "faults were: {faults:?}");
        assert!(disagreements.iter().any(|fault| matches!(
            fault,
            Fault::ManifestDisagrees {
                field: "capabilities.process",
                ..
            }
        )));
        assert!(disagreements.iter().any(|fault| matches!(
            fault,
            Fault::ManifestDisagrees {
                field: "capabilities.timer",
                ..
            }
        )));
    }

    /// The bytes hash to their entry, so the `id` comparison is the only
    /// rule left that can speak — and it must, or an entry could point at
    /// a module belonging to an entirely different plugin.
    #[test]
    fn a_manifest_naming_a_different_id_is_reported() {
        let other = MANIFEST.replace("org.example.thing", "org.example.other");
        let index = index_with_manifest_text(OK_CAPS, &other);
        let net = Canned::default()
            .with("https://example.org/w", b"wasm")
            .with("https://example.org/t", other.as_bytes());
        let faults = check_artifacts(&index, &net);
        assert!(
            matches!(
                faults.as_slice(),
                [Fault::ManifestDisagrees { field: "id", index, manifest, .. }]
                    if index == "org.example.thing" && manifest == "org.example.other"
            ),
            "got {faults:?}"
        );
    }

    /// The same for `version`: an entry that says 1.0.0 while the manifest
    /// behind it says 2.0.0 publishes a release under the wrong number, and
    /// no checksum notices — both files are exactly what the entry names.
    #[test]
    fn a_manifest_naming_a_different_version_is_reported() {
        let other = MANIFEST.replace("1.0.0", "2.0.0");
        let index = index_with_manifest_text(OK_CAPS, &other);
        let net = Canned::default()
            .with("https://example.org/w", b"wasm")
            .with("https://example.org/t", other.as_bytes());
        let faults = check_artifacts(&index, &net);
        assert!(
            matches!(
                faults.as_slice(),
                [Fault::ManifestDisagrees { field: "version", index, manifest, .. }]
                    if index == "1.0.0" && manifest == "2.0.0"
            ),
            "got {faults:?}"
        );
    }

    /// The index calls it `min_api`, the manifest calls it `api`, and they
    /// are the same number. An entry understating it is the one mismatch
    /// nothing downstream can recover from: every client whose API level
    /// reaches the entry's claim is offered the version, downloads both
    /// files, and is then refused by its own daemon.
    #[test]
    fn a_manifest_built_against_another_api_than_the_entry_claims_is_reported() {
        let other = MANIFEST.replace("api = 1", "api = 2");
        let index = index_with_manifest_text(OK_CAPS, &other);
        let net = Canned::default()
            .with("https://example.org/w", b"wasm")
            .with("https://example.org/t", other.as_bytes());
        let faults = check_artifacts(&index, &net);
        assert!(
            matches!(
                faults.as_slice(),
                [Fault::ManifestDisagrees { field: "min_api", index, manifest, .. }]
                    if index == "1" && manifest == "2"
            ),
            "got {faults:?}"
        );
    }

    /// A manifest without `api` at all is not a manifest with `api = 0`:
    /// the daemon's own type requires the field, so the honest answer is
    /// that the file cannot be read, not a comparison against a number
    /// nobody wrote.
    #[test]
    fn a_manifest_with_no_api_field_is_reported_as_unreadable() {
        let other = MANIFEST.replace("api = 1\n", "");
        let index = index_with_manifest_text(OK_CAPS, &other);
        let net = Canned::default()
            .with("https://example.org/w", b"wasm")
            .with("https://example.org/t", other.as_bytes());
        assert!(
            check_artifacts(&index, &net)
                .iter()
                .any(|fault| matches!(fault, Fault::ManifestUnreadable { .. }))
        );
    }

    /// The comparison runs only on bytes that already matched their
    /// checksum, and this pins that order: a manifest that hashes to
    /// something else is reported for *that* and nothing more. Reporting a
    /// field disagreement as well would name a file the entry does not
    /// actually point at, and send a contributor to fix the wrong one of
    /// the two.
    #[test]
    fn a_manifest_that_fails_its_checksum_is_not_also_compared_field_by_field() {
        let index = index_with_manifest(OK_CAPS);
        let other = MANIFEST.replace("org.example.thing", "org.example.other");
        let net = Canned::default()
            .with("https://example.org/w", b"wasm")
            .with("https://example.org/t", other.as_bytes());
        let faults = check_artifacts(&index, &net);
        assert!(
            faults
                .iter()
                .any(|fault| matches!(fault, Fault::ChecksumMismatch { .. }))
        );
        assert!(
            !faults
                .iter()
                .any(|fault| matches!(fault, Fault::ManifestDisagrees { .. })),
            "got {faults:?}"
        );
    }

    #[test]
    fn a_manifest_that_is_not_toml_at_all_is_reported() {
        let broken = "this is not toml {{{";
        let index = Index::from_json(&format!(
            r#"{{"schema":2,"updated":"2026-08-19","plugins":[{{
              "id":"org.example.thing","name":"N","description":"d","author":"a",
              "homepage":"https://example.org","versions":[{{
                "version":"1.0.0","min_api":1,"license":"MIT",
                "module":{{"url":"https://example.org/w","sha256":"{}","bytes":4}},
                "manifest":{{"url":"https://example.org/t","sha256":"{}","bytes":{}}}
              }}]}}]}}"#,
            sha256_hex(b"wasm"),
            sha256_hex(broken.as_bytes()),
            broken.len()
        ))
        .unwrap();
        let net = Canned::default()
            .with("https://example.org/w", b"wasm")
            .with("https://example.org/t", broken.as_bytes());
        assert!(
            check_artifacts(&index, &net)
                .iter()
                .any(|fault| matches!(fault, Fault::ManifestUnreadable { .. }))
        );
    }
}
