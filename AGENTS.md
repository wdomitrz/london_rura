# london_rura

London Rura: a TfL London Underground departures board, private and
browser-only. Every decision the board makes — which stop points are stations,
in what order they are offered, how arrivals sort, how they group into
platforms, the minutes, the line colours — is Rust, compiled to WebAssembly.
`cargo build` writes the whole publishable site into `./dist`. There is no
server, no `serve` subcommand, and no binary at all.

AGPL-3.0-only. See `LICENSE`.

This is a standalone repository. It is the Rust rewrite of the JavaScript PWA at
`github.com/wdomitrz/london_rura`, whose history is preserved below the rewrite
commit; the original `app.js`, `index.html`, `style.css`, `manifest.json` and
`sw.js` were deleted by it, and `tests/shell.rs` asserts they stay gone so the
tree never carries two boards at once.

## Build and run

Two builds, because there are two targets. Nothing generated is committed.

```
# 1. the site: compile the crate to wasm and run the bindings generator
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
cargo build --locked --lib --target wasm32-unknown-unknown --release
wasm-bindgen --target web --no-typescript --out-dir dist --out-name app \
  target/wasm32-unknown-unknown/release/london_rura.wasm

# 2. the rest of the site
touch build.rs
cargo build --release --locked
```

Step 1 writes `dist/app.js` and `dist/app_bg.wasm`; step 2 adds the six files
`build.rs` owns, and derives the service worker's cache version. **The order
matters**: step 2's cache hash covers the wasm, so running it first would pin a
version to whatever the previous build left behind.

`build.rs` writes only the files it owns, in place. It does not replace `dist/`,
so it leaves the wasm alone. Each file is written under a scratch name and
renamed over its target, so a host serving `dist/` never sees a half-written
one.

There is **no run step and no server**: the build is the whole story.

`wasm-bindgen` installs to `~/.cargo/bin`, which is on `PATH` in a normal login
shell; in a bare or non-login shell call it by absolute path
(`~/.cargo/bin/wasm-bindgen`). The version is pinned to `=0.2.128` in
`Cargo.toml` and must match the CLI exactly; a mismatched generator emits
bindings the runtime will not load, and the page then fails to start with
"London Rura could not start". No npm/JS build tool is needed. A tiny
dynamic-import loader in `ui.html` plus the service-worker cache/lifecycle
shell and the generated wasm-bindgen bindings are the only JavaScript in the
app; there is no handwritten JavaScript application and no raw-pointer ABI.

## The static site

`dist/` is the whole site, and nothing in it is committed. The eight files
arrive from two builds:

- `app.js` and `app_bg.wasm` are written by `wasm-bindgen` in step 1. They are
  the board itself and exist nowhere else in the tree.
- `index.html`, `icon.svg`, `icon-192.png`, `icon-512.png` and
  `manifest.webmanifest` are written by `build.rs` in step 2 from committed
  sources, and `service-worker.js` is the sixth, with its cache name pinned to a
  version derived from the bytes of every other file in the directory **and its
  own source** — so a change to the caching logic invalidates the cache too, and
  changing the wasm moves the version.

Everything the shell references is relative (`./app.js`,
`new URL('./', self.location.href)`, `start_url: "./"`), so one build works from
any subdirectory. Any file host can publish it.

## The icon, and where it came from

Every other app in this family keeps the author's original `icon.svg` byte for
byte. **This one could not**: the original `manifest.json` pointed at a
Wikimedia-hosted *PNG* of the London Underground roundel
(`upload.wikimedia.org/.../512px-Underground.svg.png`), so the repository
contained no SVG to preserve. Per the family rule, `assets/icon.svg` is
therefore a **new committed SVG** of the same roundel, and it is the
authoritative source from which `build.rs` derives the 192 and 512 install PNGs.

