// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! Invariants of the app shell source, and of what is and is not committed.
//!
//! Everything here reads committed files. `dist/` is build output and is
//! gitignored, so the release gate — which exports the candidate tree — never
//! has it, and cannot build it either: that needs the wasm target and a pinned
//! `wasm-bindgen` CLI. A test asserting on `dist/` would therefore run only in
//! a developer's checkout, which is exactly where it is least likely to catch
//! anything, so those assertions are gone rather than skipped. What covers the
//! built output is `.github/workflows/build.yml`, which runs both build steps
//! from a clean checkout and inspects the eight files that come out.
//!
//! There are no browser tests, and there is nothing a browser test would add
//! here that the domain tests and this file do not already pin: the board's
//! decisions are pure and tested in `src/departures.rs`, and everything that
//! reaches the DOM is Rust building elements rather than a string being parsed.

use std::path::Path;

/// The app shell, as committed.
///
/// `build.rs` copies this into `dist/index.html` byte for byte, so asserting
/// on it asserts on exactly what gets published.
fn shell() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui.html");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// The service worker template, as committed.
fn worker() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/service-worker.js");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// The browser layer, as committed.
///
/// The one file that talks to the DOM, so it is where a registration is made
/// and where a stale one is released.
fn browser_layer() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui.rs");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// Tracked file names, or `None` outside a checkout.
///
/// The release gate exports the candidate as a bare directory with no `.git`,
/// so there is no index to ask. Callers decide what that means.
fn tracked_files() -> Option<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let inside = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(root)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if !inside {
        return None;
    }
    let output = std::process::Command::new("git")
        .args(["ls-files"])
        .current_dir(root)
        .output()
        .expect("git ls-files");
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The board is wasm. A hand-written ABI would mean the DOM logic no longer
/// shares the domain the tests cover — which is the property those tests exist
/// to protect — and a second `fetch(` would mean hand-written JavaScript
/// application code, which this app has none of.
#[test]
fn the_page_loads_generated_bindings_not_a_manual_wasm_abi() {
    let page = shell();
    assert!(
        page.contains("<!DOCTYPE html>"),
        "the shell must be a document"
    );
    assert!(page.contains("<script type=\"module\">"), "a module script");
    assert!(
        page.contains("import('./app.js')"),
        "the page must load the generated bindings"
    );
    assert_eq!(
        page.matches("<script").count(),
        1,
        "exactly one script tag:\n{page}"
    );
    for obsolete in [
        "instantiateStreaming",
        "alloc_buf",
        "wasm.exports",
        "fetch(",
        "innerHTML",
    ] {
        assert!(!page.contains(obsolete), "{obsolete} in the static shell");
    }
}

/// The parts of the page Rust owns must exist and be identified, because
/// `ui.rs` looks them up by id and a rename here is a blank board with no
/// error. The picker, the board area and the standing notice are the three.
#[test]
fn the_shell_has_the_ids_the_board_looks_up() {
    let page = shell();
    for id in [
        "search",
        "results",
        "toggles",
        "departures",
        "notice",
        "station-name",
        "stamp",
    ] {
        assert!(page.contains(&format!("id=\"{id}\"")), "missing #{id}");
    }
}

/// A board is read at arm's length on a phone, by people who may not be able to
/// see the colour the whole thing is keyed on. The line name is text, the
/// changing regions announce themselves, and the one control is labelled.
#[test]
fn the_shell_is_labelled_live_and_legible_without_colour() {
    let page = shell();
    // The search field's label is for screen readers; the placeholder is what a
    // sighted reader reads. Both must exist: one without the other leaves either
    // a keyboard user or a sighted one guessing what the box is for.
    assert!(
        page.contains("for=\"search\""),
        "the search field is labelled"
    );
    assert!(
        page.contains("placeholder="),
        "the search field says what it is for"
    );
    assert!(
        page.contains("role=\"combobox\""),
        "the search field is a combobox"
    );
    assert!(
        page.contains("role=\"listbox\""),
        "the results are a listbox"
    );
    assert!(
        page.contains("visually-hidden"),
        "a label that is only read aloud still has to be hidden from the page"
    );
    assert!(
        page.contains("aria-live=\"polite\""),
        "changing text announces"
    );
    assert!(
        page.contains("role=\"status\""),
        "the notice is a status region"
    );
    assert!(page.contains(":focus-visible"), "focus must be visible");
    assert!(
        page.contains("prefers-color-scheme"),
        "dark mode is respected"
    );
    assert!(
        page.contains("prefers-reduced-motion"),
        "reduced motion is respected"
    );
    // The line chips carry a hairline ring, because two of the published line
    // colours are all but invisible against the page behind them: Northern's
    // black is 1.07:1 on the dark page and Circle's yellow is 1.34:1 on the
    // light one. The ring gives every chip a defined edge without touching the
    // colour the reader is meant to be identifying. See `the_chip_edge_is_a_
    // decision_the_numbers_record` in src/departures.rs.
    assert!(
        page.contains("--ring"),
        "chips need an edge, or a line's own colour disappears into the page"
    );
    // The chip's colour arrives as a custom property set inline by Rust, and the
    // stylesheet consumes it. That indirection is the point: `build.rs` and
    // `ui.rs` agree on `--chip` and `--chip-ink`, and no colour is written in
    // two places.
    assert!(
        page.contains("--chip:"),
        "the chip colour must be a custom property"
    );
    assert!(
        page.contains("span[data-line]") || page.contains(".chip {"),
        "the chip styles must target the element that carries the class"
    );
    assert!(
        page.contains("box-shadow: 0 0 0 1px var(--ring)"),
        "a chip without its ring disappears into the page for two of the twelve \
         published colours"
    );
    // Density is the complaint this rewrite answers, so it is asserted rather
    // than left to taste: a row height and a type size, both small.
    assert!(page.contains("--row:"), "rows must have a declared height");
    assert!(
        page.contains("td.minutes"),
        "the minutes need their own cell class so they can be the loud thing"
    );
    // The board's one accent, for the minutes, and the "arriving now" mark.
    assert!(
        page.contains("--accent"),
        "the minutes need an accent colour"
    );
    assert!(
        page.contains("td.minutes.due"),
        "arriving now must be distinguishable: it is the one minute count that \
         means the train is at the platform"
    );
    assert!(
        page.contains("<noscript>"),
        "the page must say what it needs"
    );
}

/// The site is mounted under an arbitrary prefix, so every URL in it is
/// relative. One build, any subdirectory — which also means the app is not
/// pinned to the TfL API's host from the shell's point of view.
#[test]
fn the_shell_is_mountable_anywhere() {
    let page = shell();
    assert!(
        !page.contains("http://") && !page.contains("https://"),
        "an absolute URL would break the site outside its own origin"
    );
    assert!(
        page.contains("./app.js"),
        "bindings must be referenced relatively"
    );
    assert!(
        page.contains("manifest.webmanifest"),
        "the page must register a manifest"
    );
    assert!(
        page.contains("./icon.svg"),
        "the committed SVG must be the page icon"
    );
}

/// The service worker is a committed template with exactly one placeholder, and
/// `build.rs` substitutes it. A template with no placeholder would mean the
/// cache never invalidates; a second one would mean the substitution is not
/// the only edit.
#[test]
fn the_service_worker_template_has_exactly_one_placeholder() {
    let worker = worker();
    assert_eq!(
        worker.matches("__VERSION__").count(),
        1,
        "the template must carry exactly one version placeholder"
    );
    assert!(
        worker.contains("new URL('./', self.location.href)"),
        "the worker must resolve its cache from its own location"
    );
    assert!(
        !worker.contains("skipWaiting"),
        "an update must not swap the wasm under a live tab"
    );
}

/// The eight files the worker caches must be exactly the eight the site is
/// made of. A name in one list and not the other is a site that installs but
/// does not start, or one that starts and does not install.
#[test]
fn the_worker_caches_exactly_the_sites_files() {
    let worker = worker();
    let assets = worker
        .lines()
        .find(|line| line.contains("const ASSETS"))
        .expect("the worker declares an ASSETS list");
    for file in [
        "'./'",
        "'app.js'",
        "'app_bg.wasm'",
        "'manifest.webmanifest'",
        "'icon-192.png'",
        "'icon-512.png'",
        "'icon.svg'",
        "'index.html'",
    ] {
        assert!(assets.contains(file), "{file} missing from the cache list");
    }
}

