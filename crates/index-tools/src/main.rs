//! Command-line tools for the registry index.

mod check;
mod fetch;

use std::process::ExitCode;

use deck_index::Index;

use crate::check::check_artifacts;
use crate::fetch::Http;

/// The only flag this program recognises.
const ALLOW_RESERVED: &str = "--allow-reserved";

/// What the command line asked for, decided from the arguments alone. Kept
/// separate from `main` so the decision — as opposed to reading the file or
/// reaching the network — can be tested directly.
#[derive(Debug, PartialEq, Eq)]
enum Command<'a> {
    /// Check the index at `path`.
    Check {
        /// The index file to read.
        path: &'a str,
        /// Whether `deck_index::RESERVED_NAMESPACE` may be claimed.
        allow_reserved: bool,
    },
    /// The arguments do not describe a known command.
    Usage,
}

/// Decide what was asked for.
///
/// Any `--`-prefixed argument other than [`ALLOW_RESERVED`] falls back to
/// [`Command::Usage`] rather than being silently ignored. Today that is the
/// only flag there is, so a typo would happen to fail safe — but a later
/// workflow turns `--allow-reserved` on only for changes that can have come
/// from this repository's own owner, and a misspelling there deserves a
/// visible usage error, not a run that quietly landed on the strict
/// default for reasons nobody can read back from the output.
fn parse_args(args: &[String]) -> Command<'_> {
    let flags: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| a.starts_with("--"))
        .collect();
    let plain: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with("--"))
        .collect();

    if flags.iter().any(|flag| *flag != ALLOW_RESERVED) {
        return Command::Usage;
    }

    match plain.as_slice() {
        ["check", path] => Command::Check {
            path,
            allow_reserved: flags.contains(&ALLOW_RESERVED),
        },
        _ => Command::Usage,
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        Command::Check {
            path,
            allow_reserved,
        } => run_check(path, allow_reserved),
        Command::Usage => {
            eprintln!("usage: index-tools check <index.json> [--allow-reserved]");
            ExitCode::FAILURE
        }
    }
}

/// Check an index.
///
/// `reserved_allowed` is off by default and is what keeps
/// [`deck_index::RESERVED_NAMESPACE`] reserved. The workflow turns it on
/// only where the change can have come from the registry's owner — a push
/// to `main`, or a pull request whose branch lives in this very repository.
/// A pull request from somebody's fork runs without it, and a stranger
/// claiming `dev.simix.*` is refused by the same rule that refuses it in a
/// unit test.
fn run_check(path: &str, reserved_allowed: bool) -> ExitCode {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("{path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let index = match Index::from_json(&text) {
        Ok(index) => index,
        Err(err) => {
            eprintln!("{path}: {err}");
            return ExitCode::FAILURE;
        }
    };

    // The offline rules first, and the network only if they all hold: an
    // index that is malformed has nothing worth fetching, and a contributor
    // gets the cheap answer without waiting on five downloads.
    let problems = if reserved_allowed {
        index.check_allowing_reserved()
    } else {
        index.check()
    };
    if !problems.is_empty() {
        for problem in &problems {
            eprintln!("{problem}");
        }
        return ExitCode::FAILURE;
    }

    let faults = check_artifacts(&index, &Http);
    if !faults.is_empty() {
        for fault in &faults {
            eprintln!("{fault}");
        }
        return ExitCode::FAILURE;
    }

    println!(
        "{}: {} plugins, everything checks out",
        path,
        index.plugins.len()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::{Command, parse_args};

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn a_well_formed_check_is_recognised() {
        assert_eq!(
            parse_args(&args(&["check", "index.json"])),
            Command::Check {
                path: "index.json",
                allow_reserved: false
            }
        );
    }

    #[test]
    fn the_allow_reserved_flag_is_recognised() {
        assert_eq!(
            parse_args(&args(&["check", "index.json", "--allow-reserved"])),
            Command::Check {
                path: "index.json",
                allow_reserved: true
            }
        );
    }

    #[test]
    fn an_unrecognised_flag_falls_back_to_usage() {
        assert_eq!(
            parse_args(&args(&["check", "index.json", "--bogus-flag"])),
            Command::Usage
        );
    }

    /// The whole point of rejecting rather than ignoring: a misspelling of
    /// the one flag that matters must not be read as "flag absent".
    #[test]
    fn a_misspelled_allow_reserved_falls_back_to_usage() {
        assert_eq!(
            parse_args(&args(&["check", "index.json", "--allow-reserverd"])),
            Command::Usage
        );
    }

    #[test]
    fn no_arguments_at_all_falls_back_to_usage() {
        assert_eq!(parse_args(&args(&[])), Command::Usage);
    }
}
