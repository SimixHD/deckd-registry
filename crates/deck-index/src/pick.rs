//! Which version of a plugin a client of a given API level gets.

use crate::{Plugin, Version};

impl Plugin {
    /// The newest version this client can use: the first entry that its API
    /// level reaches and that has not been withdrawn.
    ///
    /// The list is newest-first, so "first that fits" *is* "newest that
    /// fits". This is the whole reason the index carries a list rather than
    /// one entry per plugin: when the plugin API moves to 2, a client still
    /// on 1 keeps being offered the last version built for it, instead of
    /// seeing a plugin it can never install.
    pub fn pick(&self, api: u32) -> Option<&Version> {
        self.versions
            .iter()
            .find(|version| version.min_api <= api && version.yanked.is_none())
    }
}

#[cfg(test)]
mod tests {
    use crate::{Artifact, Capabilities, Index, Plugin, Version, Yanked};

    fn version(number: &str, min_api: u32, yanked: bool) -> Version {
        Version {
            version: number.to_owned(),
            min_api,
            license: "MIT".to_owned(),
            capabilities: Capabilities::default(),
            module: Artifact {
                url: "https://example.org/w".into(),
                sha256: "aa".into(),
                bytes: 1,
            },
            manifest: Artifact {
                url: "https://example.org/t".into(),
                sha256: "bb".into(),
                bytes: 2,
            },
            yanked: yanked.then(|| Yanked {
                reason: "it ate the settings".to_owned(),
                since: "2026-08-19".to_owned(),
            }),
            signature: None,
        }
    }

    fn plugin(versions: Vec<Version>) -> Plugin {
        Plugin {
            id: "org.example.thing".to_owned(),
            name: "Thing".to_owned(),
            description: "d".to_owned(),
            author: "a".to_owned(),
            homepage: "https://example.org".to_owned(),
            categories: Vec::new(),
            i18n: Default::default(),
            versions,
        }
    }

    #[test]
    fn the_newest_version_wins_when_everything_fits() {
        let plugin = plugin(vec![version("2.0.0", 1, false), version("1.0.0", 1, false)]);
        assert_eq!(plugin.pick(1).unwrap().version, "2.0.0");
    }

    /// The case the version list exists for: API 2 arrived, the plugin
    /// followed, and a client still on 1 must keep getting 1.9.0 rather
    /// than nothing at all.
    #[test]
    fn an_older_client_gets_the_last_version_built_for_it() {
        let plugin = plugin(vec![version("2.0.0", 2, false), version("1.9.0", 1, false)]);
        assert_eq!(plugin.pick(1).unwrap().version, "1.9.0");
        assert_eq!(plugin.pick(2).unwrap().version, "2.0.0");
    }

    #[test]
    fn a_withdrawn_version_is_skipped_and_the_one_below_it_is_offered() {
        let plugin = plugin(vec![version("2.0.0", 1, true), version("1.0.0", 1, false)]);
        assert_eq!(plugin.pick(1).unwrap().version, "1.0.0");
    }

    #[test]
    fn a_plugin_whose_every_version_is_withdrawn_offers_nothing() {
        let plugin = plugin(vec![version("2.0.0", 1, true), version("1.0.0", 1, true)]);
        assert!(plugin.pick(1).is_none());
    }

    #[test]
    fn a_plugin_that_needs_a_newer_api_than_this_client_has_offers_nothing() {
        let plugin = plugin(vec![version("1.0.0", 7, false)]);
        assert!(plugin.pick(1).is_none());
    }

    #[test]
    fn a_plugin_with_no_versions_at_all_offers_nothing() {
        assert!(plugin(Vec::new()).pick(1).is_none());
    }

    /// Both reasons at once, on different entries — the check is per
    /// version and not a filter applied once.
    #[test]
    fn a_too_new_version_above_a_withdrawn_one_falls_through_to_the_third() {
        let plugin = plugin(vec![
            version("3.0.0", 2, false),
            version("2.0.0", 1, true),
            version("1.0.0", 1, false),
        ]);
        assert_eq!(plugin.pick(1).unwrap().version, "1.0.0");
    }

    /// `pick` reads the list and does not reorder it. Every other case here
    /// is newest-first, so a `pick` that sorted would pass them all; this
    /// one lists 1.0.0 above 2.0.0 and asks for the first fit, which is the
    /// older one. `check` refuses an index in this order — and that is
    /// exactly the point: the ordering rule is what makes "first that fits"
    /// mean "newest that fits", so `pick` must not paper over a list that
    /// broke it.
    #[test]
    fn pick_returns_the_first_version_that_fits_rather_than_the_newest() {
        let plugin = plugin(vec![version("1.0.0", 1, false), version("2.0.0", 1, false)]);
        assert_eq!(plugin.pick(1).unwrap().version, "1.0.0");
    }

    /// An index that offers nothing is a normal index, not a broken one.
    #[test]
    fn an_empty_index_is_readable() {
        let index = Index::from_json(r#"{"schema":2,"updated":"2026-08-19","plugins":[]}"#)
            .expect("an empty index is valid");
        assert!(index.plugins.is_empty());
    }
}
