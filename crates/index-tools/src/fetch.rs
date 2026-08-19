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

/// The real one.
pub struct Http;

impl Fetcher for Http {
    fn fetch(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
        let mut response = ureq::get(url).call().map_err(|err| err.to_string())?;
        response
            .body_mut()
            .with_config()
            .limit(max)
            .read_to_vec()
            .map_err(|err| err.to_string())
    }
}
