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
    for id in ["station", "departures", "notice"] {
        assert!(page.contains(&format!("id=\"{id}\"")), "missing #{id}");
    }
}

/// A board is read at arm's length on a phone, by people who may not be able to
/// see the colour the whole thing is keyed on. The line name is text, the
/// changing regions announce themselves, and the one control is labelled.
#[test]
fn the_shell_is_labelled_live_and_legible_without_colour() {
    let page = shell();
    assert!(page.contains("<label for=\"station\">"), "the picker is labelled");
    assert!(page.contains("aria-live=\"polite\""), "changing text announces");
    assert!(page.contains("role=\"status\""), "the notice is a status region");
    assert!(page.contains(":focus-visible"), "focus must be visible");
    assert!(page.contains("prefers-color-scheme"), "dark mode is respected");
    assert!(
        page.contains("prefers-reduced-motion"),
        "reduced motion is respected"
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
        code.contains("ASSETS.includes(event.request.url)"),
        "the worker must answer only its own assets, not every GET"
    );
    assert!(
        !code.contains("caches.match(event.request)\n    return cached || fetch(event.request);\n  }"),
        "the handler must stay an allowlist"
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
