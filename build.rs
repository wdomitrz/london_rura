// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! Write the static app shell to `dist/` while the crate compiles.
//!
//! The board is a browser application, so publishing it is a file copy and a
//! rasterization, not a program run. `build.rs` owns five of the eight files
//! in `dist/`:
//!
//! * `index.html` is `src/ui.html` byte for byte.
//! * `icon-192.png` and `icon-512.png` are rasterized from `assets/icon.svg`,
//!   the committed and authoritative icon. They are build output and are never
//!   committed; the SVG is never replaced by them.
//! * `manifest.webmanifest` is assembled here with `serde_json`, so its icon
//!   list cannot drift from what was actually rasterized.
//! * `service-worker.js` is `src/service-worker.js` with `__VERSION__`
//!   substituted by a hash of the bytes of everything else.
//!
//! The other three — `app.js` and `app_bg.wasm` from `wasm-bindgen`, and
//! nothing else — are written into the same directory by the `wasm-bindgen`
//! step, which runs *before* this one. So this script writes the files it owns
//! in place, under scratch names and renamed over their targets, and never
//! replaces the directory: a wholesale swap would take the wasm with it and
//! leave a publishable-looking `dist/` with no board in it.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// The app's name, as its original `manifest.json` had it.
const NAME: &str = "London Rura";

/// The app's colours, from the original `manifest.json`.
const THEME_COLOR: &str = "#000000";
const BACKGROUND_COLOR: &str = "#ffffff";

/// The install icon sizes to rasterize, matching the original's 512.
const ICON_SIZES: &[u32] = &[192, 512];

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());

    // The site is built only for the host target. This script also runs during
    // `cargo build --lib --target wasm32-unknown-unknown`, and at that moment
    // `app.js` and `app_bg.wasm` are the *output* of that build: they do not
    // exist yet, so writing the site there fails on the very step that
    // produces them. The host build that follows the `wasm-bindgen` pass is
    // the one that publishes.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.starts_with("wasm") {
        return;
    }

    // Watch the SOURCE paths, not the output names: cargo compares these
    // against real files, so `ui.html` and `service-worker.js` have to be
    // named as the files they are. Watching the destination names watches files
    // that never change, and the script then never re-runs.
    println!("cargo:rerun-if-changed=src/ui.html");
    println!("cargo:rerun-if-changed=src/service-worker.js");
    println!("cargo:rerun-if-changed=assets/icon.svg");
    println!("cargo:rerun-if-changed=build.rs");

    // The files this script owns, each paired with the committed source it comes
    // from. Deliberately not listed: `app.js` and `app_bg.wasm`. Those are
    // written into `dist/` by the `wasm-bindgen` step, and this script must not
    // delete them — they are the board itself.
    let mut built: Vec<(String, Vec<u8>)> = Vec::new();
    built.push((
        "index.html".to_string(),
        read(&root, "src/ui.html").expect("the static shell"),
    ));
    // The committed SVG ships as well as rasterizes: the shell links it
    // directly as the page icon, so a site that published only the PNGs would
    // have lost its own icon's origin.
    built.push((
        "icon.svg".to_string(),
        read(&root, "assets/icon.svg").expect("the icon SVG"),
    ));
    // Both PNGs come from the one committed SVG. They are build output: they
    // live only in `dist/` and are never committed.
    let icon = read(&root, "assets/icon.svg").expect("the icon SVG");
    for size in ICON_SIZES {
        built.push((format!("icon-{size}.png"), rasterize(&icon, *size)));
    }

    // The worker's own template is hashed too, and deliberately kept out of
    // `built` so it is hashed exactly once, in template form. A change to the
    // caching logic must invalidate the cache: clients holding the old worker
    // would otherwise keep running stale logic against new assets.
    let template = read(&root, "src/service-worker.js").expect("the worker template");
    let version = cache_version(&built, &template);

    // The manifest is assembled, not copied, so its icon list is exactly the
    // set of PNGs that were just rasterized.
    let manifest = manifest_json();
    built.push(("manifest.webmanifest".to_string(), manifest.into_bytes()));

    let worker = replace(&template, b"__VERSION__", version.as_bytes());
    assert!(
        !contains(&worker, b"__VERSION__"),
        "the service worker still contains the version placeholder"
    );
    built.push(("service-worker.js".to_string(), worker));

    write_tree(&root.join("dist"), &built);
}

