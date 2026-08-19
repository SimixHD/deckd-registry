//! Command-line tools for the registry index.

mod check;
mod fetch;

use std::process::ExitCode;

use deck_index::Index;

use crate::check::check_artifacts;
use crate::fetch::Http;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
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

    match plain.as_slice() {
        ["check", path] => run_check(path, flags.contains(&"--allow-reserved")),
        _ => {
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