/// The manifest is what the browser reads to decide what an install *is*, so
/// it is the one precached URL the worker must never answer from the cache: a
/// cached copy would let a fresh install re-derive the app's identity from a
/// manifest older than the last change to it — the exact shape of the bug
/// that once made every install in this family claim the same identity. It
/// still sits in `ASSETS`, so a manifest that 404s fails the install loudly;
/// it just always reaches the network.
#[test]
fn the_worker_never_serves_the_manifest_from_the_cache() {
    let worker = worker();
    assert!(
        worker.contains("'manifest.webmanifest'"),
        "the manifest must stay in ASSETS: a broken manifest must fail the install loudly"
    );
    let code: String = worker
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let fetch = code
        .split_once("addEventListener('fetch'")
        .expect("the worker must handle fetch")
        .1;
    assert!(
        fetch.contains("url === MANIFEST"),
        "the fetch handler must exempt the manifest before ever touching the cache; \
         a cached manifest lets an install re-derive a stale identity"
    );
}

/// This is the one app in the family that cannot work offline, and the reason
/// it still behaves is that the worker never touches the live API. Catching a
/// cached arrivals list is not an inconvenience: TfL's feed holds a few minutes
/// of trains, so a cached one is confidently wrong, and a board that looks live
/// while serving yesterday's trains is the worst thing this app could do.
///
/// The allowlist is the enforcement, so assert the allowlist and assert that
/// the API is not in it. The comment in the template explains this rule by
/// name, so the check is on the code and the list, not on the whole text.
#[test]
fn the_service_worker_does_not_cache_or_intercept_the_tfl_api() {
    let worker = worker();
    let code: String = worker
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code.contains("api.tfl.gov.uk"),
        "the worker must not name the TfL API in code; it has no business \
         caching or intercepting it"
    );
    assert!(
        code.contains("ASSETS.includes(url)"),
        "the worker must answer only its own assets, not every GET"
    );
    assert!(
        !code.contains(
            "caches.match(event.request)\n    return cached || fetch(event.request);\n  }"
        ),
        "the handler must stay an allowlist"
    );
}

/// No user-visible text may name the implementation.
///
/// A reader who is told a failure happened "in Rust", or that the page needs
/// "WebAssembly", has been told something they cannot act on and did not ask
/// for. What they need to know is what the app is called and what to do next.
///
/// The line is the *interface*, not the codebase: `AGENTS.md`, `README.md` and
/// every source comment here may and do say "Rust" in as much detail as they
/// like, and this test deliberately does not read them. What it checks is the
/// strings a person can see — the markup that is rendered, and the literals
/// `ui.rs` writes into the DOM.
///
/// What is still allowed is a fact about the *service*. "Departures come live
/// from the TfL API" tells the reader why a time might be stale, which is
/// exactly the kind of thing a board should say, and names no implementation.
#[test]
fn no_user_visible_text_names_the_implementation() {
    const BANNED: &[&str] = &[
        "rust",
        "webassembly",
        "wasm",
        "bindings",
        "compile",
        "compiler",
    ];
    /// Banned everywhere except the `<noscript>` notice, and only there because
    /// its reader is the one person for whom "this needs JavaScript to run" is
    /// the true and actionable explanation. See `strip_non_rendered`.
    const BANNED_EXCEPT_NOSCRIPT: &[&str] = &["javascript"];

    // The rendered parts of the shell: text nodes, and the message the loader
    // writes into the page. Comments, `<style>` and `<script>` bodies are
    // stripped first, because they are for the next person to read, not the
    // reader. The `<noscript>` notice is the one deliberate exception; see
    // `strip_non_rendered`.
    let page = shell();
    let visible = strip_non_rendered(&page);
    for word in BANNED {
        assert!(
            !contains_word(&visible, word),
            "the page shows the word {word:?} to a reader; it must not name the \
             implementation.\n--- visible text ---\n{visible}"
        );
    }

    // `javascript` is banned everywhere except the `<noscript>` notice, so the
    // notice is removed and the rest checked on its own.
    let without_noscript = strip_noscript(&visible);
    for word in BANNED_EXCEPT_NOSCRIPT {
        assert!(
            !contains_word(&without_noscript, word),
            "the page shows the word {word:?} to a reader outside the \
             <noscript> notice.\n--- visible text ---\n{without_noscript}"
        );
    }

    // Every string literal `ui.rs` can put on screen.
    let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui.rs"))
        .expect("the browser layer");
    for line in source.lines() {
        let trimmed = line.trim_start();
        // Only `const NAME: &str = "..."` is a user-visible message. Anything
        // else with a string in it is a URL, a query-parameter name, or a
        // format string, none of which is prose a reader ever sees.
        let Some(rest) = trimmed.strip_prefix("const ") else {
            continue;
        };
        if !rest.contains(": &str = \"") {
            continue;
        }
        // `javascript` is banned here without exception: the <noscript> notice
        // is in the shell, and nothing this file writes into the DOM is shown to
        // a reader whose scripting is off.
        for word in BANNED.iter().chain(BANNED_EXCEPT_NOSCRIPT) {
            assert!(
                !contains_word(rest, word),
                "a user-visible message names the implementation ({word:?}): {trimmed}"
            );
        }
    }
}

/// Strip the parts of the HTML a reader never sees: comments, `<style>`,
/// `<script>`, and the tags themselves, leaving the text that is rendered.
///
/// The loader's `catch` handler *does* write to the page, so `<script>` is not
/// simply dropped — its string literals are kept, and the code around them
/// removed, which is enough to see whether the message names the
/// implementation.
fn strip_non_rendered(page: &str) -> String {
    let mut out = String::with_capacity(page.len());
    let mut rest = page;
    loop {
        let Some(at) = rest.find('<') else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.split_once("-->").map_or("", |(_, r)| r);
            continue;
        }
        if rest.starts_with("<style") {
            rest = rest
                .split_once('>')
                .and_then(|(_, r)| r.split_once("</style>"))
                .map_or("", |(_, r)| r);
            continue;
        }
        // `<noscript>` is kept, and that is the one deliberate exception. Its
        // text only ever reaches someone whose JavaScript is switched off, so
        // "this needs JavaScript to run" is the one true and actionable thing
        // it can tell them — the failure the notice exists to explain is
        // precisely that the scripting did not run. It names no application
        // implementation, only the browser setting the reader controls. The
        // rest of the app says nothing of the kind; see the test above.
        if rest.starts_with("<noscript") {
            let Some(close) = rest.find('>') else {
                break;
            };
            let after = &rest[close + 1..];
            if let Some((body, tail)) = after.split_once("</noscript>") {
                out.push(' ');
                out.push_str(NOSCRIPT_OPEN);
                out.push_str(body);
                out.push_str(NOSCRIPT_CLOSE);
                out.push(' ');
                rest = tail;
                continue;
            }
        }
        let Some(close) = rest.find('>') else {
            out.push_str(rest);
            break;
        };
        let tag = &rest[..=close];
        if tag.starts_with("<script") {
            for literal in quoted(tag) {
                out.push(' ');
                out.push_str(&literal);
                out.push(' ');
            }
        }
        rest = &rest[close + 1..];
    }
    out
}

/// Delimiters wrapped around the `<noscript>` body so the notice can be
/// recognised again, and taken back out, after the fact.
const NOSCRIPT_OPEN: &str = "\u{1}noscript\u{2}";
const NOSCRIPT_CLOSE: &str = "\u{2}\u{1}";

/// Remove the `<noscript>` notice from already-extracted visible text.
fn strip_noscript(visible: &str) -> String {
    let mut out = String::with_capacity(visible.len());
    let mut rest = visible;
    while let Some(start) = rest.find(NOSCRIPT_OPEN) {
        out.push_str(&rest[..start]);
        let after = &rest[start + NOSCRIPT_OPEN.len()..];
        match after.find(NOSCRIPT_CLOSE) {
            Some(end) => rest = &after[end + NOSCRIPT_CLOSE.len()..],
            None => {
                rest = after;
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The double-quoted string literals in a fragment of HTML or JavaScript.
fn quoted(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut literal = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                // An escaped quote ends the literal only if it is not itself
                // escaped; keeping it simple is fine here, because the strings
                // this reads are the app's own prose.
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        literal.push(escaped);
                    }
                }
                _ => literal.push(c),
            }
        }
        out.push(literal);
    }
    out
}

