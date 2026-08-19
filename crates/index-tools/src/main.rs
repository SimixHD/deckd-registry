//! Command-line tools for the registry index.

mod check;
mod fetch;
mod render;

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
    /// Build the static site for the index at `path`.
    Render {
        /// The index file to read.
        path: &'a str,
    },
    /// The arguments do not describe a known command.
    Usage,
}

/// Decide what was asked for.
///
/// Flag legality is decided per command rather than once for all of them:
/// `check` accepts `--allow-reserved` and nothing else, and any other flag —
/// including a typo of that one — falls back to [`Command::Usage`] rather
/// than being silently ignored. A later workflow turns `--allow-reserved` on
/// only for changes that can have come from this repository's own owner, and
/// a misspelling there deserves a visible usage error, not a run that
/// quietly landed on the strict default for reasons nobody can read back
/// from the output.
///
/// `render` takes no flags at all. It permits the reserved namespace
/// unconditionally — it renders this repository's own index, not somebody
/// else's pull request — so `--allow-reserved` on `render` could only
/// mislead, and rejecting it outright is safer than a global flag table that
/// would otherwise swallow it unused.
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

    match plain.as_slice() {
        ["check", path] => {
            if flags.iter().any(|flag| *flag != ALLOW_RESERVED) {
                return Command::Usage;
            }
            Command::Check {
                path,
                allow_reserved: flags.contains(&ALLOW_RESERVED),
            }
        }
        ["render", path] if flags.is_empty() => Command::Render { path },
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
        Command::Render { path } => run_render(path),
        Command::Usage => {
            eprintln!("usage: index-tools check <index.json> [--allow-reserved]");
            eprintln!("       index-tools render <index.json>");
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

/// Write the site into `_site/`, beside a copy of the index itself — the
/// page footer links to it, and a marketplace whose data is one click from
/// its presentation is easier to trust than one where it is not.
fn run_render(path: &str) -> ExitCode {
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

    // The offline rules, before a single byte is written. `render` builds
    // file names out of plugin ids, and it is this call that has already
    // refused an id that could name a file somewhere else. No network here:
    // whether a download URL answers is not this command's business, and
    // publishing must not wait on it.
    let problems = index.check_allowing_reserved();
    if !problems.is_empty() {
        for problem in &problems {
            eprintln!("{problem}");
        }
        return ExitCode::FAILURE;
    }

    let out = std::path::Path::new("_site");
    if let Err(err) = std::fs::create_dir_all(out) {
        eprintln!("_site: {err}");
        return ExitCode::FAILURE;
    }
    for (name, html) in crate::render::render(&index) {
        if let Err(err) = std::fs::write(out.join(&name), html) {
            eprintln!("{name}: {err}");
            return ExitCode::FAILURE;
        }
    }
    for (from, to) in [("site/style.css", "style.css"), (path, "index.json")] {
        if let Err(err) = std::fs::copy(from, out.join(to)) {
            eprintln!("{from}: {err}");
            return ExitCode::FAILURE;
        }
    }

    println!("_site: {} plugins rendered", index.plugins.len());
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

    #[test]
    fn a_well_formed_render_is_recognised() {
        assert_eq!(
            parse_args(&args(&["render", "index.json"])),
            Command::Render { path: "index.json" }
        );
    }

    /// `render` takes no flags: it always permits the reserved namespace,
    /// since it renders this repository's own index and never a stranger's
    /// pull request, so `--allow-reserved` here could only mislead about
    /// what the command does. Without this arm the flag would be collected
    /// and then silently unused — the same failure mode Task 6's review
    /// raised for an unrecognised flag, one level up.
    #[test]
    fn render_does_not_accept_the_allow_reserved_flag() {
        assert_eq!(
            parse_args(&args(&["render", "index.json", "--allow-reserved"])),
            Command::Usage
        );
    }
}
