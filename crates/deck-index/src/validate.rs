//! Every rule that can be decided from the index alone.
//!
//! What needs a network — is that URL reachable, does the file behind it
//! hash to what the entry claims — lives in `index-tools`. The split is what
//! keeps this crate testable from memory, and it is also the split between
//! "the registry rejects your pull request" and "your client refuses to
//! install": a client has the index and nothing else at the moment it
//! decides whether the index makes sense.

use std::collections::BTreeSet;
use std::fmt;

use crate::{Index, SCHEMA};

/// The namespace the registry keeps for its own plugins.
pub const RESERVED_NAMESPACE: &str = "dev.simix.";

/// Something wrong with an index, in words a contributor can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The schema version is not understood by this crate.
    UnknownSchema {
        /// The schema version found in the index.
        found: u32,
    },
    /// A plugin ID appears more than once.
    DuplicateId {
        /// The plugin ID that is duplicated.
        id: String,
    },
    /// A plugin ID is not a reverse domain.
    NotReverseDomain {
        /// The plugin ID that is not a reverse domain.
        id: String,
    },
    /// A plugin ID uses the reserved namespace.
    ReservedNamespace {
        /// The plugin ID using the reserved namespace.
        id: String,
    },
    /// A plugin has no versions.
    NoVersions {
        /// The plugin ID with no versions.
        id: String,
    },
    /// A version string is not semantic.
    NotSemver {
        /// The plugin ID containing the non-semantic version.
        id: String,
        /// The non-semantic version string.
        version: String,
    },
    /// A version appears more than once.
    DuplicateVersion {
        /// The plugin ID with the duplicate version.
        id: String,
        /// The version that is duplicated.
        version: String,
    },
    /// Versions are not sorted newest first.
    VersionsOutOfOrder {
        /// The plugin ID with out-of-order versions.
        id: String,
        /// The earlier version in the list.
        earlier: String,
        /// The later version that should come first.
        later: String,
    },
    /// A checksum is not 64 hex characters.
    BadChecksum {
        /// The plugin ID with the invalid checksum.
        id: String,
        /// The version with the invalid checksum.
        version: String,
        /// The artifact type ("module" or "manifest").
        artifact: &'static str,
    },
    /// A URL is not HTTPS.
    NotHttps {
        /// The plugin ID with the non-HTTPS URL.
        id: String,
        /// The version with the non-HTTPS URL.
        version: String,
        /// The artifact type ("module" or "manifest").
        artifact: &'static str,
    },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSchema { found } => {
                write!(f, "index says schema {found}, this tool speaks {SCHEMA}")
            }
            Self::DuplicateId { id } => write!(f, "{id}: listed twice"),
            Self::NotReverseDomain { id } => write!(
                f,
                "{id}: an id is a reverse domain of a domain you own, such as org.example.thing"
            ),
            Self::ReservedNamespace { id } => write!(
                f,
                "{id}: {RESERVED_NAMESPACE}* is reserved for the registry's own plugins"
            ),
            Self::NoVersions { id } => write!(f, "{id}: no versions listed"),
            Self::NotSemver { id, version } => write!(f, "{id} {version}: not a semantic version"),
            Self::DuplicateVersion { id, version } => write!(f, "{id} {version}: listed twice"),
            Self::VersionsOutOfOrder { id, earlier, later } => write!(
                f,
                "{id}: {earlier} is listed above {later}; versions go newest first"
            ),
            Self::BadChecksum {
                id,
                version,
                artifact,
            } => write!(
                f,
                "{id} {version}: the {artifact} sha256 is not 64 hex characters"
            ),
            Self::NotHttps {
                id,
                version,
                artifact,
            } => write!(f, "{id} {version}: the {artifact} url is not https"),
        }
    }
}