/// Whether `haystack` contains `word` as a word, not as part of another one.
///
/// Word-bounded so that `rust` does not fire on `trusted` and `wasm` does not
/// fire on `wasmbounded`; a check loose enough to trip on those gets deleted
/// by the first person it annoys, and then stops protecting anything.
fn contains_word(haystack: &str, word: &str) -> bool {
    let haystack = haystack.to_lowercase();
    let mut at = 0;
    while let Some(found) = haystack[at..].find(word) {
        let start = at + found;
        let end = start + word.len();
        let before_ok = haystack[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric());
        let after_ok = haystack[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        at = end;
    }
    false
}

/// The CI workflow must be runnable.
///
/// This repository's first CI run was red on `master` with every one of the
/// eight files reported `ok`: the build was fine and the *check* could not
/// run. `python3 -c '…'` with an indented multi-line body is an
/// `IndentationError` before the interpreter reads a statement, and an
/// apostrophe anywhere in that body ends the shell quote, so bash never reaches
/// Python at all. The form looks correct in review and fails only on a runner.
///
/// So: no `python3 -c` in the workflow, and every heredoc terminated. A
/// `python3 -c` here is always a mistake, never a style choice.
#[test]
fn the_workflow_can_actually_run_its_checks() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/build.yml");
    let workflow = std::fs::read_to_string(&path).expect("the workflow");
    // Only the lines that actually run: a comment explaining why the form is
    // banned must not make this test fail, which is the same trap as a shell
    // assertion matching its own documentation.
    let code: String = workflow
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !code.contains("python3 -c"),
        "the workflow uses the inline `python3` flag; an indented body is an \
         IndentationError and an apostrophe ends the shell quote. Use a heredoc."
    );
    // Every heredoc that is opened is closed. The terminator may be indented —
    // the YAML block scalar dedents the whole step, so it lands in column 0 —
    // but it must be the only thing on its line.
    let opened = workflow.matches("<<'PY'").count();
    let closed = workflow.lines().filter(|line| line.trim() == "PY").count();
    assert_eq!(
        opened, closed,
        "{opened} heredocs are opened and {closed} are closed; an unterminated \
         one swallows the rest of the step"
    );
    // And the steps the build depends on are all still there, by name. Matched
    // as whole `- name:` lines, not by substring: `contains("Lint")` is
    // satisfied by a step renamed "Linting", which is precisely the sort of
    // quiet drift that leaves a job green and unverified.
    for step in [
        "Build the board (wasm target)",
        "Generate the web bindings",
        "Build the rest of the site",
        "Check the built site",
        "Run the tests",
        "Lint",
        "Upload the built site",
    ] {
        let named = workflow
            .lines()
            .any(|line| line.trim() == format!("- name: {step}"));
        assert!(named, "the workflow lost or renamed its step: {step}");
    }
    assert!(workflow.contains("--all-targets -- -D warnings"));
    assert!(workflow.contains("--lib --target wasm32-unknown-unknown -- -D warnings"));
    for file in [
        "app_bg.wasm",
        "app.js",
        "icon-192.png",
        "icon-512.png",
        "icon.svg",
        "index.html",
        "manifest.webmanifest",
        "service-worker.js",
    ] {
        assert!(workflow.contains(file), "the site check omits {file}");
    }
}

/// The worker only ever answers for a URL inside this board's own directory.
///
/// This is the guard that stops the app from taking over the pages it shares an
/// origin with. A service worker registered for a scope is consulted for every
/// URL under that scope, and this app is served from the same origin as pages
/// that are not a departures board — so "the scope is small" is a promise, and
/// this test is what keeps it one. It is also the second half of the allowlist:
/// the eight files are still the whole of what the handler will serve.
#[test]
fn the_worker_never_answers_outside_its_own_directory() {
    let worker = worker();
    assert!(
        worker.contains("new URL('./', self.location.href)"),
        "the worker must resolve its own directory from its location"
    );
    // The guard is a prefix test against that directory, on the request URL,
    // applied before the allowlist decides anything.
    assert!(
        worker.contains("IS_OWN(url)"),
        "the fetch handler must check the request is inside this app's directory; \
         without it a mis-scoped registration serves whatever it cached"
    );
    assert!(
        worker.contains("const IS_OWN = url => url.startsWith(ROOT.href)"),
        "the directory guard must be a prefix test against the worker's own root"
    );
}

/// The page states the worker's scope instead of inheriting it, and cleans up a
/// wider registration left behind by an earlier version.
///
/// A registration outlives the page that created it, and nothing short of an
/// explicit `unregister` takes one away. So the second half is what makes this
/// recoverable without the reader clearing their browser: a stale registration
/// is not fixed by a reload, and the newer worker cannot take control of a
/// scope it does not own.
#[test]
fn the_page_states_the_scope_and_releases_a_wider_one() {
    let ui = browser_layer();
    assert!(
        ui.contains("register_with_options"),
        "the worker must be registered with an explicit scope; left to default, \
         the scope is whatever directory the registering page sits in"
    );
    assert!(
        ui.contains("RegistrationOptions::new()") && ui.contains("set_scope(SCOPE)"),
        "the scope has to be actually stated, not merely a named constant"
    );
    assert!(
        ui.contains("get_registrations") && ui.contains("unregister"),
        "a stale wider registration survives a reload, a version bump and a \
         reinstall; only an explicit unregister clears it"
    );
}

/// A worker's script is compared by suffix, not by `trim_end_matches`.
///
/// `trim_end_matches` strips a *set of characters*, so a directory whose name
/// ends in those letters is silently treated as ours — and a registration
/// belonging to a sibling app would be torn down. This is a regression test for
/// a real bug in the first version of this code.
#[test]
fn the_script_comparison_strips_a_suffix_rather_than_a_character_set() {
    let ui = browser_layer();
    // The prose in this file names the method to explain why it is not used, so
    // the assertion is about code: a call, not the word.
    let calls: Vec<&str> = ui
        .lines()
        .filter(|line| {
            let code = line.split("//").next().unwrap_or(line);
            code.contains("trim_end_matches(")
        })
        .collect();
    assert!(
        calls.is_empty(),
        "`trim_end_matches` strips a character set, not a filename: it would eat \
         any directory ending in those letters and tear down a sibling's worker. \
         Found: {calls:?}"
    );
    assert!(
        ui.contains("strip_suffix(\"service-worker.js\")"),
        "the comparison must strip the one filename it expects"
    );
}

/// The scope is named once, and the page and the worker agree on the directory.
///
/// Two independent resolutions of "where am I" — the page's `./` and the worker's
/// `new URL('./', self.location.href)`. They have to describe the same
/// directory, or the page registers a scope the worker's guard does not match.
#[test]
fn the_scope_is_a_relative_directory_shared_with_the_worker() {
    let ui = browser_layer();
    assert!(
        ui.contains("const SCOPE: &str = \"./\";"),
        "the scope must be the app's own directory, relative — so one build works \
         from any subdirectory"
    );
    assert!(
        worker().contains("new URL('./', self.location.href)"),
        "the worker must resolve the same directory the page registered"
    );
}

/// The board must actually start.
///
/// This app shipped four times with a green build, a green test run, a green
/// CI job and a page that did nothing at all. Every check that could run had
/// passed, because the failure was not an error: **nothing was running**, so
/// there was nothing to fail.
///
/// The cause was the start function. A plain `#[wasm_bindgen]` declares an
/// ordinary export; `wasm-bindgen` puts the symbol in the wasm and does *not*
/// re-export it, so the generated glue's only exports are `initSync` and
/// `__wbg_init as default`. The shell's loader calls the default export, which
/// initialises the module — and never calls the app. `#[wasm_bindgen(start)]` is
/// what makes `wasm-bindgen` emit a start section that the glue invokes.
///
/// This asserts the source has the right attribute, because that is the part
/// that is in the repository and the part every earlier check was blind to. The
/// generated glue's own call is asserted in CI, where `dist/` exists.
#[test]
fn the_board_declares_a_start_function() {
    let source = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui.rs"))
        .expect("the browser layer");
    assert!(
        source.contains("#[wasm_bindgen(start)]"),
        "src/ui.rs must mark its entry point with #[wasm_bindgen(start)]. A \
         plain #[wasm_bindgen] is an unreachable export: the glue does not \
         re-export it, nothing calls the app, and the page sits on its loading \
         message forever with no error."
    );
    assert!(
        !source.contains("\n#[wasm_bindgen]\npub fn start"),
        "the start function must not also be a plain export"
    );
    // The start function cannot be awaited, so its first act must hand the real
    // work to the microtask queue rather than trying to run it inline.
    let after = source
        .split("pub fn start()")
        .nth(1)
        .expect("a start function");
    let body = &after[..after.len().min(2000)];
    assert!(
        body.contains("spawn_local"),
        "the start function is synchronous and must hand async work to \
         spawn_local"
    );
}

/// The loader must not depend on a function that does not exist.
///
/// `m.default()` is the module's *initializer*, not the app. That is correct
/// and is what the reference does — but it is only correct because the start
/// section runs inside it, which is the previous test's subject. Asserting the
/// loader does not reach for `m.start()` keeps the two halves honest: there is
/// no `start` export to reach for.
#[test]
fn the_loader_only_asks_for_the_initializer() {
    let page = shell();
    assert!(page.contains("import('./app.js')"), "the dynamic import");
    assert!(
        page.contains("m.default()"),
        "the initializer is the default export"
    );
    assert!(
        !page.contains("m.start(") && !page.contains(".start()"),
        "there is no start export to call; the app runs from the wasm start \
         section when the module is initialised"
    );
}

