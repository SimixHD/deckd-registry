//! The static site, built from the index and nothing else.
//!
//! No template engine and no JavaScript toolchain: the whole page set is a
//! listing plus one page per plugin, and the index is the only input. The
//! registry's own README calls it "deliberately just a git repository with
//! an index file" — a build step that needed a package manager would be the
//! first thing to make that untrue.

use deck_index::{Capabilities, Index, Plugin};

/// The API level the rendered pages describe. The site shows what a
/// current client would get, so it picks versions the same way one does.
const SITE_API: u32 = 1;

/// Escape the five characters that would otherwise be markup.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// A byte count as a person reads it.
///
/// Every branch rounds up, on purpose: a shown size must never be smaller
/// than the file. `{:.1}` on a float rounds to nearest, which understates
/// about half the time — 1 100 000 bytes would print `1.0 MB`, naming a
/// size (1 048 576 bytes) smaller than the download actually is. Doing the
/// rounding in integers, the same way the kB branch already does with
/// `div_ceil`, keeps the guarantee true in the MB branch too.
fn human_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{} kB", bytes.div_ceil(1024))
    } else {
        let tenths_of_mb = (u128::from(bytes) * 10).div_ceil(1024 * 1024);
        format!("{}.{} MB", tenths_of_mb / 10, tenths_of_mb % 10)
    }
}

/// Every permission, in sentences rather than field names.
fn permissions_in_words(capabilities: &Capabilities) -> Vec<String> {
    let mut lines = Vec::new();
    for name in &capabilities.process {
        lines.push(format!("run the command {}", escape(name)));
    }
    for host in &capabilities.http {
        lines.push(format!("reach {}", escape(host)));
    }
    if capabilities.http_private {
        lines.push("reach hosts inside your own network".to_owned());
    }
    for prefix in &capabilities.fs_read {
        lines.push(format!("read files under {}", escape(prefix)));
    }
    if capabilities.timer {
        lines.push("run periodic timers".to_owned());
    }
    if lines.is_empty() {
        lines.push("nothing outside its own data directory".to_owned());
    }
    lines
}

fn shell(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="stylesheet" href="style.css">
</head>
<body>
<header><a href="index.html">deckd plugins</a></header>
<main>
{body}
</main>
<footer>Built from <a href="index.json">index.json</a>.</footer>
</body>
</html>
"#,
        title = escape(title)
    )
}

fn plugin_page(plugin: &Plugin) -> String {
    let mut body = format!(
        "<h1>{}</h1>\n<p class=\"description\">{}</p>\n<p class=\"author\">by {}</p>\n",
        escape(&plugin.name),
        escape(&plugin.description),
        escape(&plugin.author)
    );

    match plugin.pick(SITE_API) {
        None => body
            .push_str("<p class=\"none\">No version available for the current plugin API.</p>\n"),
        Some(version) => {
            body.push_str(&format!(
                "<dl>\n<dt>Version</dt><dd>{}</dd>\n<dt>Licence</dt><dd>{}</dd>\n\
                 <dt>Download</dt><dd>{} module, {} manifest</dd>\n</dl>\n",
                escape(&version.version),
                escape(&version.license),
                human_bytes(version.module.bytes),
                human_bytes(version.manifest.bytes),
            ));
            body.push_str("<h2>What it is allowed to do</h2>\n<ul>\n");
            for line in permissions_in_words(&version.capabilities) {
                body.push_str(&format!("<li>{line}</li>\n"));
            }
            body.push_str("</ul>\n");
        }
    }

    body.push_str(
        "<h2>Versions</h2>\n<table>\n<tr><th>Version</th><th>Minimum API</th><th>State</th></tr>\n",
    );
    for version in &plugin.versions {
        let state = match &version.yanked {
            Some(yanked) => format!("withdrawn: {}", escape(&yanked.reason)),
            None => "available".to_owned(),
        };
        body.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{state}</td></tr>\n",
            escape(&version.version),
            version.min_api
        ));
    }
    body.push_str("</table>\n");
    body.push_str(&format!(
        "<p><a href=\"{}\">Homepage</a></p>\n",
        escape(&plugin.homepage)
    ));

    shell(&plugin.name, &body)
}

