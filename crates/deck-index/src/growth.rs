//! Whether one version wants more than the one already installed.
//!
//! This is the rule background updates hang on: a version that asks for the
//! same or less installs itself, a version that asks for **more** is held
//! back and reported. Without it, a silent update could take a permission
//! the user was never shown — and the promise that a user sees in plain
//! text what a plugin may do would hold everywhere except where it matters.

use std::fmt;

use crate::Capabilities;

/// One permission an offered version wants and the installed one does not
/// have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Growth {
    /// Permission to run a new command.
    Process {
        /// The command name.
        name: String,
    },
    /// Permission to reach a new host.
    Http {
        /// The hostname.
        host: String,
    },
    /// Permission to reach hosts inside the user's own network.
    HttpPrivate,
    /// Permission to read files under a new prefix.
    FsRead {
        /// The file system prefix.
        prefix: String,
    },
    /// Permission to run periodic timers.
    Timer,
}

impl fmt::Display for Growth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Process { name } => write!(f, "run the command {name}"),
            Self::Http { host } => write!(f, "reach {host}"),
            Self::HttpPrivate => write!(f, "reach hosts inside your own network"),
            Self::FsRead { prefix } => write!(f, "read files under {prefix}"),
            Self::Timer => write!(f, "run periodic timers"),
        }
    }
}

