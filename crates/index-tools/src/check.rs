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
        }
    }
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
                if actual != artifact.sha256 {
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

    fn index_for(module_sha: &str, bytes: u64) -> Index {
        Index::from_json(&format!(
            r#"{{"schema":2,"updated":"2026-08-19","plugins":[{{
              "id":"org.example.thing","name":"N","description":"d","author":"a",
              "homepage":"https://example.org","versions":[{{
                "version":"1.0.0","min_api":1,"license":"MIT",
                "module":{{"url":"https://example.org/w","sha256":"{module_sha}","bytes":{bytes}}},
                "manifest":{{"url":"https://example.org/t","sha256":"{}","bytes":4}}
              }}]}}]}}"#,
            sha256_hex(b"toml")
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
            .with("https://example.org/t", b"toml");
        assert!(check_artifacts(&index, &net).is_empty());
    }

    #[test]
    fn a_url_that_answers_with_nothing_is_reported() {
        let index = index_for(&sha256_hex(b"wasm"), 4);
        let net = Canned::default().with("https://example.org/t", b"toml");
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
            .with("https://example.org/t", b"toml");
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
            .with("https://example.org/t", b"toml");
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
            .with("https://example.org/t", b"toml");
        assert!(matches!(
            check_artifacts(&index, &net).as_slice(),
            [Fault::Unreachable { artifact: "module", why, .. }]
                if why.contains(&(super::MAX_ARTIFACT_BYTES + 1).to_string())
        ));
    }
}