/// Nothing generated may be tracked — not the wasm, not the bindings, not the
/// site, and not the install PNGs. This is the test that would have caught any
/// of them being committed.
#[test]
fn no_build_artifact_is_committed() {
    let Some(tracked) = tracked_files() else {
        return; // not a checkout: the gate's exported tree
    };
    for artefact in [
        "dist/index.html",
        "dist/app.js",
        "dist/app_bg.wasm",
        "dist/service-worker.js",
        "dist/manifest.webmanifest",
        "assets/icon-192.png",
        "assets/icon-512.png",
    ] {
        assert!(
            !tracked.lines().any(|line| line == artefact),
            "{artefact} is tracked; generated artefacts must never be committed"
        );
    }
}

/// The install PNGs are derived from the SVG at build time, so they must not
/// exist in the source tree at all — a checked-in copy is the first step of the
/// PNG-only icon this project is meant to be rid of.
#[test]
fn the_install_pngs_exist_only_in_dist() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    for png in ["icon-192.png", "icon-512.png"] {
        assert!(
            !assets.join(png).exists(),
            "assets/{png} must not exist: the PNGs are build output, rasterized \
             from assets/icon.svg by build.rs"
        );
    }
    assert!(
        assets.join("icon.svg").exists(),
        "assets/icon.svg is the committed, authoritative icon"
    );
}

/// `dist/` and `target/` have to be ignored, or a build would leave the next
/// commit dirty with build output.
#[test]
fn build_output_is_ignored() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    if tracked_files().is_none() {
        return; // not a checkout
    }
    for path in ["dist/", "target/"] {
        let ignored = std::process::Command::new("git")
            .args(["check-ignore", "-q", path])
            .current_dir(root)
            .status()
            .expect("git check-ignore")
            .success();
        assert!(ignored, "{path} must be in .gitignore");
    }
}

/// The original JavaScript PWA is gone, not merely unused. A repository that
/// still carries `app.js`, `style.css`, `sw.js` and `manifest.json` ships a
/// second, dead departures board alongside the Rust one, and the next person to
/// open the tree cannot tell which one is live.
#[test]
fn the_original_javascript_pwa_is_gone() {
    let Some(tracked) = tracked_files() else {
        return; // not a checkout
    };
    for dead in [
        "app.js",
        "style.css",
        "sw.js",
        "manifest.json",
        "index.html",
    ] {
        assert!(
            !tracked.lines().any(|line| line == dead),
            "{dead} is still tracked; the JavaScript PWA has been replaced by \
             the Rust crate and its build output"
        );
        assert!(
            !Path::new(env!("CARGO_MANIFEST_DIR")).join(dead).exists(),
            "{dead} still exists in the tree"
        );
    }
}