/// Whether `prefix` is already covered by one of `installed`.
///
/// Covered means at or below: `/home/x/notes` is covered by `/home/x`, and
/// `/home/x` is **not** covered by `/home/x/notes`. Comparing the two lists
/// as sets would miss exactly this, which is the one direction that matters.
///
/// Empty prefixes never cover anything (fail-safe against total filesystem access).
/// Trailing slashes are normalized away for comparison, so `/home/x/` covers
/// `/home/x/notes` as `/home/x` would. A bare `/` covers every absolute path —
/// if a plugin is allowed to read the whole filesystem, narrowing to any subtree
/// is not growth.
fn already_covered(prefix: &str, installed: &[String]) -> bool {
    // Empty prefix cannot cover anything — fail-safe for total filesystem access.
    if prefix.is_empty() {
        return false;
    }

    installed.iter().any(|allowed| {
        // Empty installed entry covers nothing.
        if allowed.is_empty() {
            return false;
        }

        // A bare `/` covers every absolute path.
        if allowed == "/" {
            return prefix.starts_with('/');
        }

        // Normalize by trimming trailing slashes for comparison.
        let allowed_normalized = allowed.trim_end_matches('/');
        let prefix_normalized = if prefix == "/" {
            "/"
        } else {
            prefix.trim_end_matches('/')
        };

        prefix_normalized == allowed_normalized
            || prefix_normalized
                .strip_prefix(allowed_normalized)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// Normalise an HTTP entry to match how `deck_core::Capabilities::allows_http`
/// compares permissions: lowercase the host, and treat a missing port as `443`.
/// This way `example.org`, `Example.org`, and `example.org:443` all denote the
/// same permission. Must be kept in sync with the deckd repository's logic
/// to avoid false-positive "wants more permissions" prompts.
fn normalize_http(host: &str) -> String {
    let lower = host.to_lowercase();
    if lower.contains(':') {
        lower
    } else {
        format!("{}:443", lower)
    }
}

impl Capabilities {
    /// Everything this version wants that `installed` does not already have.
    ///
    /// Empty means an update may install itself unattended.
    pub fn growth_over(&self, installed: &Capabilities) -> Vec<Growth> {
        let mut growth = Vec::new();

        for name in &self.process {
            if !installed.process.contains(name) {
                growth.push(Growth::Process { name: name.clone() });
            }
        }
        for host in &self.http {
            let normalized_offered = normalize_http(host);
            let is_installed = installed
                .http
                .iter()
                .any(|installed_host| normalize_http(installed_host) == normalized_offered);
            if !is_installed {
                growth.push(Growth::Http { host: host.clone() });
            }
        }
        if self.http_private && !installed.http_private {
            growth.push(Growth::HttpPrivate);
        }
        for prefix in &self.fs_read {
            if !already_covered(prefix, &installed.fs_read) {
                growth.push(Growth::FsRead {
                    prefix: prefix.clone(),
                });
            }
        }
        if self.timer && !installed.timer {
            growth.push(Growth::Timer);
        }

        growth
    }
}

#[cfg(test)]
mod tests {
    use crate::{Capabilities, Growth};

    fn caps() -> Capabilities {
        Capabilities::default()
    }

    #[test]
    fn an_unchanged_version_asks_for_nothing_new() {
        let mut installed = caps();
        installed.process = vec!["wpctl".into()];
        installed.timer = true;
        assert!(installed.clone().growth_over(&installed).is_empty());
    }

    #[test]
    fn a_version_that_gives_something_up_asks_for_nothing_new() {
        let mut installed = caps();
        installed.process = vec!["wpctl".into(), "pw-link".into()];
        let mut offered = caps();
        offered.process = vec!["wpctl".into()];
        assert!(offered.growth_over(&installed).is_empty());
    }

    #[test]
    fn a_new_command_is_growth() {
        let mut installed = caps();
        installed.process = vec!["wpctl".into()];
        let mut offered = installed.clone();
        offered.process.push("rm".into());
        assert_eq!(
            offered.growth_over(&installed),
            vec![Growth::Process { name: "rm".into() }]
        );
    }

    #[test]
    fn a_new_host_is_growth() {
        let mut offered = caps();
        offered.http = vec!["example.org".into()];
        assert_eq!(
            offered.growth_over(&caps()),
            vec![Growth::Http {
                host: "example.org".into()
            }]
        );
    }

    /// The flag is its own permission, not something the host list implies.
    #[test]
    fn reaching_into_the_private_network_is_growth_on_its_own() {
        let mut offered = caps();
        offered.http_private = true;
        assert_eq!(offered.growth_over(&caps()), vec![Growth::HttpPrivate]);
    }

    #[test]
    fn a_timer_where_there_was_none_is_growth() {
        let mut offered = caps();
        offered.timer = true;
        assert_eq!(offered.growth_over(&caps()), vec![Growth::Timer]);
    }

    #[test]
    fn a_new_read_prefix_is_growth() {
        let mut offered = caps();
        offered.fs_read = vec!["/etc".into()];
        assert_eq!(
            offered.growth_over(&caps()),
            vec![Growth::FsRead {
                prefix: "/etc".into()
            }]
        );
    }

    /// The subtle one, and the reason `fs_read` is not compared as a set of
    /// strings: `/home/x` is not a new entry beside `/home/x/notes`, it is
    /// the whole directory above it. Set comparison would wave that through
    /// as "one removed, one added, no growth".
    #[test]
    fn widening_a_read_prefix_upward_is_growth() {
        let mut installed = caps();
        installed.fs_read = vec!["/home/x/notes".into()];
        let mut offered = caps();
        offered.fs_read = vec!["/home/x".into()];
        assert_eq!(
            offered.growth_over(&installed),
            vec![Growth::FsRead {
                prefix: "/home/x".into()
            }]
        );
    }

    #[test]
    fn narrowing_a_read_prefix_downward_is_not_growth() {
        let mut installed = caps();
        installed.fs_read = vec!["/home/x".into()];
        let mut offered = caps();
        offered.fs_read = vec!["/home/x/notes".into()];
        assert!(offered.growth_over(&installed).is_empty());
    }

    #[test]
    fn every_kind_of_growth_is_reported_and_not_only_the_first() {
        let mut offered = caps();
        offered.process = vec!["rm".into()];
        offered.timer = true;
        offered.http_private = true;
        assert_eq!(offered.growth_over(&caps()).len(), 3);
    }

    /// An empty installed prefix does not cover anything — fail-safe.
    #[test]
    fn an_empty_installed_prefix_does_not_cover_an_offered_absolute_path() {
        let mut installed = caps();
        installed.fs_read = vec!["".into()];
        let mut offered = caps();
        offered.fs_read = vec!["/root/.ssh".into()];
        assert_eq!(
            offered.growth_over(&installed),
            vec![Growth::FsRead {
                prefix: "/root/.ssh".into()
            }]
        );
    }

    #[test]
    fn an_installed_prefix_with_a_trailing_slash_still_covers_its_children() {
        let mut installed = caps();
        installed.fs_read = vec!["/home/x/".into()];
        let mut offered = caps();
        offered.fs_read = vec!["/home/x/notes".into()];
        assert!(offered.growth_over(&installed).is_empty());
    }

    #[test]
    fn example_org_installed_example_org_443_offered_is_not_growth() {
        let mut installed = caps();
        installed.http = vec!["example.org".into()];
        let mut offered = caps();
        offered.http = vec!["example.org:443".into()];
        assert!(offered.growth_over(&installed).is_empty());
    }

    #[test]
    fn example_org_installed_example_org_uppercase_offered_is_not_growth() {
        let mut installed = caps();
        installed.http = vec!["example.org".into()];
        let mut offered = caps();
        offered.http = vec!["EXAMPLE.ORG".into()];
        assert!(offered.growth_over(&installed).is_empty());
    }

    #[test]
    fn example_org_installed_example_org_8443_offered_is_growth() {
        let mut installed = caps();
        installed.http = vec!["example.org".into()];
        let mut offered = caps();
        offered.http = vec!["example.org:8443".into()];
        assert_eq!(
            offered.growth_over(&installed),
            vec![Growth::Http {
                host: "example.org:8443".into()
            }]
        );
    }

    /// A bare `/` covers the entire filesystem. Narrowing from it is not growth.
    #[test]
    fn a_bare_slash_installed_covers_all_absolute_paths() {
        let mut installed = caps();
        installed.fs_read = vec!["/".into()];
        let mut offered = caps();
        offered.fs_read = vec!["/etc/hosts".into()];
        assert!(offered.growth_over(&installed).is_empty());
    }

    /// Empty string must still cover nothing, distinct from bare `/`.
    #[test]
    fn an_empty_installed_entry_still_covers_nothing() {
        let mut installed = caps();
        installed.fs_read = vec!["".into()];
        let mut offered = caps();
        offered.fs_read = vec!["/etc/hosts".into()];
        assert_eq!(
            offered.growth_over(&installed),
            vec![Growth::FsRead {
                prefix: "/etc/hosts".into()
            }]
        );
    }

    /// Widening to the root filesystem is the maximum widening.
    #[test]
    fn widening_to_the_root_filesystem_is_growth() {
        let mut installed = caps();
        installed.fs_read = vec!["/etc".into()];
        let mut offered = caps();
        offered.fs_read = vec!["/".into()];
        assert_eq!(
            offered.growth_over(&installed),
            vec![Growth::FsRead { prefix: "/".into() }]
        );
    }
}
