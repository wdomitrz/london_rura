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

## The TfL API, measured

Everything here was measured against the live API on 2026-09-30, not read off
a document, because the documentation and the endpoint disagree and the
endpoint is what the app talks to. The next app to use this API should take
these numbers rather than re-deriving them.

| | |
|---|---|
| `StopPoint/Mode/tube,elizabeth-line` | **16,958,009 bytes** (17.0 MB), 1,858 stop points, ~2.5 s |
| of which are stations (`940GZZLU`/`940GZZCR` with a `commonName`) | **270** |
| `?detail=false` on the same URL | 16,957,998 bytes — a **0.0001%** saving. Accepted and ignored. |
| the 12 × `Line/<id>/StopPoints` | **3,255,303 bytes** total, 12 requests, **81% less** |
| `Line/Mode/tube` (line ids only) | 6,717 bytes |
| CORS on a real GET | `access-control-allow-origin: *` |
| CORS on a HEAD request | **absent** — do not use HEAD to check CORS here |

Three findings that each cost something:

**1. The payload is 95% two fields nobody reads.** `children` is 9.0 MB and
`additionalProperties` is 7.1 MB; together that is 16.1 MB of the 17.0. The
board reads two fields, `id` and `commonName`, which together are 70 KB.
There is no parameter that strips them, which is why the fix below changes
*which endpoint* is called rather than asking for less of this one.

**2. There is no smaller station-list endpoint.** `StopPoint/Search/{name}` is
343 bytes but is a name search, not a list. `StopPoint/{id}` is 37 KB for one
stop point. `Line/Mode/<mode>` returns 6.7 KB of line metadata, and
`Line/<id>/StopPoints` returns one line's stations as a bare array. The last
one is the answer: fetching all twelve lines gives **exactly the same 270
stations** — verified by diffing the two sets, identical, no station missing
and none extra — for 81% fewer bytes, with a maximum single response of one
line rather than the whole network. Stations served by three lines appear three
times and are deduplicated.

**3. The stop-point list sends `id`, not `stopPointId`.** The older TfL Unified
API documentation shows `stopPointId`; the live endpoint does not send it, and
of 1,858 stop points, **zero** carry it. Deserialising that name leaves every
id as the empty string, every id fails the prefix test, and the picker is
permanently empty **with no error reported anywhere** — which is exactly what
"stuck on Loading stations…" looked like. The arrivals list disagrees in the
other direction and has no id at all. `the_wire_formats_still_parse` and
`a_real_stop_point_response_yields_stations` now pin this against a fixture
taken from a real response.

### Why the fix is per-line, and parse from bytes

`response.json()` resolves the whole body to a live JavaScript object, and
deserialising *that* walks the object graph property by property across the
wasm boundary, on the UI thread. On 17 MB that is long enough to be seen: the
page stops responding, which is indistinguishable from a network that never
answers. The app now uses `array_buffer()` and `serde_json::from_slice`, which
parses once in Rust and never builds the JavaScript object.

Both changes are needed, and neither is sufficient alone. Bytes-only still
parses 17 MB; per-line only still builds a JS object out of 3 MB. Together the
largest thing the UI thread ever deserialises is one line's stop points.

## The colour scheme, and why

The original was Arial on white with grey borders: legible, and not a board.
The scheme now is a platform board's — a near-black face, greyscale text, and
one warm accent — chosen against measured contrast rather than by eye.

- **The minutes are the only bright thing.** They are what the reader came
  for, so they get the only saturated colour on the page: `#FFD300`, Circle's
  yellow and the colour a real board lights its times in. Everything else is
  greyscale, which leaves the line chips as the only other colour present.
- **Zero minutes is marked, not left blank.** A train at the platform is not
  "no time", it is the thing the reader is waiting for, so it gets the red
  `#FF453A` and a `due` class.
- **Rows are separated by a hairline, not boxed.** A split-flap board has no
  grid; the tables read as a list of departures rather than a spreadsheet.

**The interesting problem is the line colours, and it has no solution in
CSS.** The published line colours are the one piece of visual information a
reader actually uses, and they are not all legible on any single background.
Circle's `#FFD300` is 1.44:1 on white; Victoria's `#00A0E2` is 2.95:1; the
three pale lines are under 2:1. On a dark page, District drops to 3.49:1 and
Metropolitan to 2.37:1. **No background works for all twelve**, so the
original's black-on-white table left three lines unreadable.

So each line's name is drawn on a **chip of its own colour**, in whichever of
black or white is legible on it — and that choice is *computed*, not made by
hand, in `line_ink`, from the WCAG relative-luminance formula in
`departures.rs`. All twelve now clear **4.5:1** (worst: Bakerloo at 4.70:1,
best: Northern at 21:1), and `every_line_is_readable_on_its_own_chip` fails if
a new line's colour cannot. The colour is still the first thing the eye goes
to; the text is what survives when the colour cannot be seen, which also makes
the board readable for a colour-blind reader or on a dimmed screen.