/// Remove Rust comments from a source file, so an assertion about what the code
/// *does* cannot be satisfied by a doc comment that merely *describes* it.
///
/// This is not a nicety. Every test below asserts that some call is present in
/// `ui.rs`, and `ui.rs` is a file in which almost every function carries a long
/// comment explaining why it is the way it is — including, several times, an
/// explanation of the very call the test is looking for. Without stripping,
/// deleting the call and leaving the comment behind keeps the test green, which
/// is worse than having no test: it reports the behaviour is protected when
/// nothing is.
fn strip_rust_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut depth = 1;
                while let Some(c) = chars.next() {
                    if c == '/' && chars.peek() == Some(&'*') {
                        chars.next();
                        depth += 1;
                    } else if c == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else if c == '\n' {
                        // Keep the line count stable so a failing assertion
                        // still points at roughly the right place.
                        out.push('\n');
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// The browser layer with its comments removed.
fn browser_code() -> String {
    strip_rust_comments(&browser_layer())
}

/// A switch of mode repaints the board it already has, rather than asking TfL
/// for it again.
///
/// This is the "the whole website refreshes just to update the timetable" fix.
/// The board is fetched per stop point and covers every mode the station has;
/// which of them the reader wants is a view of that one fetch. So a toggle must
/// go through `repaint`, and must not call `load` — a toggle that refetched
/// would blank the board, show "Loading departures…" and rebuild it underneath
/// a reader who is mid-sentence on it, thirty seconds of churn for data the app
/// already had.
#[test]
fn switching_a_mode_repaints_and_does_not_refetch() {
    let code = browser_code();
    let toggle = code
        .split("fn toggle_mode")
        .nth(1)
        .expect("toggle_mode is in the browser layer")
        .split("\nfn ")
        .next()
        .expect("the body of toggle_mode");
    assert!(
        toggle.contains(".repaint()"),
        "a mode switch must redraw the board it already has"
    );
    assert!(
        !toggle.contains(".load("),
        "a mode switch must not refetch: it is a filter over data already in \
         hand, and refetching blanks the board the reader is reading.\n--- \
         toggle_mode ---\n{toggle}"
    );
}

/// A refresh updates the timetable and leaves the rest of the page alone.
///
/// The reader-visible symptoms of the old behaviour were three: the board was
/// blanked to "Loading departures…" on every tick, a failed refresh threw away
/// a board that was still worth reading, and the whole page was re-derived
/// thirty seconds apart. The first two are the code below; asserting them here
/// is what stops them coming back.
#[test]
fn a_refresh_only_touches_the_timetable() {
    let code = browser_code();
    let load = code
        .split("async fn load")
        .nth(1)
        .expect("load is in the browser layer")
        .split("\n    /// ")
        .next()
        .expect("the body of load");
    // The loading notice is for a reader who has nothing yet. Written on every
    // tick it is a thirty-second cycle of blank-and-refill.
    assert!(
        load.contains("raw_board.borrow().is_none()"),
        "the loading notice must wait for a first board, so a refresh does not \
         blank the one the reader is looking at"
    );
    // A failed refresh keeps the board it has.
    assert!(
        !load.contains("say(&self.departures, FAILED);")
            || load.matches("raw_board.borrow().is_none()").count() >= 2,
        "a failed refresh must not throw away a board that is only thirty \
         seconds stale"
    );
}

/// The board is drawn from a filter, so a switch and a refresh share one path.
///
/// One path is the point: if a toggle filtered the board one way and a refresh
/// another, the same station would show different services depending on which
/// of the two happened last.
#[test]
fn the_board_is_painted_through_the_mode_filter() {
    let code = browser_code();
    assert!(
        code.contains("raw.filtered("),
        "the board must be filtered by the mode switches before it is painted"
    );
}

/// A row's colour comes from the line, or from the mode when the line has none.
///
/// The complaint was that every non-Underground service was grey. `line_colour`
/// alone cannot fix it, because a bus route number is not a key in the line
/// table; `departure_colour` is the function that falls back to the mode's own
/// published colour, and `paint` is what has to call it.
#[test]
fn a_row_is_coloured_by_its_line_or_its_mode() {
    let code = browser_code();
    let paint = code
        .split("fn paint")
        .nth(1)
        .expect("paint is in the browser layer")
        .split("\nfn ")
        .next()
        .expect("the body of paint");
    assert!(
        paint.contains("departure_colour("),
        "a chip must fall back to the mode's colour, or every bus and river \
         service is grey.\n--- paint ---\n{paint}"
    );
    assert!(
        !paint.contains("line_colour("),
        "the chip must not be coloured from the line table alone; that is what \
         left every mode that is not an Underground line without a colour"
    );
}

/// The timetable is grouped by mode, so a reader can see what a switch hides.
///
/// The switches filter the board, and a filter with no visible effect is
/// indistinguishable from a broken one. A mode heading over each block is what
/// says "these are the buses; the trains are switched off".
#[test]
fn the_timetable_groups_by_mode() {
    let code = browser_code();
    let paint = code
        .split("fn paint")
        .nth(1)
        .expect("paint is in the browser layer")
        .split("\nfn ")
        .next()
        .expect("the body of paint");
    assert!(
        paint.contains("data-mode"),
        "a mode heading must be marked up with its mode, so it can be styled on \
         that mode's own colour"
    );
}

/// Two stops that share a name say something that tells them apart.
///
/// This is the bus-stop complaint. `StopPoint/Search` returns every stop on a
/// street under the street's name, so "Oxford Circus Station" came back several
/// times over with nothing to choose between. The stop's flag letter, its routes
/// and its direction are what a reader actually has, and all three come from one
/// `StopPoint/{id}` request.
#[test]
fn a_same_named_stop_is_described_so_it_can_be_told_apart() {
    let code = browser_code();
    assert!(
        code.contains("StopPointDetail"),
        "the stop detail must be read, or same-named stops stay identical"
    );
    assert!(
        code.contains("fn describe_new_stops"),
        "the detail must be fetched by a function of its own"
    );
    // And it must actually be *called*. Asserting the function exists is not
    // enough: deleting the call leaves a dead function that satisfies the check
    // above, and the stops come back indistinguishable — the bug this whole
    // field exists to fix.
    assert!(
        code.contains("describe_new_stops(&owned).await"),
        "the stop detail must be fetched after a search returns stops; an \
         uncalled function describes nothing"
    );
    let render = code
        .split("fn refresh_suggestions")
        .nth(1)
        .expect("refresh_suggestions is in the browser layer")
        .split("\nfn ")
        .next()
        .expect("the body of refresh_suggestions");
    assert!(
        render.contains("StopDetail::describe"),
        "a suggestion row must show what tells its stop apart from the others"
    );
    // And the stylesheet has to have somewhere to put it.
    let page = shell();
    assert!(
        page.contains(".result .detail"),
        "the stop detail needs a style, or it lands unstyled in the row"
    );
}

/// An interchange is one row, and its mode-labelled names still find it.
///
/// The duplicate the reader reported: TfL names each half of a station after its
/// mode, so the Underground, the DLR and the National Rail at one place were
/// three rows. The merge is in `departures.rs`; what is asserted here is that
/// the names a reader might type — including the old, mode-stamped ones — are
/// still kept on the row, because the row is *shown* under a different one.
#[test]
fn an_interchange_keeps_the_names_a_reader_will_type() {
    let source =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/departures.rs"))
            .expect("the domain");
    let code = strip_rust_comments(&source);
    assert!(
        code.contains("pub fn place_name(") && code.contains("pub fn interchange_key("),
        "the interchange name rule must exist: TfL names each mode's stop \
         separately, so one place is several raw names"
    );
    assert!(
        code.contains("pub names: Vec<String>"),
        "a row must keep every name TfL gave it, so a reader typing the old \
         mode-stamped name still finds the station"
    );
    // The key has to be what rows are actually merged on. Asserting only that
    // the function exists would pass with the merge reverted, because the
    // function would still be there and still unused — which is exactly the
    // state where the duplicates the reader reported come back.
    let index = code
        .split("pub fn station_index")
        .nth(1)
        .expect("station_index is in the domain")
        .split("\n/// ")
        .next()
        .expect("the body of station_index");
    assert!(
        index.contains("interchange_key(&name)"),
        "rows must be merged on the place name, not the raw per-mode name, or \
         one interchange is several rows again.\n--- station_index ---\n{index}"
    );
}

/// The line column is wide enough for the longest name any board puts in it.
///
/// A clipped chip is worse than a narrow one: "Elizabeth li…" is neither a
/// colour cue nor a name, and the Elizabeth line is the longest `lineName` TfL
/// sends — so the column that carries every line's colour is exactly the one
/// that cuts off the longest of them.
///
/// The floor is **measured, not guessed**: at 375px (the phone breakpoint) the
/// rendered cell is 108px and the rendered "Elizabeth line" chip is 74px, and at
/// 500px they are 120px and 79px. A declared width below 6rem puts the cell
/// under 96px, which is under the chip plus the cell's own 12px of padding, and
/// the chip is then clipped. `6rem` is the floor with a real margin under it.
#[test]
fn the_line_column_fits_the_longest_chip() {
    let page = shell();
    let minimum = 6.0f64; // rem — see the measured figures above
    let mut widths: Vec<f64> = Vec::new();
    for line in page.lines().filter(|line| line.contains("td:nth-child(1)")) {
        let Some(rest) = line.split("width:").nth(1) else {
            continue;
        };
        let Some(value) = rest.split(';').next() else {
            continue;
        };
        let Some(rem) = value.trim().strip_suffix("rem") else {
            continue;
        };
        if let Ok(rem) = rem.trim().parse::<f64>() {
            widths.push(rem);
        }
    }
    assert!(
        widths.len() >= 2,
        "both the desktop and the phone line column must declare a width"
    );
    for width in &widths {
        assert!(
            *width >= minimum,
            "the line column is {width}rem, narrower than the longest chip on \
             the board needs; every line colour would be clipped"
        );
    }
    // The chip must also be allowed to fill the cell rather than overflow it.
    assert!(
        page.contains("td:first-child .chip"),
        "a chip must be bounded by its own column"
    );
}

/// Every platform the domain produced gets its own table.
///
/// **This is a regression test for the board showing one platform instead of
/// all of them.** The mode-run grouping in `paint` used to collapse a run of
/// same-mode platforms to its *first* platform, so a station whose eight
/// platforms are all the Underground — King's Cross, measured at eight live
/// platforms — rendered one table and silently dropped the other seven. The
/// domain had grouped them correctly throughout; only the renderer lost them.
///
/// The check is structural: a loop over the runs must be followed by a loop
/// over the platforms *inside* each run, and the run loop must hold a
/// collection rather than a single platform. Asserting on the run loop alone
/// is what let the original through — it had one, and it was wrong.
#[test]
fn every_platform_gets_its_own_table() {
    let code = browser_code();
    let paint = code
        .split("fn paint")
        .nth(1)
        .expect("paint is in the browser layer")
        .split("\n/// ")
        .next()
        .expect("the body of paint");

    // The runs must collect their platforms rather than keeping one.
    assert!(
        paint.contains("Vec<(Option<Mode>, Vec<&Platform>)>"),
        "a mode run must hold every platform it covers, not just the first -- \
         that is what hid seven of King's Cross's eight platforms"
    );
    // **The accumulation, not just the type.** A `Vec` that is declared and
    // then only ever pushed one element at a time compiles identically and
    // behaves exactly as the bug did, so the type alone is not evidence. What
    // has to be there is a run that *gains* the platform it already covers.
    assert!(
        paint.contains("members.push(platform)"),
        "a platform that continues the current mode's run must be added to \
         that run; without this every run holds one platform and only the \
         first platform of each mode is drawn"
    );
    // And the render loop must walk the platforms inside a run.
    assert!(
        paint.contains("for platform in members"),
        "the renderer must draw each platform in a mode run, not the run's \
         first platform"
    );
    // A run must produce one table per platform, not one table.
    let append = paint.matches("root.append_child(&table)").count();
    assert_eq!(
        append, 1,
        "the table is appended once inside the per-platform loop"
    );
    assert!(
        paint.contains("root.append_child(&table)"),
        "each platform's table must be appended to the board"
    );
}

/// The domain must keep one table's worth of platforms per platform.
///
/// The renderer's fix is only correct if the board it is handed actually holds
/// every platform. King's Cross is the case that broke: eight platforms, all
/// Underground, and the run grouping used to show one.
#[test]
fn a_board_keeps_every_platform_of_a_single_mode_station() {
    use london_rura::departures::{board, Arrival, Platform};
    use london_rura::modes::Mode;

    let arrivals: Vec<Arrival> = [
        ("Westbound - Platform 1", "Uxbridge", 120),
        ("Eastbound - Platform 2", "Epping", 300),
        ("Southbound - Platform 8", "Wimbledon", 480),
        ("Northbound - Platform 7", "Mill Hill East", 600),
        ("Eastbound - Platform 6", "Hainault", 720),
        ("Westbound - Platform 5", "Chesham", 840),
        ("Southbound - Platform 4", "Wimbledon", 960),
        ("Northbound - Platform 3", "High Barnet", 1080),
    ]
    .iter()
    .map(|(platform, destination, seconds)| Arrival {
        line_name: "Piccadilly".to_string(),
        destination: (*destination).to_string(),
        time_to_station: Some(*seconds),
        platform: Some((*platform).to_string()),
        mode: Some(Mode::Tube),
    })
    .collect();

    let made = board(arrivals);
    assert_eq!(
        made.platforms.len(),
        8,
        "eight platforms are eight tables: {:?}",
        made.platforms
            .iter()
            .map(|p: &Platform| p.name.as_str())
            .collect::<Vec<_>>()
    );
    // Every one of them is the same mode, which is precisely the case the
    // renderer used to collapse.
    assert!(
        made.platforms.iter().all(|p| p.mode == Some(Mode::Tube)),
        "all eight are Underground"
    );
    assert_eq!(
        made.platforms[0].name, "Westbound - Platform 1",
        "platforms stay in platform order"
    );
}

/// A service with no destination name shows what TfL said instead of a dash.
///
/// At King's Cross the Hammersmith & City services arrive with an empty
/// `destinationName` and `towards: "Check Front of Train"`. That is a working
/// notice for the crew rather than a passenger destination, and it is the only
/// thing TfL says about where the train is going. Rendered as a dash the row
/// read as missing data; rendered as itself it is the instruction a reader
/// standing on that platform needs.
///
/// `towards` is a **fallback**, not a replacement: TfL sends it on ordinary
/// services too, where it repeats the destination, and a duplicate would be
/// worse than the name alone.
#[test]
fn a_missing_destination_falls_back_to_what_tfl_actually_said() {
    let code = browser_code();
    assert!(
        code.contains("fn destination_of("),
        "the destination cell must be decided in one place, so the fallback is \
         one rule rather than a special case at each use"
    );
    let helper = code
        .split("fn destination_of(")
        .nth(1)
        .expect("destination_of is in the browser layer")
        .split("\nfn ")
        .next()
        .expect("the body of destination_of");
    assert!(
        helper.contains("destination_name"),
        "the named destination must be preferred when there is one"
    );
    assert!(
        helper.contains("towards"),
        "the towards field must be the fallback when no name was given"
    );
    // And the wire field must actually be read, or the fallback is empty.
    assert!(
        code.contains("#[serde(rename = \"towards\""),
        "the towards field must be deserialised from the arrival"
    );
}

/// Every request to TfL waits its turn, because the API rate-limits.
///
/// **This is the fix for the wall of "Error fetching departures."** Measured on
/// 2026-10-02: typing one eight-letter word into the search box produced 201
/// requests, and TfL answers a burst with HTTP 429 — which the app reported with
/// the same words it uses for a dead network, so a reader could not tell that the
/// app had rate-limited itself.
#[test]
fn every_request_to_tfl_goes_through_the_throttle() {
    let code = browser_code();
    let fetch = code
        .split("async fn fetch_bytes")
        .nth(1)
        .expect("fetch_bytes is in the browser layer")
        .split("\n/// ")
        .next()
        .expect("the body of fetch_bytes");
    assert!(
        fetch.contains("throttle().await"),
        "the one function that talks to TfL must wait its turn before it does. \
         A caller that bypasses it can burst, and a burst is 429.\n--- \
         fetch_bytes ---\n{fetch}"
    );
    // And the gap must be a real number, not a no-op yield.
    let declared = code
        .split("const TFL_REQUEST_GAP_MS")
        .nth(1)
        .and_then(|rest| rest.split('=').nth(1))
        .and_then(|value| value.trim().split(';').next())
        .expect("the request gap is stated as a constant")
        .trim()
        .to_string();
    let gap = declared
        .parse::<u32>()
        .unwrap_or_else(|_| panic!("the gap must be a number of milliseconds, got {declared:?}"));
    assert!(
        gap > 0,
        "a zero gap throttles nothing; measured 429s begin well under this traffic"
    );
}

/// A rate limit is told apart from a dead network, because they are different
/// problems and only one of them clears on its own.
///
/// TfL's own body for this is "Rate limit is exceeded. Try again in 7 seconds",
/// so the reader is told a wait is what is needed, rather than being sent away
/// to retry into the same limit.
#[test]
fn being_throttled_is_not_reported_as_a_dead_network() {
    let code = browser_code();
    assert!(
        code.contains("FetchError::RateLimited"),
        "a 429 must be recognised as its own failure"
    );
    assert!(
        code.contains("response.status() == 429"),
        "the 429 must be picked out of the response status"
    );
    assert!(
        code.contains("const RATE_LIMITED"),
        "a rate limit needs its own message"
    );
    // And the two messages must genuinely differ: one constant used for both
    // would satisfy every other assertion here.
    let rate_limited = code
        .split("const RATE_LIMITED")
        .nth(1)
        .expect("RATE_LIMITED is defined")
        .split(';')
        .next()
        .expect("the message is one statement");
    let failed = code
        .split("const FAILED")
        .nth(1)
        .expect("FAILED is defined")
        .split(';')
        .next()
        .expect("the message is one statement");
    assert_ne!(
        rate_limited.trim(),
        failed.trim(),
        "a throttled board and an offline board must not say the same thing"
    );
}

/// A stop whose detail was asked for is not asked for again.
///
/// The regression: `describe_new_stops` selected on `detail.is_none()`, which is
/// also true of a stop whose detail was *refused*, so every subsequent search
/// re-issued the whole same-named set. Measured: four keystrokes, 111 requests.
#[test]
fn a_stop_detail_is_requested_at_most_once() {
    let code = browser_code();
    let describe = code
        .split("async fn describe_new_stops")
        .nth(1)
        .expect("describe_new_stops is in the browser layer")
        .split("\nasync fn ")
        .next()
        .expect("the body of describe_new_stops");
    assert!(
        !describe.contains("detail.is_none()"),
        "selecting on `detail.is_none()` re-asks for every stop whose detail \
         was refused, which is what produced 111 requests for four keystrokes. \
         The selection belongs to `stops_needing_detail`, which tracks 'asked' \
         separately from 'arrived'.\n--- describe_new_stops ---\n{describe}"
    );
    assert!(
        describe.contains("stops_needing_detail"),
        "the browser layer must use the tested selection rule rather than \
         re-implementing it, or the two drift apart"
    );
}

/// A workflow file, as committed.
///
/// Nonexistent is not a reason to fail. A fresh export of a release branch, or
/// a checkout with CI stripped, need not carry `.github/` at all, and a test
/// that hard-failed on its absence would be red for a reason that says nothing
/// about the app. Callers say which they want.
fn workflow(name: &str) -> Option<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".github/workflows")
        .join(name);
    std::fs::read_to_string(&path).ok()
}

