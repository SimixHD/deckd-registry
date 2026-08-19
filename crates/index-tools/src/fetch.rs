//! Where bytes come from.
//!
//! A trait and not a function, for one reason: it is what lets every test
//! in this crate answer from memory. The check that matters — does this
//! file hash to what the entry claims — must be provable without a network,
//! or it is only ever proved on days the network agrees.

/// Something that can hand over the bytes behind a URL.
pub trait Fetcher {
    /// At most `max` bytes. More than that is an error rather than a
    /// truncation: a silently cut-off file would fail the checksum anyway,
    /// but with a message that sends the reader looking in the wrong place.
    fn fetch(&self, url: &str, max: u64) -> Result<Vec<u8>, String>;
}

/// How far a redirect chain may run before the fetch is given up on. The
/// same number the daemon allows a plugin's `http-request`, and for the
/// same reason: a chain has to end somewhere, and five hops is more than
/// any honest release URL needs.
const MAX_REDIRECTS: u32 = 5;

/// The real one.
pub struct Http {
    /// Configured once and reused: the configuration *is* the safety
    /// property here, so it must not be something a call site can forget.
    agent: ureq::Agent,
}

impl Http {
    /// A fetcher that will not leave `https`.
    ///
    /// `ureq`'s defaults are `https_only = false` and ten redirects, which
    /// together mean an `https://` URL in the index can be answered with a
    /// redirect to `http://` and the bytes then arrive in plaintext — the
    /// entry's `https://` prefix, and the rule in `validate.rs` that
    /// enforces it, would be checking only the first hop of a journey
    /// somebody else gets to route. `docs/plugin-api.md` demands exactly
    /// the opposite of a plugin's own requests: every hop rechecked, every
    /// scheme but `https` refused, because one allowed host that redirects
    /// turns the whole list into decoration. What the registry requires of
    /// a plugin it enforces on itself.
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .https_only(true)
                    .max_redirects(MAX_REDIRECTS)
                    .build(),
            ),
        }
    }
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetcher for Http {
    fn fetch(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
        let mut response = self.agent.get(url).call().map_err(|err| err.to_string())?;
        response
            .body_mut()
            .with_config()
            .limit(max)
            .read_to_vec()
            .map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{Http, MAX_REDIRECTS};

    /// Neither of these is a default — `ureq` ships `https_only = false`
    /// and ten redirects — and neither can be witnessed by any other test
    /// in this crate, because every one of them answers from memory. So
    /// the configuration itself is what gets pinned: a fetcher built
    /// without it would follow an `https://` entry down to `http://`
    /// without a word.
    #[test]
    fn the_fetcher_refuses_to_leave_https_and_stops_after_five_hops() {
        let config = Http::new().agent.config().clone();
        assert!(config.https_only(), "an http:// hop is not a fallback");
        assert_eq!(config.max_redirects(), MAX_REDIRECTS);
    }
}