**One thing contrast on the chip does not solve: finding the chip at all.**
Northern's `#000000` is 1.07:1 against the dark page and Circle's `#FFD300` is
1.34:1 against the light one — in each scheme exactly one line's chip is an
invisible rectangle. No fill can fix that without falsifying the published
colour, so every chip gets a hairline ring in the page's ink colour. The ring
is decoration, not the accessibility mechanism: the line's name is on the chip
either way. This was caught by looking at a rendered board, not by a test, and
`the_chip_edge_is_a_decision_the_numbers_record` now pins the numbers behind it.

Both schemes are `prefers-color-scheme` driven, dark first, and the accent is
darkened to `#8a6d00` in the light scheme so it keeps its contrast against a
white page.

## The bug that four green builds did not catch

**Read this before changing anything about how the app starts.** This app
shipped four times with a green build, a green test run, a green CI job, all
eight files present, and a page that did nothing. The user reported it as
"stuck on Loading stations…", which was exactly right, and the cause was not
the one that diagnosis suggested.

### Why every check passed

Because **nothing was running, so nothing could fail.**

The entry point was marked `#[wasm_bindgen]`. That declares an ordinary
*export*: `wasm-bindgen` puts the symbol in the wasm binary, and the generated
glue does **not** re-export it. The glue's entire export list is:

```js
export { initSync, __wbg_init as default }
```

The shell's loader calls `m.default()`, which is the module's **initializer** —
it instantiates the wasm and returns. The board's `start` is nowhere in that
list and nothing else calls it. The page therefore:

- loaded `index.html`, `app.js` and `app_bg.wasm` — all HTTP 200,
- ran the loader, successfully, with no error,
- drew its static shell, including the "Loading stations…" notice,
- and then sat there forever, because the app's first line never executed.

There is no error to log, no failed assertion, no missing file. A build that
cannot fail cannot be caught by a test that checks the build.

### The fix

```rust
#[wasm_bindgen(start)]   // not #[wasm_bindgen]
```

That attribute marks the function as the module's **start function**, which
`wasm-bindgen` emits into the wasm's start section and the generated glue calls
during initialisation — `wasm.__wbindgen_start();`, visible in `dist/app.js`.
The reference implementation, `lego_mosaic`, uses this and only this; it is
worth reading its `browser.rs` before writing another entry point.

A start function is synchronous and cannot be awaited, so it does its wiring
and hands the real work to `spawn_local`.

### What now catches it

- `the_board_declares_a_start_function` in `tests/shell.rs` asserts the
  attribute is in the source, and that the start function hands async work to
  `spawn_local` rather than trying to run it inline.
- `the_loader_only_asks_for_the_initializer` asserts the loader does not reach
  for an export that does not exist.
- The CI site check asserts `dist/app.js` contains `__wbindgen_start` — the
  property of the *built* artefact that no source-level test can see. This is
  the one that matters: it checks what actually ships.

All three are mutation-tested.

### The third bug, found the same way

With the app finally running, the picker held **383 options for 270 stations**,
and Acton Town appeared three times. The per-line fetch returns a station once
per line that serves it, and the big interchanges are on three or four lines, so
concatenating twelve responses repeats them.

`station_list` now deduplicates by id, first sighting wins. Two things are
worth recording about how it got there:

- **It was documented and tested, and never implemented.** `LINE_IDS` said
  "a station on three lines appears in three responses and is deduplicated
  here", and a test asserted no duplicates — but the assertion was over a
  single response, which cannot contain one. So the comment and the test both
  *looked* like coverage and covered nothing. The test now concatenates two
  real per-line responses that genuinely share Acton Town and Alperton, so it
  fails without the dedupe.
- **A unit test fed from one response can never catch this.** It is a property
  of the fetch *strategy*, not of any one payload. Reading the options out of
  the rendered page is what found it, three bugs running.

### The second bug, hiding behind the first

Once the app started, it **froze the renderer**. `load_stations` spawned twelve
fetches with `spawn_local`, then polled a shared cell until every slot was
filled, yielding between polls with a resolved `Promise` awaited from Rust.

A microtask is drained *before* the browser returns to its event loop, so
awaiting one does not let a pending `fetch` make progress. The polling loop
starved the very tasks it was waiting for, forever — a hard hang, again with no
error, and again invisible to every check that only inspects the build.

It is now `join_all` over boxed futures: each turn polls every future and only
yields when none is ready, and the yield is a real macrotask
(`setTimeout(…, 0)`), which does hand control back to the event loop. That is
also genuinely concurrent — the naive `for f in futures { f.await }` version
would have compiled, passed every test, and serialised twelve requests into
twelve round trips, which is the one thing the per-line fetch exists to avoid.

**The lesson, and it generalises to every app in this family:** a board that
renders a static shell and then stops is indistinguishable, to every automated
check, from a board that works. Only loading the page and looking at it tells
you. `tests/shell.rs` asserts the *properties* that made this possible; it
cannot assert that the app renders, and nothing in the repository can.