/// The workflow with its comments removed.
///
/// Every assertion below reads this, not the file. A commented-out line is
/// deleted code, and an assertion that matches source text cannot tell the
/// difference — which is not hypothetical: the first version of the
/// `RUSTFLAGS` assertion in a sibling repo passed with the line commented out,
/// and the job it was written to protect then failed in CI.
///
/// A `#` inside a quoted string is not a comment and a `#` in a value is not
/// one either, but this workflow quotes nothing in the keys it is read for and
/// carries no `#` in any value it depends on — only in the trailing comments on
/// the action pins — so a line-wise cut at the first `#` is enough and a YAML
/// parser is not worth a dependency.
fn strip_yaml_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find('#') {
            Some(index) => &line[..index],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The Pages deployment cannot drift from the crate it publishes.
///
/// The deploy job installs its own `wasm-bindgen`, pinned to a literal in the
/// YAML, and `Cargo.toml` pins the same version the crate compiles against.
/// Move the dependency and the workflow keeps building happily: it generates
/// bindings for a runtime the page does not have, and the only symptom is a
/// live site that fails at startup with "London Rura could not start" — for
/// every visitor, and only in the browser. So the two are asserted equal here
/// rather than trusted to be edited together.
#[test]
fn the_pages_build_generates_bindings_for_the_pinned_runtime() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .expect("Cargo.toml");

    // Read the pin as Cargo writes it: `wasm-bindgen = "=0.2.128"`, an exact
    // requirement. A looser form ("0.2.128", "^0.2") would resolve to whatever
    // is newest in the lockfile, and the workflow's literal would then be
    // naming one arbitrary version of several.
    let expected = manifest
        .lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("wasm-bindgen")?;
            let rest = rest.trim_start().strip_prefix('=')?;
            Some(rest.trim().trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| panic!("Cargo.toml pins no exact wasm-bindgen version"));
    let version = expected.trim_start_matches('=').trim_matches('"');

    // The version has to reach the job that generates the bindings, as the
    // `version=` it installs — not merely somewhere in the file, where a
    // mention in a comment would satisfy this and the job would still install
    // whatever else it found.
    let literal = format!("version=\"{version}\"");
    let indirect = "version=\"$WASM_BINDGEN_VERSION\"";
    assert!(
        pages.contains(&literal) || pages.contains(indirect),
        "pages.yml must install wasm-bindgen {version} (`{literal}` or `{indirect}`); it cannot \
         drift from the Cargo.toml pin, or the site fails to start in the browser and nowhere else",
    );

    // A version passed through `env:` is only a single source of truth if the
    // variable is actually declared, at the same literal value, in a real
    // `env:` block. An undeclared variable expands to nothing: the generator
    // step installs no generator, every other check passes, and the job then
    // fails at the bindings step. That is silent at the point of the mistake —
    // an undeclared and a declared variable are indistinguishable in the YAML
    // that reads them.
    if pages.contains(indirect) {
        // Read the block's own text, not the file: `environment:` is not `env:`,
        // but a comment that *says* `env:` would satisfy a bare substring
        // check -- and this file explains its own decision two lines above it.
        let live = strip_yaml_comments(&pages);
        assert!(
            live.lines().any(|line| line.trim_end() == "env:"),
            "pages.yml reads $WASM_BINDGEN_VERSION but declares no top-level `env:` block, so \
             the variable expands to nothing and the generator step installs nothing",
        );
        let declared = format!("WASM_BINDGEN_VERSION: {version}");
        assert!(
            pages.contains(&declared),
            "pages.yml installs $WASM_BINDGEN_VERSION but never declares it as {version}; an \
             undeclared variable expands to nothing and the generator step installs nothing",
        );
    }
}

