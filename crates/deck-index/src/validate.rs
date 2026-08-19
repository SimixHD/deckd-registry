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
    /// A plugin ID is not shaped like a reverse domain: fewer than three
    /// dot-separated parts, or one of them empty.
    NotReverseDomain {
        /// The plugin ID that is not a reverse domain.
        id: String,
    },
    /// A plugin ID holds a character an id may not hold.
    ///
    /// Separate from [`Problem::NotReverseDomain`] rather than folded into
    /// it: an id can be perfectly shaped and still carry a capital letter,
    /// and a contributor told only "an id is a reverse domain" would read
    /// that as a description of the id they already wrote.
    BadIdCharacter {
        /// The plugin ID holding the character.
        id: String,
        /// The first character that is not allowed.
        character: char,
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
    /// A plugin's homepage URL is not HTTPS.
    ///
    /// Separate from [`Problem::NotHttps`] rather than reusing it: a
    /// homepage has no version and no artifact kind, so it does not fit
    /// that variant's `{ id, version, artifact }` shape.
    HomepageNotHttps {
        /// The plugin ID whose homepage URL is not HTTPS.
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
    /// A checksum is 64 hex characters, but some of them are uppercase.
    ///
    /// Its own variant, because "not 64 hex characters" is exactly what an
    /// uppercase checksum *is not* — a contributor holding a correct digest
    /// typed in capitals would count the characters, find 64 of them, and
    /// have nowhere to go.
    UppercaseChecksum {
        /// The plugin ID with the uppercase checksum.
        id: String,
        /// The version with the uppercase checksum.
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
                "{id}: an id needs at least three parts separated by dots, none of them empty \
                 — the domain you own, reversed, then a name for the plugin, such as \
                 org.example.thing"
            ),
            Self::BadIdCharacter { id, character } => write!(
                f,
                "{id}: an id holds only a-z, 0-9, - and _ between its dots; {character:?} is \
                 none of those"
            ),
            Self::ReservedNamespace { id } => write!(
                f,
                "{id}: {RESERVED_NAMESPACE}* is reserved for the registry's own plugins"
            ),
            Self::NoVersions { id } => write!(f, "{id}: no versions listed"),
            Self::HomepageNotHttps { id } => write!(f, "{id}: the homepage url is not https"),
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
            Self::UppercaseChecksum {
                id,
                version,
                artifact,
            } => write!(
                f,
                "{id} {version}: the {artifact} sha256 has uppercase letters; write it the way \
                 sha256sum prints it, in lowercase"
            ),
            Self::NotHttps {
                id,
                version,
                artifact,
            } => write!(f, "{id} {version}: the {artifact} url is not https"),
        }
    }
}