## Verifying by hand, in a real browser

`cargo test` and CI do not catch any of the above. This is the loop that does,
and it needs no browser automation:

```bash
cd dist && setsid python3 -m http.server 8099 &      # serve the built site
chromium --headless --disable-gpu --no-sandbox          --virtual-time-budget=40000 --dump-dom http://127.0.0.1:8099/index.html
```

Then look at the DOM: the notice must have changed from "Loading stations…",
and `<select id="station">` must hold more than one `<option>`. Two traps, both
of which cost time here:

- **`--virtual-time-budget` is a budget, not a timeout.** The app fetches from
  the live network, so give it 30–60 s, and expect a rate limit after a handful
  of runs. A rate limit shows the station-list failure notice, which is the app
  behaving correctly — wait a minute and retry before believing it.
- **`--dump-dom` is intermittently empty.** If it returns nothing, retry before
  concluding the page is broken; the same command works on a re-run.

A page that hangs the renderer produces no DOM at all, which is a distinct
failure from a page that renders and shows an error — worth telling apart.

Two things that page view is the only way to get:

- **A count that is wrong but not obviously so.** "383 options" looks like a
  station list. It is 270 stations with 113 repeats, and no assertion anywhere
  would ever have said so.
- **Whether a feature works at all**, which is the first two bugs in this
  section.

## Code map

- `departures.rs`: the whole board, with no browser in it. The stop-point
  filter (`940GZZLU`/`940GZZCR` plus a `commonName`), the alphabetical station
  order, the sort by `timeToStation`, the grouping by `platformName` (falling
  back to "Unknown Platform"), the platform ordering by the first run of digits
  in the name, `floor(timeToStation / 60)`, the ten-per-platform cut, and the
  line-colour table with its `#666` default. Pure, and the unit tests at the
  bottom of the file drive all of it with fixture data.
- `ui.rs`: wasm-only. The fetches to TfL — one per line, joined concurrently,
  parsed from bytes — the DOM, the thirty-second refresh timer, the
  `?station=` parameter, the page title, the connectivity notice and the
  service-worker registration. Every question of *what* to show is answered by
  `departures.rs`. The entry point is `#[wasm_bindgen(start)]`; see above for
  why that is not interchangeable with `#[wasm_bindgen]`.
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
- **A deserialised field that is not there is not an error.** `stopPointId`
  against a payload that sends `id` deserialises to an empty string for every
  record, silently, and the filter then drops everything. `#[serde(default)]` on
  a field that should always be present is what turns a loud parse failure into
  a silent empty board. The fixtures for the wire formats are now transcribed
  from real responses, because a fixture written to match the code cannot catch
  this.
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

Two tests carry real weight beyond coverage. `a_real_stop_point_response_
yields_stations` parses a fixture cut from an actual API response and fails if
the picker comes out empty — the check that would have caught the `stopPointId`
bug, and it reproduces the reported symptom exactly (`left: []`). And
`every_line_is_readable_on_its_own_chip` fails if any line's colour cannot
reach 4.5:1 against its own ink, which makes the colour scheme a property of
the code rather than a matter of taste.

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

## The CI check step, and a trap in it

The first run of this workflow on `master` was **red with every one of the
eight files reported `ok`**. The build was fine; the check could not run:

```python3 -c '
    import json, sys
    ...
'
```

`python3 -c` hands the string to the interpreter exactly as written, so the
indentation of a multi-line body is an `IndentationError` before a single
statement executes. The sibling failure mode is worse: an apostrophe anywhere
in the body terminates the shell quote, and bash never reaches Python at all.
Both forms look correct in review and fail only on a runner.

The fix is the heredoc, `python3 - <<'PY'`, with the body at the same
indentation as the `python3` line — the YAML block scalar dedents the whole
step, so the terminator lands in column 0.

`tests/shell.rs` now asserts there is no inline `python3` flag in the workflow
(that is always a mistake, never a style choice), that every heredoc is
terminated, and that the seven build steps are present under their exact
names. Exact names matter: a substring check for `Lint` is satisfied by a step
renamed `Linting`, which is the quiet drift that leaves a job green and
unverified. Those assertions are mutation-tested, so they are known to fail
when the workflow is broken.

To rehearse a workflow change without waiting for a runner, extract the
`run:` scripts and execute them under `bash -euo pipefail`, which is what
GitHub does.

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
- The station list costs twelve requests instead of one, issued concurrently.
  They are cached by TfL, so this is cheap, but it is not free, and a reader on a
  slow connection waits for the slowest line rather than the fastest. A single
  line failing is not fatal — the other eleven still load and the notice says
  so — but that line's stations are simply missing until the page is reloaded.
- `lineName` values are matched exactly against the twelve published lines. A
  new line, or a renamed one, renders in the `#666` grey until the table in
  `departures.rs` learns it. That is the original's behaviour and is a
  one-line fix.