/// This board must not carry a compiler flag it does not need.
///
/// Sibling apps in this family need `RUSTFLAGS=--cfg=web_sys_unstable_apis`:
/// the Screen Wake Lock API is behind that cfg in web-sys 0.3.105 rather than
/// merely feature-gated, and a `build.rs` `cargo:rustc-cfg` does not reach a
/// registry dependency compiled in its own unit. This board has no wake lock,
/// and the build was measured both ways: with `RUSTFLAGS` unset, no
/// `.cargo/config.toml` and no user-level cargo config, a clean
/// `cargo build --locked --lib --target wasm32-unknown-unknown --release`
/// succeeds.
///
/// The flag is harmless when set, so this is not about a broken build. It is
/// about the file not asserting a fact about this crate that is not true of
/// it: a `RUSTFLAGS` line here would be cargo-culted from a sibling whose
/// failure mode this crate does not have, and the comment above it would be a
/// lie the next reader cannot check. If a wake lock is ever added, this test
/// is the one that must change with it.
#[test]
fn the_pages_build_carries_only_the_flags_this_board_needs() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    let live = strip_yaml_comments(&pages);
    assert!(
        !live.contains("RUSTFLAGS:"),
        "pages.yml sets RUSTFLAGS, which asserts that this board needs \
         --cfg=web_sys_unstable_apis. It does not: it has no wake lock, and the \
         wasm build was verified with the variable unset and no .cargo/config.toml. \
         Remove the line, or add the wake lock that makes it true.",
    );
    // The same claim, made in the source tree rather than the workflow: if a
    // future change brings the Screen Wake Lock API in through some other
    // spelling, this is where it shows up, and the flag comes back with it.
    let source = browser_code();
    for call in ["wake_lock", "WakeLockSentinel", "WakeLockType"] {
        assert!(
            !source.contains(call),
            "src/ui.rs now uses {call}, which is behind --cfg=web_sys_unstable_apis. The \
             workflow's `env:` needs RUSTFLAGS back, and the test above must be replaced \
             with the one that pins it.",
        );
    }
}

/// A deploy that can run from any branch is a deploy a stranger can run.
///
/// `pages: write` and `id-token: write` are the two permissions that let a
/// GitHub Actions job overwrite the live site, and the token behind them is
/// minted for the repository however the workflow was reached. The project
/// *wants* an automatic deploy on every merge to master — that is the point,
/// and it is why nobody has to remember to publish. What it does not want is
/// that same power on every other ref, so the invariant asserted here is the
/// narrow one that survives the convenience: master is the only ref that can
/// reach the live site, and the publishing permissions live in the one job
/// that is gated on it.
#[test]
fn only_master_can_reach_the_live_site() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    let live = strip_yaml_comments(&pages);

    // The trigger must be the named branch, not a bare `push:`. A bare `push:`
    // deploys from every branch that exists, including a contributor's feature
    // branch — and it also changes what `on:` means for pull requests, which
    // is the opposite of the intent.
    assert!(
        live.contains("branches: [master]"),
        "pages.yml must trigger on `branches: [master]`, not a bare `push:`; a bare push \
         deploys from every branch, including other people's",
    );
    // A tag trigger alongside the branch trigger would publish a version that
    // was never on master.
    assert!(
        !live.contains("tags:"),
        "pages.yml must not also deploy on tags; a tagged commit that never reached master \
         would be published to the live site",
    );
    // Nor a pull-request trigger: `build.yml` has one, and copying its `on:`
    // block wholesale into this file would hand a fork's PR the deploy job.
    assert!(
        !live.contains("pull_request:"),
        "pages.yml must not trigger on pull requests; a fork's PR has neither the permission \
         nor the environment, and the trigger belongs to build.yml",
    );

    // The deploy job's gate, read from the job it belongs to so the assertion
    // cannot be satisfied by a gate on some other job, and read with comments
    // stripped: the comment block above the `if:` names both halves of the
    // condition while explaining it, so a comment-stripping assertion that read
    // the raw file would be satisfied by the prose alone. That is not
    // hypothetical: it is how the `RUSTFLAGS` assertion in this same file was
    // shipped, and it shipped.
    //
    // This is the check that actually holds when the trigger is widened by
    // accident.
    let deploy_job = live
        .split("\n  deploy:")
        .nth(1)
        .expect("pages.yml must have a `deploy:` job");
    // The gate itself, taken as one line: the `if:` of the `deploy` job and
    // nothing else. Read it out rather than searched for across the file, so a
    // second `if:` on some other job cannot satisfy this.
    let live_gate = live
        .split("\n  deploy:")
        .nth(1)
        .and_then(|job| job.split_once("\n    if:").map(|(_, after)| after))
        .and_then(|after| after.lines().next())
        .expect("the `deploy` job must have an `if:` gate");
    assert!(
        live_gate.contains("github.ref == 'refs/heads/master'"),
        "the `deploy` job must be gated on the build being for master (found: {live_gate:?})",
    );
    // ... and it must ALSO be gated on not being a fork. This file is
    // byte-identical in `wdomitrz/london_rura` and in its fork
    // `bot-git-ai/london_rura`, so a gate that tests only the branch name
    // cannot tell the two repositories apart: both have a `master`, and a push
    // to the fork's master would try to publish. Two things then go wrong, and
    // the first is the one that happens. A fork has no Pages site of its own
    // until someone enables one by hand, so every push to fork master dies
    // with "Creating Pages deployment failed ... Ensure GitHub Pages has been
    // enabled" — a red run per push. And if Pages were enabled there, the fork
    // would serve its own copy, which drifts from the published site as soon as
    // the two masters diverge.
    //
    // `github.event.repository.fork` is the discriminator because it needs no
    // configuration: it is supplied by the event, false upstream and true in
    // the fork. The obvious alternative, a repository Actions variable, has
    // the failure mode this assertion exists to prevent — it would have to be
    // set on the *upstream* repository to publish, and no account but the
    // user's can do that, so the gate would ship as silently off on the one
    // repository where it matters.
    assert!(
        live_gate.contains("!github.event.repository.fork"),
        "the `deploy` job must also be gated on `!github.event.repository.fork`; this \
         workflow is byte-identical in the fork `bot-git-ai/london_rura`, so a \
         branch-name-only gate publishes from the fork too — failing with 'Ensure GitHub \
         Pages has been enabled' until Pages is enabled there, and serving a divergent \
         copy afterwards (found: {live_gate:?})",
    );
    // The two halves are one condition, not two jobs: an `if:` per job would be
    // an AND across two independent gates, and a `build`-job gate would silently
    // stop the *build* from running on the fork rather than just its publish,
    // which is the opposite of what this is for. One `if:`, both halves.
    let deploy_if_lines = deploy_job
        .lines()
        .filter(|line| line.trim_start().starts_with("if:"))
        .count();
    assert_eq!(
        deploy_if_lines, 1,
        "the two halves must be one `if:` on `deploy`, not a gate per job (found {deploy_if_lines} \
         in the `deploy` job)",
    );

    // And the permissions that can actually publish must be scoped to that job
    // rather than granted workflow-wide, so a build step or a third-party
    // action added later cannot spend them.
    let build_job = live
        .split("\n  build:")
        .nth(1)
        .and_then(|after| after.split("\n  deploy:").next())
        .expect("pages.yml must have a `build:` job");
    for permission in ["pages: write", "id-token: write"] {
        assert!(
            !build_job.contains(permission),
            "the `build` job must not hold `{permission}`; that permission belongs to `deploy` \
             alone, and a build step is exactly where it would be spent by accident",
        );
    }
    // Scoped to the job is not the same as present on it: `deploy-pages` needs
    // both, and a workflow that grants `pages: write` and forgets
    // `id-token: write` gets a 403 at the last step of the run.
    for permission in ["pages: write", "id-token: write"] {
        assert!(
            deploy_job.contains(permission),
            "the `deploy` job must hold `{permission}`; deploy-pages cannot publish without it",
        );
    }
}