**Credit.** The file is the roundel published on Wikimedia Commons as
[`Underground.svg`](https://commons.wikimedia.org/wiki/File:Underground.svg),
fetched from
`https://upload.wikimedia.org/wikipedia/commons/4/41/Underground.svg`
(sha256 `556cda36a81f7a1176782a740596e169fec8bd7f6ab8a17985a1d6cc0dfa36bb`,
3786 bytes, an Inkscape export). It is in the **public domain** — a TfL
roundel, not a copyrightable drawing of one — and is used unmodified, which is
why it is committed byte for byte rather than redrawn. The colours are the
author's: the roundel red `#EE2622`, the bar blue `#263D96`, and white.

The wordmark in that SVG is drawn as **paths, not `<text>`**, which is what
makes it safe to rasterize with `usvg`/`resvg` built with
`default-features = false`: that configuration drops usvg's `text` feature, so
there is no font database, and a roundel whose lettering is geometry needs
none. The icon therefore renders identically on a machine with no fonts, and
identically in CI. The alternative — a `<text>` roundel — would have made the
install icons depend on which fonts happened to be installed on the build host.

The shell links the **SVG** directly (`<link rel="icon" href="./icon.svg">`) and
uses `icon-192.png` only for `apple-touch-icon` and the manifest, so the
committed source stays the icon the browser actually uses. The PNGs exist only
in `dist/`; `tests/shell.rs` fails if either appears in `assets/`.

## Offline: the one app in the family that cannot be

The other apps in this family are offline-capable because their data is local.
This one is not, and pretending otherwise would be a lie: a departures board
whose minutes are wrong is worse than a board that says it cannot reach the
network. So the trade is explicit.

**What the service worker does.** It caches the eight shell files, so with a
connection made once over HTTPS the page loads offline, the Rust board starts,
and it can tell you it has no connection. Its `fetch` handler is an
allowlist — `ASSETS.includes(event.request.url)` — and returns early for
everything else, so no request to `api.tfl.gov.uk` is ever answered from the
cache, ever stored, or even inspected. There is no `fetch` fallback that would
cache an API response as a side effect. Both the shell test and the CI site
check assert the API's absence from the worker, because a stray allowlist entry
would serve arrivals from a cache that is hours old while the board claims to be
live.

**Why not cache the arrivals.** TfL's arrivals feed is a few minutes deep and
its `timeToStation` values are relative to the moment of the request. Cached,
they are meaningless: "3 min" from an hour ago is not "3 min", it is a lie
with a countdown. An app that stored them would need to invalidate them, and
the honest form of that is "do not store them".

**What the user sees.** The shell's `#notice` element is a `role="status"`
region that carries the connectivity state and is rewritten on the `online` and
`offline` events, so the board says what it is before you wonder why the
minutes stopped moving. A failed departures request — no connection, a 500, a
rate limit, a body that is not JSON — produces the original's own wording,
"Error fetching departures. Please try again later.", in the departures area.
The station list failing is a separate message, because it is a different
failure with a different consequence: there is nothing to choose, so the picker
stays empty and the app says why.

CORS is not a problem: TfL sends permissive `Access-Control-Allow-Origin` on
this API, so a plain cross-origin `fetch` works with no proxy. It is still
handled as a failure, because it is one whenever the network is not there.

## Code map

- `departures.rs`: the whole board, with no browser in it. The stop-point
  filter (`940GZZLU`/`940GZZCR` plus a `commonName`), the alphabetical station
  order, the sort by `timeToStation`, the grouping by `platformName` (falling
  back to "Unknown Platform"), the platform ordering by the first run of digits
  in the name, `floor(timeToStation / 60)`, the ten-per-platform cut, and the
  line-colour table with its `#666` default. Pure, and the unit tests at the
  bottom of the file drive all of it with fixture data.
- `ui.rs`: wasm-only. The `fetch` to TfL, the DOM, the thirty-second refresh
  timer, the `?station=` parameter, the page title, the connectivity notice and
  the service-worker registration. Every question of *what* to show is answered
  by `departures.rs`.
- `ui.html`: the static shell, copied to `dist/index.html` byte for byte. One
  inline `<style>`, one `<script type="module">`, and empty containers Rust
  fills.
- `service-worker.js`: the shell cache and nothing else; see "Offline" above.
- `build.rs`: writes the six files it owns into `dist/`, rasterizes the icon,
  assembles the manifest, and derives the worker's content-derived cache
  version. It leaves `dist/app.js` and `dist/app_bg.wasm` to the `wasm-bindgen`
  step, so it writes files in place rather than replacing the directory.

## Fidelity notes

Porting JavaScript to Rust changes a few things by accident rather than by
choice. Each of these was decided deliberately and is pinned by a test:

- **`Math.floor` is not `/`.** Rust's `/` truncates *toward zero*, so
  `-30 / 60` is `0` where `Math.floor(-30 / 60)` is `-1`. TfL lists trains that
  have already gone with a negative `timeToStation`, and the original showed
  those as `-1`. `div_euclid(60)` is floor division and restores it. This is
  the single most likely silent behaviour change in any numeric port.
- **`localeCompare` is not `Ord`.** The station sort was
  `commonName.localeCompare(...)`, which the browser implements with the
  reader's locale — untestable and machine-dependent. It is a
  case-insensitive comparison with a code-point tie-break here, so the order is
  the same everywhere and the tests can pin it.
- **A truthy lookup is not a lookup.** The original wrote
  `lineColors[dep.lineName] || "#666"`, so a `lineName` of `""` — which TfL
  does send — took the grey through the `||`, not through being unknown. The
  Rust treats it as a line with no colour, which is what the original meant and
  the same colour it drew.
- **`Number` is not `i32`.** A platform name with an absurdly long digit run
  would overflow a parse and wrap negative, sending the table to the wrong end
  of the screen. The digits are read as `i64` and a value that does not fit
  sorts as 0.
- **`innerHTML` is not `set_text_content`.** The original built the board as an
  HTML string, so a destination called `<script>` would have been a script. The
  board is built from elements, and every value that reaches the page goes
  through `textContent`.

**Elizabeth line stations.** The original's filter is `940GZZLU` and
`940GZZCR`, kept exactly. The Elizabeth line's own stop points are `940GZZEL`,
so they are *not* in the board, even though the mode endpoint asks for
`elizabeth-line` and those stops come back. The list simply holds a mode the
prefix filter excludes — the original behaved this way too, and the Elizabeth
line's stations are mostly reached through the `940GZZCR`/`940GZZLU` ids for the
interchanges that carry it. Keeping the two prefixes unchanged is the
faithful port; widening them would be a redesign.

## User-visible text never names the implementation

No string a reader can see may contain "Rust", "WebAssembly", "wasm",
"JavaScript", "bindings" or "compile" — not in the shell's rendered text, and
not in a string `ui.rs` writes into the DOM. A reader told a failure happened
"in Rust" has been told something they cannot act on and did not ask for; what
they need is the app's name and what to do next. So the loader's message is
"London Rura could not start. Reload the page or check your connection."

The line is the **interface**, not the codebase. This file, `README.md` and
every source comment may and do say "Rust" in as much detail as they like;
`tests/shell.rs` deliberately does not read them. It reads the rendered
markup, the literals the loader writes, and the `const … : &str` messages in
`ui.rs`, and fails on any of them.

Two things are deliberately still allowed:

- **A fact about the service.** "Departures come live from the TfL API" tells
  the reader why a time might be stale, which is exactly what a board should
  say. It names no implementation.
- **The `<noscript>` notice**, which is the one place "JavaScript" survives.
  Its only reader is someone whose scripting is switched off, so "this needs
  JavaScript to run" is the true and actionable explanation of the failure the
  notice exists to describe: the scripting did not run. It names a browser
  setting the reader controls, and no application implementation. The test
  treats it as the single exception and bans the word everywhere else,
  including in `ui.rs`.

## Tests

`src/departures.rs` carries the unit tests, inline, and they are the valuable
half of this rewrite: the id filter and both of its halves, the alphabetical
station order and its tie-break, the sort by `timeToStation`, the platform
grouping and the numeric platform order, the floor-to-minutes arithmetic
including the negative case, the ten-per-platform cut, and every line colour
including the `#666` default. They run under plain `cargo test` with no
browser, no network and no `web-sys`, driven by fixture data. One of them
parses the JSON shapes TfL actually sends, because a fixture built in Rust
cannot catch a field renamed upstream.

`tests/shell.rs` asserts the invariants of the committed shell and of what is
committed: no user-visible string names the implementation (see above), the page
loads the generated bindings and not a manual wasm ABI,
there is exactly one `<script>`, no absolute URL and no `fetch(` in the shell,
the ids the board looks up exist, the worker has exactly one `__VERSION__` and
no skip-waiting call, the worker caches exactly the eight site files and does
not name the TfL API in code, `dist/` and `target/` are ignored, no generated
file is tracked, the install PNGs exist only in `dist/`, and the original
JavaScript PWA is gone.

There are no browser tests, no Node and no Chromium. The board's decisions are
pure and are covered above; everything that touches the DOM is Rust building
elements rather than a string being parsed, so a browser would be testing the
DOM library.

## Verification

```
cargo clippy --all-targets -- -D warnings
cargo clippy --lib --target wasm32-unknown-unknown -- -D warnings
cargo test --locked
```

`cargo build` writes `dist/`, and the release gate does not carry it (see
`tests/shell.rs`), so in a checkout the site has to be rebuilt by hand to be
looked at. `.github/workflows/build.yml` does exactly that on every push and
pull request, in the documented order, and then inspects what came out: eight
files, all non-empty, no strays, the service worker's `__VERSION__` already
substituted, bindings that still export what the page imports, a manifest that
kept the original name and colours — and, the check that is this app's alone,
that the service worker does not mention the TfL API in code. That check is
aimed at the one failure this app could have and the others cannot: a worker
that grew an allowlist entry for the API and quietly started serving
arrivals from a cache. The generator is the same pinned prebuilt as the rest of
the family, verified against the release's own checksum and against a digest
pinned in the workflow.

## Known limitations

- Nothing is cached but the shell, so a cold start with no connection shows an
  empty station picker and says why. The stations *are* held in memory once
  fetched, so a board already open keeps working as a board — it just cannot
  refresh, and the notice says so.
- The board shows what TfL's free API returns, which is a few minutes of
  arrivals and nothing further out. There is no timetable view.
- `lineName` values are matched exactly against the twelve published lines. A
  new line, or a renamed one, renders in the `#666` grey until the table in
  `departures.rs` learns it. That is the original's behaviour and is a
  one-line fix.