fn listing(index: &Index) -> String {
    if index.plugins.is_empty() {
        return shell(
            "deckd plugins",
            "<h1>deckd plugins</h1>\n<p class=\"none\">No plugins yet. \
             The index is in place and the first entries are on their way.</p>\n",
        );
    }

    let mut body = String::from("<h1>deckd plugins</h1>\n<ul class=\"listing\">\n");
    for plugin in &index.plugins {
        body.push_str(&format!(
            "<li><a href=\"{id}.html\">{name}</a> — {description}</li>\n",
            id = escape(&plugin.id),
            name = escape(&plugin.name),
            description = escape(&plugin.description),
        ));
    }
    body.push_str("</ul>\n");
    shell("deckd plugins", &body)
}

/// Every page this index produces: file name and contents.
///
/// The file name of a plugin page is its id. That is safe **because
/// `run_render` runs the offline rules first**: they refuse anything that is
/// not a reverse domain, so an id holds nothing but lowercase letters,
/// digits, hyphens and dots — no slash, no `..`, nothing that could name a
/// file outside `_site`. That guard lives in the renderer's own caller and
/// not only in the workflow, so it holds however this code is reached.
pub fn render(index: &Index) -> Vec<(String, String)> {
    let mut pages = vec![("index.html".to_owned(), listing(index))];
    for plugin in &index.plugins {
        pages.push((format!("{}.html", plugin.id), plugin_page(plugin)));
    }
    pages
}

#[cfg(test)]
mod tests {
    use deck_index::{Index, Problem};

    use super::render;