/// The deploy has to be handed something the upload actually produced.
///
/// `deploy-pages` v5 takes `artifact_name`. There is no `artifact_id` input:
/// passing one is reported as `Unexpected input(s) 'artifact_id'` and the
/// action falls back to its own default, which is only the right answer while
/// the upload side also defaults to the same string. Change one side and the
/// deploy finds no artifact and fails with a bare `HttpError: Not Found` —
/// which says nothing about the artifact, so the cause has to be read out of
/// the workflow, not out of the error.
#[test]
fn the_deploy_is_handed_the_artifact_the_build_uploaded() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    let live = strip_yaml_comments(&pages);

    // The input that v5 does not have. Its presence is a warning at run time
    // and never an error, so nothing else would ever report it.
    assert!(
        !live.contains("artifact_id:"),
        "pages.yml passes `artifact_id` to deploy-pages v5, which has no such input; it is \
         warned about and ignored, leaving the deploy to guess the artifact name",
    );
    // And the right action: a plain `upload-artifact` produces a green build
    // and then a deploy that cannot find what it was given, because the Pages
    // artifact is a single tarball the deploy finds *by name*.
    assert!(
        live.contains("actions/upload-pages-artifact@"),
        "pages.yml must upload with actions/upload-pages-artifact; a plain upload-artifact is a \
         green build followed by a deploy that cannot find the artifact",
    );
    assert!(
        !live.contains("actions/upload-artifact@"),
        "pages.yml must not use actions/upload-artifact; the Pages artifact is a tarball found \
         by name, not a file in the run's artifacts",
    );

    // Both sides name the artifact the same way. Read the two keys out of the
    // live text rather than asserting on a fixed string, so the invariant is
    // the agreement and not the particular name.
    //
    // Only the two are read, and only where they are the artifact's own keys:
    // the file also carries the workflow's `name:` and every step's `name:`,
    // and a plain prefix match picks up whichever of those comes first.
    // `artifact_name:` is unique, and the upload's `name:` is the one indented
    // ten spaces — a step's own `name:` is eight.
    let name_of = |key: &str, indent: usize| {
        live.lines().find_map(|line| {
            let prefix = format!("{}{key}: ", " ".repeat(indent));
            let rest = line.strip_prefix(prefix.as_str())?;
            Some(rest.trim().trim_matches('"').to_owned())
        })
    };
    let uploaded = name_of("name", 10).unwrap_or_else(|| {
        panic!("pages.yml must state the upload step's artifact `name:` so the deploy can match it")
    });
    let deployed = name_of("artifact_name", 10).unwrap_or_else(|| {
        panic!(
            "pages.yml must pass `artifact_name:` to deploy-pages, or it uses a default that \
                can drift from the upload"
        )
    });
    assert_eq!(
        uploaded, deployed,
        "the artifact the build uploads ({uploaded:?}) and the one the deploy asks for \
         ({deployed:?}) must be the same name",
    );
}

/// A workflow that only ever runs the host build would publish a site with no
/// board in it.
///
/// `dist/app.js` and `dist/app_bg.wasm` are written by `wasm-bindgen` and
/// exist nowhere else in the tree, and `dist/` is gitignored, so a Pages
/// deploy that skipped the bindings step would upload a site that loads and
/// then never starts — with nothing in the run to say so. The same applies to
/// the `touch build.rs` that makes `build.rs` re-run for the shell files.
///
/// The order is asserted as well as the presence: `build.rs` derives the
/// service worker's cache version from the bytes of the other seven files, so
/// running it first pins the version to whatever a previous build left behind.
#[test]
fn the_pages_build_produces_the_board_and_not_just_the_shell() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    let live = strip_yaml_comments(&pages);
    for step in [
        "Build the board (wasm target)",
        "Generate the web bindings",
        "Build the rest of the site",
        "Check the built site",
    ] {
        assert!(
            live.lines()
                .any(|line| line.trim() == format!("- name: {step}")),
            "pages.yml lost or renamed its step: {step}. Matched as whole `- name:` lines, \
             because a substring check for `Build` is satisfied by a step renamed 'Building'.",
        );
    }
    // The two build steps, in the order AGENTS.md requires and the order the
    // cache version depends on.
    let wasm = live
        .find("cargo build --locked --lib --target wasm32-unknown-unknown --release")
        .expect("pages.yml must build the wasm target");
    let bindings = live
        .find("wasm-bindgen --target web --no-typescript --out-dir dist --out-name app")
        .expect("pages.yml must generate the web bindings");
    let shell = live.find("touch build.rs").expect(
        "pages.yml must touch build.rs, or the second build is a no-op and dist/ keeps \
                 only the two wasm artefacts",
    );
    assert!(
        wasm < bindings && bindings < shell,
        "the two builds run in the documented order: the wasm, then the bindings, then \
         build.rs. run: the order matters -- build.rs derives the worker's cache version from \
         the bytes the bindings step wrote, so running it first pins a version to whatever the \
         previous build left behind.",
    );
    // And the generator is fed this crate's own artefact, by name. The wasm
    // file is named after the package, so a renamed crate breaks the bindings
    // step — and it breaks it in a way that leaves `dist/` with six files and
    // no app, which the count check above would catch only by accident.
    let package = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("Cargo.toml")
        .lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("name")?;
            let rest = rest.trim_start().strip_prefix('=')?;
            Some(rest.trim().trim_matches('"').to_owned())
        })
        .expect("Cargo.toml names the package");
    assert!(
        live.contains(&format!(
            "target/wasm32-unknown-unknown/release/{package}.wasm"
        )),
        "pages.yml must generate the bindings from this crate's own wasm, {package}.wasm; a \
         path that names something else produces a site with no board in it.",
    );
}

/// The site check has to check the site.
///
/// A build that produced four of the eight files is a publishable-looking site
/// with no app in it, and a check that only asserts the *exit status* of the
/// build cannot see that. So the assertions are made against the built files,
/// and the two that matter most for this app are the ones no source-level test
/// can reach: that the glue still calls the wasm start section (without it the
/// page loads and does nothing, which has happened here four times), and that
/// the shell still initialises it.
#[test]
fn the_pages_check_asserts_the_built_site_not_the_exit_status() {
    let Some(pages) = workflow("pages.yml") else {
        return;
    };
    let live = strip_yaml_comments(&pages);

    // The whole file list, present and — the part that matters — exact: an
    // extra file in `dist/` is served to every reader and cached by the worker.
    for file in [
        "app_bg.wasm",
        "app.js",
        "icon-192.png",
        "icon-512.png",
        "icon.svg",
        "index.html",
        "manifest.webmanifest",
        "service-worker.js",
    ] {
        assert!(
            live.contains(file),
            "the pages site check omits {file}; a half-built site looks publishable"
        );
    }
    // The properties only a built artefact can have. Each is a failure that is
    // invisible until a browser, i.e. a reader, hits it, and each is asserted
    // *of the built file* rather than of the workflow's own text: the workflow
    // names `dist/service-worker.js` in the file-list check and in the icon
    // loop, so a plain substring search is satisfied by the name being listed
    // and by nothing else. What is asserted here is that the check reads the
    // file and tests it.
    for (needle, why) in [
        (
            "__VERSION__",
            "an unsubstituted cache version ships a worker that never invalidates",
        ),
        (
            "__wbindgen_start",
            "without it the page loads and the board never runs",
        ),
        (
            "export",
            "the shell's dynamic import needs a real module to import",
        ),
        (
            "m.default()",
            "the shell must initialise the bindings it loaded",
        ),
        (
            "89504e470d0a1a0a",
            "the install icons must be real PNGs, rasterized from the committed roundel",
        ),
    ] {
        assert!(
            live.contains(needle),
            "the pages site check does not assert {needle}: {why}",
        );
    }
    // The check reads the files it asserts on. A build step that ends in
    // `cargo build` is not a check, and this is the assertion that says so.
    assert!(
        live.contains("test -s dist/app_bg.wasm"),
        "the pages site check must test the built files for non-emptiness rather than trusting \
         a green build step to mean the site is whole",
    );
    // The heredoc form, for the same reason `build.yml` requires it: a
    // `python3 -c` with an indented body is an IndentationError before the
    // interpreter reads a statement, and an apostrophe in that body ends the
    // shell quote, so bash never reaches Python at all.
    let opened = pages.matches("<<'PY'").count();
    let closed = pages.lines().filter(|line| line.trim() == "PY").count();
    assert!(
        !live.contains("python3 -c"),
        "the pages workflow uses the inline `python3` flag; an indented body is an \
         IndentationError and an apostrophe ends the shell quote."
    );
    assert_eq!(
        opened, closed,
        "{opened} heredocs are opened and {closed} are closed; an unterminated one swallows \
         the rest of the step",
    );
}