/// The first character of `id` that an id may not hold, if there is one.
///
/// The same allowlist the daemon applies to a manifest id — `a-z`, `0-9`,
/// `.`, `-`, `_` — and deliberately not a narrower one. The registry may ask
/// for *more* than the daemon does about an id's shape, since an id here has
/// to be unique across everything anybody publishes; it must not reject
/// characters the daemon accepts and `docs/plugin-api.md` publishes as
/// legal, or a plugin that installs by hand could never be listed.
///
/// An allowlist rather than a list of forbidden characters, for the reason
/// the daemon gives too: the id becomes a directory name there and a file
/// name here, and a rule that enumerates what may appear cannot be walked
/// past by an encoding nobody thought of.
fn forbidden_character(id: &str) -> Option<char> {
    id.chars()
        .find(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_')))
}

/// Whether `id` is shaped like a reverse domain with a plugin name on the
/// end: at least three parts, none of them empty.
///
/// This part is the registry's own rule and is stricter than the daemon's,
/// which accepts any non-empty name that does not start with a dot. Two
/// reasons: an id here is the marketplace's unique key, so it has to be a
/// domain somebody owns plus a name below it rather than a word like
/// `audio` that the first person to ask for it would take from everyone
/// else — and no empty part means no `..` and no leading dot, which is what
/// makes the id safe as the file name `render` builds out of it.
fn has_reverse_domain_shape(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 3 && parts.iter().all(|part| !part.is_empty())
}

fn is_sixty_four_hex(text: &str) -> bool {
    text.len() == 64 && text.chars().all(|c| c.is_ascii_hexdigit())
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
            // Two rules and two findings, the same way the reserved
            // namespace does not hide behind the shape rule: an id can be
            // shaped right and spelled wrong, or the reverse, and a
            // contributor fixing one should not then be sent back for the
            // other.
            if !has_reverse_domain_shape(&id) {
                problems.push(Problem::NotReverseDomain { id: id.clone() });
            }
            if let Some(character) = forbidden_character(&id) {
                problems.push(Problem::BadIdCharacter {
                    id: id.clone(),
                    character,
                });
            }
            if id.starts_with(RESERVED_NAMESPACE) && !reserved_allowed {
                problems.push(Problem::ReservedNamespace { id: id.clone() });
            }
            if plugin.versions.is_empty() {
                problems.push(Problem::NoVersions { id: id.clone() });
            }
            // Its own `if`, not chained onto another with `else if`: a
            // reserved-namespace id with an unsafe homepage must report
            // both, the same reason `ReservedNamespace` above does not
            // hide behind `NotReverseDomain`.
            if !plugin.homepage.starts_with("https://") {
                problems.push(Problem::HomepageNotHttps { id: id.clone() });
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
                    // One message per checksum, and the more specific one
                    // wins: a digest that is 64 hex characters but shouted
                    // gets told so, rather than being told it is not 64 hex
                    // characters when it plainly is.
                    if !is_sixty_four_hex(&artifact.sha256) {
                        problems.push(Problem::BadChecksum {
                            id: id.clone(),
                            version: number.clone(),
                            artifact: what,
                        });
                    } else if artifact.sha256.chars().any(|c| c.is_ascii_uppercase()) {
                        problems.push(Problem::UppercaseChecksum {
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

    /// An underscore is legal in a manifest id — the daemon accepts it and
    /// `docs/plugin-api.md` says so — so it has to be legal here too. A
    /// registry that refused it would make a plugin that installs by hand
    /// impossible to publish, for no reason anybody could name.
    #[test]
    fn an_id_with_an_underscore_is_accepted() {
        let json = plugin("org.example.my_thing", &version("1.0.0"));
        let problems = index(&json).check();
        assert!(problems.is_empty(), "got {problems:#?}");
    }

    /// The message has to name what is actually wrong. A capital letter is
    /// the one an author hits by hand, and being told "an id is a reverse
    /// domain" describes the id they already wrote.
    #[test]
    fn an_id_with_a_capital_letter_names_the_character() {
        let json = plugin("org.example.Thing", &version("1.0.0"));
        assert!(matches!(
            index(&json).check().as_slice(),
            [Problem::BadIdCharacter { character: 'T', .. }]
        ));
        assert!(
            index(&json).check()[0].to_string().contains("'T'"),
            "the message names the character: {}",
            index(&json).check()[0]
        );
    }

    /// The reserved namespace. `check` refuses it for everyone;
    /// `check_allowing_reserved` is what lets the registry's own pull
    /// requests through, and the validate workflow calls it only for a
    /// change that can have come from this repository's own owner.
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

    /// The other half of the checksum rule, and the half nothing pinned:
    /// `sha256sum` prints lowercase, so an uppercase digest is somebody's
    /// retyping — and it must be reported as *that*, not as "not 64 hex
    /// characters", which is untrue of the 64 hex characters in front of
    /// them.
    #[test]
    fn a_checksum_in_uppercase_is_refused_and_the_message_says_why() {
        let json = plugin("org.example.thing", &version("1.0.0"))
            .replace(&"a".repeat(64), &"A".repeat(64));
        let problems = index(&json).check();
        assert!(
            matches!(
                problems.as_slice(),
                [Problem::UppercaseChecksum {
                    artifact: "module",
                    ..
                }]
            ),
            "got {problems:#?}"
        );
        assert!(
            problems[0].to_string().contains("uppercase"),
            "the message names the violation: {}",
            problems[0]
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

    /// The case a contributor actually hits: a homepage typed as `http://`
    /// rather than copied with the scheme it already had.
    #[test]
    fn a_plain_http_homepage_is_refused() {
        let json = plugin("org.example.thing", &version("1.0.0")).replace(
            r#""homepage":"https://example.org""#,
            r#""homepage":"http://example.org""#,
        );
        assert!(matches!(
            index(&json).check().as_slice(),
            [Problem::HomepageNotHttps { id }] if id == "org.example.thing"
        ));
    }

    /// The case that matters: a homepage is an `href` on the rendered
    /// page, and a `javascript:` URL there is a click-triggered exploit,
    /// not a broken link. `https://` is required, not merely preferred.
    #[test]
    fn a_homepage_with_a_javascript_url_is_refused() {
        let json = plugin("org.example.thing", &version("1.0.0")).replace(
            r#""homepage":"https://example.org""#,
            r#""homepage":"javascript:alert(1)""#,
        );
        assert!(matches!(
            index(&json).check().as_slice(),
            [Problem::HomepageNotHttps { id }] if id == "org.example.thing"
        ));
    }

    /// Two rules, two findings. A reserved id that is *also* malformed used
    /// to report only the malformation — the namespace rule sat behind an
    /// `else` and never ran, which quietly made "every problem is reported"
    /// untrue for the one input where both matter.
    #[test]
    fn a_malformed_id_in_the_reserved_namespace_reports_both_problems() {
        let json = plugin("dev.simix.AUDIO", &version("1.0.0"));
        let problems = index(&json).check();
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Problem::BadIdCharacter { character: 'A', .. }))
        );
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Problem::ReservedNamespace { .. }))
        );
        assert_eq!(problems.len(), 2, "got {problems:#?}");
    }

    #[test]
    fn a_plugin_with_no_versions_is_refused() {
        let json = plugin("org.example.thing", "");
        assert!(matches!(
            index(&json).check().as_slice(),
            [Problem::NoVersions { .. }]
        ));
    }

    #[test]
    fn only_the_module_url_being_http_is_reported() {
        let version_json = format!(
            r#"{{"version":"1.0.0","min_api":1,"license":"MIT",
                 "module":{{"url":"http://example.org/w","sha256":"{}","bytes":1}},
                 "manifest":{{"url":"https://example.org/t","sha256":"{}","bytes":2}}}}"#,
            "a".repeat(64),
            "b".repeat(64)
        );
        let json = plugin("org.example.thing", &version_json);
        let problems = index(&json).check();
        assert_eq!(problems.len(), 1, "got {problems:#?}");
        assert!(matches!(
            problems.as_slice(),
            [Problem::NotHttps {
                artifact: "module",
                ..
            }]
        ));
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

    /// A whole index that breaks five rules at once, kept as a file rather
    /// than as a string in a test: this is the shape a contributor's first
    /// attempt actually has, and it is what proves the tool reports all of
    /// it in one go instead of sending them round five times.
    #[test]
    fn the_broken_fixture_reports_every_rule_it_breaks() {
        let index = Index::from_json(include_str!("../tests/fixtures/broken.json"))
            .expect("it is malformed, not unparseable");
        let problems = index.check();

        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Problem::NotReverseDomain { .. }))
        );
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Problem::VersionsOutOfOrder { .. }))
        );
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Problem::BadChecksum { .. }))
        );
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Problem::NotHttps { .. }))
        );
        assert!(problems.len() >= 5, "got {problems:#?}");
    }
}