    const ONE: &str = r#"{"schema":2,"updated":"2026-08-19","plugins":[{
      "id":"org.example.thing","name":"Thing","description":"Does a thing.",
      "author":"Someone","homepage":"https://example.org","categories":["audio"],
      "versions":[{
        "version":"1.0.0","min_api":1,"license":"MIT",
        "capabilities":{"process":["wpctl"],"timer":true},
        "module":{"url":"https://example.org/w","sha256":"aa","bytes":148992},
        "manifest":{"url":"https://example.org/t","sha256":"bb","bytes":4212}
      }]}]}"#;

    /// All five `Capabilities` fields populated at once, with an
    /// attacker-controlled string (`http`) that needs escaping — `ONE`
    /// only exercises `process` and `timer`, and the escaping test below
    /// only exercises `name`, so neither witnesses `http`, `http_private`
    /// or `fs_read` at all.
    const ALL_CAPABILITIES: &str = r#"{"schema":2,"updated":"2026-08-19","plugins":[{
      "id":"org.example.thing","name":"Thing","description":"Does a thing.",
      "author":"Someone","homepage":"https://example.org","categories":["audio"],
      "versions":[{
        "version":"1.0.0","min_api":1,"license":"MIT",
        "capabilities":{
          "process":["wpctl"],
          "http":["<script>evil.example</script>"],
          "http_private":true,
          "fs_read":["/etc/secrets"],
          "timer":true
        },
        "module":{"url":"https://example.org/w","sha256":"aa","bytes":148992},
        "manifest":{"url":"https://example.org/t","sha256":"bb","bytes":4212}
      }]}]}"#;

    fn page(pages: &[(String, String)], name: &str) -> String {
        pages
            .iter()
            .find(|(file, _)| file == name)
            .unwrap_or_else(|| {
                panic!(
                    "no page {name} in {:?}",
                    pages.iter().map(|(f, _)| f).collect::<Vec<_>>()
                )
            })
            .1
            .clone()
    }

    #[test]
    fn an_index_renders_one_listing_and_one_page_per_plugin() {
        let pages = render(&Index::from_json(ONE).unwrap());
        assert_eq!(pages.len(), 2);
        page(&pages, "index.html");
        page(&pages, "org.example.thing.html");
    }

    #[test]
    fn the_listing_names_every_plugin_and_links_to_its_page() {
        let pages = render(&Index::from_json(ONE).unwrap());
        let listing = page(&pages, "index.html");
        assert!(listing.contains("Thing"));
        assert!(listing.contains("Does a thing."));
        assert!(listing.contains(r#"href="org.example.thing.html""#));
    }

    /// Everything a person needs before they decide, on the page itself —
    /// the same list the first-run screen shows: what it costs, what it is
    /// allowed to do, and under what licence.
    #[test]
    fn a_plugin_page_shows_size_licence_and_every_permission() {
        let pages = render(&Index::from_json(ONE).unwrap());
        let detail = page(&pages, "org.example.thing.html");
        assert!(detail.contains("1.0.0"));
        assert!(detail.contains("MIT"));
        // 148 992 / 1024 = 145.5, rounded up to 146. The number here is
        // computed, not estimated: `human_bytes` rounds up so a displayed
        // size is never smaller than the file.
        assert!(
            detail.contains("146 kB"),
            "sizes are shown in units people read"
        );
        assert!(detail.contains("wpctl"), "the commands it may run");
        assert!(detail.contains("timers"), "the timer permission in words");
    }

    /// The MB branch must round up exactly like the kB branch does: a
    /// shown size is never allowed to be smaller than the file. 1 100 000
    /// bytes is 1.049… MiB — round-to-nearest would print `1.0 MB`, which
    /// names a size (1 048 576 bytes) smaller than the file itself.
    #[test]
    fn a_size_that_rounds_down_to_the_nearest_tenth_of_a_megabyte_rounds_up_instead() {
        let json = ONE.replace("148992", "1100000");
        let pages = render(&Index::from_json(&json).unwrap());
        let detail = page(&pages, "org.example.thing.html");
        assert!(
            detail.contains("1.1 MB"),
            "a shown size must never be smaller than the file: {detail}"
        );
    }

    /// The state the registry is actually in on the day it goes public.
    /// A page that renders an empty list as a blank screen would read as
    /// broken rather than as new.
    /// The guard that makes a plugin id safe as a file name. It lives in
    /// `run_render`, so this test drives the rule it depends on rather than
    /// the writer itself.
    #[test]
    fn an_id_that_could_name_a_file_elsewhere_never_reaches_the_renderer() {
        let json = ONE.replace("org.example.thing", "../../etc/passwd");
        let index = Index::from_json(&json).unwrap();
        let problems = index.check_allowing_reserved();
        // `NotReverseDomain` specifically, not merely "some problem or
        // other": this index also fails `HomepageNotHttps` if the
        // replacement above ever changes, and a test that only checks
        // "not empty" would stay green for a reason that has nothing to
        // do with the id this test exists to guard.
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Problem::NotReverseDomain { .. })),
            "check is what stops this, before render is ever called: got {problems:#?}"
        );
    }

    #[test]
    fn an_empty_index_renders_a_page_that_says_so() {
        let index =
            Index::from_json(r#"{"schema":2,"updated":"2026-08-19","plugins":[]}"#).unwrap();
        let pages = render(&index);
        assert_eq!(pages.len(), 1);
        assert!(page(&pages, "index.html").contains("No plugins yet"));
    }

    /// A name is somebody else's text. It goes into HTML, so it gets
    /// escaped — the registry is curated, but "curated" is a review step
    /// and not an encoder.
    #[test]
    fn text_from_the_index_is_escaped_before_it_becomes_html() {
        let json = ONE.replace("Thing", "<script>alert(1)</script>");
        let pages = render(&Index::from_json(&json).unwrap());
        let listing = page(&pages, "index.html");
        assert!(!listing.contains("<script>"));
        assert!(listing.contains("&lt;script&gt;"));
    }

    /// Every one of the five `Capabilities` fields shows up in words, and
    /// the two that are lists of attacker-controlled strings (`http`,
    /// `fs_read`) are escaped exactly like `name` already is — a stranger's
    /// pull request controls these strings as much as it controls the
    /// plugin's name.
    #[test]
    fn every_capability_field_is_named_and_the_attacker_controlled_ones_are_escaped() {
        let pages = render(&Index::from_json(ALL_CAPABILITIES).unwrap());
        let detail = page(&pages, "org.example.thing.html");
        assert!(detail.contains("wpctl"), "process, in words");
        assert!(
            detail.contains("reach hosts inside your own network"),
            "http_private, in words"
        );
        assert!(detail.contains("/etc/secrets"), "fs_read, in words");
        assert!(detail.contains("timers"), "timer, in words");
        assert!(
            !detail.contains("<script>"),
            "an http host is somebody else's text too, and gets escaped"
        );
        assert!(
            detail.contains("&lt;script&gt;evil.example&lt;/script&gt;"),
            "the escaped host reaches the page in its escaped form"
        );
    }

    /// A version that declares no capabilities at all is not a version the
    /// page says nothing about — it is a version the page can positively
    /// describe as doing nothing outside its own data directory.
    #[test]
    fn a_version_with_no_capabilities_says_it_can_do_nothing_outside_its_data_directory() {
        let json = ONE.replace(r#""capabilities":{"process":["wpctl"],"timer":true},"#, "");
        let pages = render(&Index::from_json(&json).unwrap());
        let detail = page(&pages, "org.example.thing.html");
        assert!(detail.contains("nothing outside its own data directory"));
    }
}
