//! The deckd marketplace index.
//!
//! This crate is deliberately small and deliberately shared. The CI of the
//! registry uses it to decide whether a pull request may land; the deckd
//! client uses the very same code to decide what it is willing to install.
//! Two implementations would eventually disagree, and the day they did, the
//! registry would be advertising something no client could use.
//!
//! Everything here works without a network. What needs one — reaching a URL,
//! measuring a checksum against the file it names — lives in `index-tools`,
//! so that every rule in this crate is testable from memory alone.

mod index;
mod pick;
mod validate;

pub use index::{Artifact, Capabilities, Index, Plugin, SCHEMA, Version, Yanked};
pub use validate::{Problem, RESERVED_NAMESPACE};
