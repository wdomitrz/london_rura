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
    for id in ["search", "results", "toggles", "departures", "notice", "station-name", "stamp"] {
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
    assert!(page.contains("for=\"search\""), "the search field is labelled");
    assert!(page.contains("placeholder="), "the search field says what it is for");
    assert!(page.contains("role=\"combobox\""), "the search field is a combobox");
    assert!(page.contains("role=\"listbox\""), "the results are a listbox");
    assert!(
        page.contains("visually-hidden"),
        "a label that is only read aloud still has to be hidden from the page"
    );
    assert!(page.contains("aria-live=\"polite\""), "changing text announces");
    assert!(page.contains("role=\"status\""), "the notice is a status region");
    assert!(page.contains(":focus-visible"), "focus must be visible");
    assert!(page.contains("prefers-color-scheme"), "dark mode is respected");
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
    assert!(page.contains("--chip:"), "the chip colour must be a custom property");
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
    assert!(page.contains("--accent"), "the minutes need an accent colour");
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
        !code.contains("caches.match(event.request)\n    return cached || fetch(event.request);\n  }"),
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
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui.rs"),
    )
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
        // Comments and style blocks go entirely.
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
        // A tag: keep the quoted literals out of a <script>, drop the tag.
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
    let closed = workflow
        .lines()
        .filter(|line| line.trim() == "PY")
        .count();
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
    // Both targets are linted, and the site check names all eight files.
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
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui.rs"),
    )
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
    assert!(page.contains("m.default()"), "the initializer is the default export");
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
    for dead in ["app.js", "style.css", "sw.js", "manifest.json", "index.html"] {
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