/// Read a committed source file, or `None` with a note.
fn read(root: &Path, name: &str) -> Option<Vec<u8>> {
    match std::fs::read(root.join(name)) {
        Ok(bytes) => Some(bytes),
        Err(error) => panic!("reading {name}: {error}"),
    }
}

/// Rasterize the committed icon SVG to a square PNG of `size`.
///
/// `usvg`/`resvg` are built with `default-features = false`, which drops
/// `text`: this icon needs no font database, because the roundel's wordmark is
/// drawn as paths in the SVG, not as `<text>`. That is why the flag is safe
/// here and why it is the more reproducible choice regardless — a rasterized
/// icon must not depend on which fonts the build machine happens to have.
fn rasterize(svg: &[u8], size: u32) -> Vec<u8> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default())
        .unwrap_or_else(|error| panic!("parsing assets/icon.svg: {error}"));
    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("a square pixmap");
    // Scale to fit and centre the result, so a non-square SVG is letterboxed
    // rather than stretched -- which matters here, because the roundel is
    // 116x94mm and stretching it to a square would shear the wordmark.
    // `usvg`'s `Size` is already f32, which is what `resvg`'s `Transform` wants.
    let natural = tree.size();
    let scale = (size as f32 / natural.width()).min(size as f32 / natural.height());
    let drawn = natural.width() * scale;
    let drawn_y = natural.height() * scale;
    let mut transform = tiny_skia::Transform::from_scale(scale, scale);
    transform = transform.pre_translate((size as f32 - drawn) / 2.0, (size as f32 - drawn_y) / 2.0);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap.encode_png().expect("encoding the install PNG")
}

/// The manifest, as the spec's shape, with the app's own name and colours.
fn manifest_json() -> String {
    let icons: Vec<serde_json::Value> = ICON_SIZES
        .iter()
        .map(|size| {
            serde_json::json!({
                "src": format!("icon-{size}.png"),
                "sizes": format!("{size}x{size}"),
                "type": "image/png",
                "purpose": "any maskable",
            })
        })
        .collect();
    let manifest = serde_json::json!({
        "id": "./",
        "name": NAME,
        "short_name": NAME,
        "start_url": "./",
        "scope": "./",
        "display": "standalone",
        "background_color": BACKGROUND_COLOR,
        "theme_color": THEME_COLOR,
        "icons": icons,
    });
    // Two-space indent, newline at the end: the same shape a human would write.
    let mut text = serde_json::to_string_pretty(&manifest).expect("the manifest");
    text.push('\n');
    text
}

/// A cache name derived from the bytes of every shell file, and the worker's
/// own source.
///
/// It covers the wasm only indirectly — the wasm step runs first and
/// `build.rs` re-runs after it — but it deliberately covers the worker's
/// template: a change to the caching logic must invalidate the cache too, or
/// clients keep running the old logic against new assets.
fn cache_version(built: &[(String, Vec<u8>)], worker_template: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    for (name, bytes) in built {
        name.hash(&mut hasher);
        bytes.hash(&mut hasher);
    }
    worker_template.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Write every file this script owns into `dir`, in place.
///
/// Each is written under a scratch name and renamed over its target, so a host
/// serving the directory never observes a half-written file. The directory
/// itself is not replaced, because the wasm artefacts live in it too.
fn write_tree(dir: &Path, built: &[(String, Vec<u8>)]) {
    std::fs::create_dir_all(dir).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
    for (name, bytes) in built {
        let path = dir.join(name);
        let scratch = dir.join(format!(".{name}.new"));
        std::fs::write(&scratch, bytes)
            .unwrap_or_else(|error| panic!("writing {}: {error}", scratch.display()));
        std::fs::rename(&scratch, &path)
            .unwrap_or_else(|error| panic!("publishing {}: {error}", path.display()));
    }
}

/// The one substitution the worker template gets.
fn replace(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(haystack);
    text.replace(
        std::str::from_utf8(needle).expect("a utf-8 needle"),
        &String::from_utf8_lossy(with),
    )
    .into_bytes()
}

/// Whether a byte string contains another.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