fn looks_like_a_reverse_domain(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64
        && text
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

impl Index {
    /// Every problem with this index, for an index submitted from outside.
    pub fn check(&self) -> Vec<Problem> {
        self.check_inner(false)
    }

    /// The same, for the registry's own pull requests: the reserved
    /// namespace is allowed here and nowhere else.
    pub fn check_allowing_reserved(&self) -> Vec<Problem> {
        self.check_inner(true)
    }

    fn check_inner(&self, reserved_allowed: bool) -> Vec<Problem> {
        let mut problems = Vec::new();

        if self.schema != SCHEMA {
            // Nothing below can be trusted to mean what this crate thinks
            // it means, so this is the one problem reported alone.
            return vec![Problem::UnknownSchema { found: self.schema }];
        }

        let mut seen_ids = BTreeSet::new();
        for plugin in &self.plugins {
            let id = plugin.id.clone();
            if !seen_ids.insert(id.clone()) {
                problems.push(Problem::DuplicateId { id: id.clone() });
            }
            if !looks_like_a_reverse_domain(&id) {
                problems.push(Problem::NotReverseDomain { id: id.clone() });
            } else if id.starts_with(RESERVED_NAMESPACE) && !reserved_allowed {
                problems.push(Problem::ReservedNamespace { id: id.clone() });
            }
            if plugin.versions.is_empty() {
                problems.push(Problem::NoVersions { id: id.clone() });
            }

            let mut seen_versions = BTreeSet::new();
            let mut previous: Option<semver::Version> = None;
            for version in &plugin.versions {
                let number = version.version.clone();
                if !seen_versions.insert(number.clone()) {
                    problems.push(Problem::DuplicateVersion {
                        id: id.clone(),
                        version: number.clone(),
                    });
                }
                match semver::Version::parse(&number) {
                    Err(_) => problems.push(Problem::NotSemver {
                        id: id.clone(),
                        version: number.clone(),
                    }),
                    Ok(parsed) => {
                        if let Some(earlier) = &previous
                            && *earlier < parsed
                        {
                            problems.push(Problem::VersionsOutOfOrder {
                                id: id.clone(),
                                earlier: earlier.to_string(),
                                later: parsed.to_string(),
                            });
                        }
                        previous = Some(parsed);
                    }
                }

                for (artifact, what) in
                    [(&version.module, "module"), (&version.manifest, "manifest")]
                {
                    if !is_sha256(&artifact.sha256) {
                        problems.push(Problem::BadChecksum {
                            id: id.clone(),
                            version: number.clone(),
                            artifact: what,
                        });
                    }
                    if !artifact.url.starts_with("https://") {
                        problems.push(Problem::NotHttps {
                            id: id.clone(),
                            version: number.clone(),
                            artifact: what,
                        });
                    }
                }
            }
        }

        problems
    }
}

#[cfg(test)]
mod tests {
    use crate::{Index, Problem};

    fn index(plugins: &str) -> Index {
        Index::from_json(&format!(
            r#"{{"schema":2,"updated":"2026-08-19","plugins":[{plugins}]}}"#
        ))
        .expect("the fixture parses")
    }

    fn plugin(id: &str, versions: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"N","description":"d","author":"a",
                 "homepage":"https://example.org","versions":[{versions}]}}"#
        )
    }

    fn version(number: &str) -> String {
        format!(
            r#"{{"version":"{number}","min_api":1,"license":"MIT",
                 "module":{{"url":"https://example.org/w","sha256":"{}","bytes":1}},
                 "manifest":{{"url":"https://example.org/t","sha256":"{}","bytes":2}}}}"#,
            "a".repeat(64),
            "b".repeat(64)
        )
    }

    #[test]
    fn an_empty_index_has_nothing_to_complain_about() {
        assert!(index("").check().is_empty());
    }

    #[test]
    fn a_well_formed_entry_has_nothing_to_complain_about() {
        let json = plugin(
            "org.example.thing",
            &format!("{},{}", version("2.0.0"), version("1.0.0")),
        );
        assert!(index(&json).check().is_empty());
    }

    #[test]
    fn a_schema_number_this_crate_does_not_speak_is_refused() {
        let mut index = index("");
        index.schema = 3;
        assert!(matches!(
            index.check().as_slice(),
            [Problem::UnknownSchema { found: 3 }]
        ));
    }

    /// `pick` walks the list from the top and stops at the first fit. An
    /// ascending list would hand out the *oldest* usable version to every
    /// client, silently, forever — so the ordering is a rule and not a
    /// convention.
    #[test]
    fn versions_out_of_order_are_refused_because_pick_reads_from_the_top() {
        let json = plugin(
            "org.example.thing",
            &format!("{},{}", version("1.0.0"), version("2.0.0")),
        );
        assert!(matches!(
            index(&json).check().as_slice(),
            [Problem::VersionsOutOfOrder { id, .. }] if id == "org.example.thing"
        ));
    }

    #[test]
    fn the_same_version_twice_is_refused() {
        let json = plugin(
            "org.example.thing",
            &format!("{},{}", version("1.0.0"), version("1.0.0")),
        );
        assert!(matches!(
            index(&json).check().as_slice(),
            [Problem::DuplicateVersion { .. }]
        ));
    }

    #[test]
    fn a_version_that_is_not_semantic_is_refused() {
        let json = plugin(
            "org.example.thing",
            r#"{"version":"the good one","min_api":1,
            "license":"MIT",
            "module":{"url":"https://example.org/w","sha256":"aa","bytes":1},
            "manifest":{"url":"https://example.org/t","sha256":"bb","bytes":2}}"#,
        );
        assert!(
            index(&json)
                .check()
                .iter()
                .any(|p| matches!(p, Problem::NotSemver { .. }))
        );
    }

    #[test]
    fn two_plugins_with_the_same_id_are_refused() {
        let json = format!(
            "{},{}",
            plugin("org.example.thing", &version("1.0.0")),
            plugin("org.example.thing", &version("1.0.0"))
        );
        assert!(matches!(
            index(&json).check().as_slice(),
            [Problem::DuplicateId { .. }]
        ));
    }

    #[test]
    fn an_id_that_is_not_a_reverse_domain_is_refused() {
        let json = plugin("audio", &version("1.0.0"));
        assert!(matches!(
            index(&json).check().as_slice(),
            [Problem::NotReverseDomain { .. }]
        ));
    }

    /// The reserved namespace. `check` refuses it for everyone; the
    /// workflow in Task 8 is what lets the registry's own pull requests
    /// through.
    #[test]
    fn the_reserved_namespace_is_refused_by_default() {
        let json = plugin("dev.simix.audio", &version("1.0.0"));
        assert!(matches!(
            index(&json).check().as_slice(),
            [Problem::ReservedNamespace { .. }]
        ));
    }

    #[test]
    fn the_reserved_namespace_passes_when_it_was_allowed() {
        let json = plugin("dev.simix.audio", &version("1.0.0"));
        assert!(index(&json).check_allowing_reserved().is_empty());
    }

    #[test]
    fn a_checksum_that_is_not_sixty_four_hex_characters_is_refused() {
        let json = plugin(
            "org.example.thing",
            r#"{"version":"1.0.0","min_api":1,
            "license":"MIT",
            "module":{"url":"https://example.org/w","sha256":"nope","bytes":1},
            "manifest":{"url":"https://example.org/t","sha256":"bb","bytes":2}}"#,
        );
        assert_eq!(index(&json).check().len(), 2, "one per artifact");
        assert!(
            index(&json)
                .check()
                .iter()
                .all(|p| matches!(p, Problem::BadChecksum { .. }))
        );
    }

    /// http:// would make the pinned checksum the only thing standing
    /// between a user and whatever the network felt like returning.
    #[test]
    fn a_download_url_that_is_not_https_is_refused() {
        let json = plugin(
            "org.example.thing",
            &version("1.0.0").replace("https://", "http://"),
        );
        assert!(
            index(&json)
                .check()
                .iter()
                .any(|p| matches!(p, Problem::NotHttps { .. }))
        );
    }

    /// Every problem is reported, not just the first — a contributor
    /// should fix one pull request, not five in a row.
    #[test]
    fn several_problems_are_all_reported_at_once() {
        let json = plugin(
            "audio",
            &format!("{},{}", version("1.0.0"), version("2.0.0")),
        );
        assert_eq!(index(&json).check().len(), 2);
    }
}
