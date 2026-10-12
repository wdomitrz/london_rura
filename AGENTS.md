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

A departures board whose minutes are wrong is worse than a board that says it
cannot reach the network, so the trade is explicit: the worker caches the eight
shell files and nothing else, and never touches `api.tfl.gov.uk`. The full why
is in `src/service-worker.js`, and `tests/shell.rs` enforces it; the
connectivity notice and the failure wordings live in `src/ui.rs`. CORS is not a
problem — TfL sends `access-control-allow-origin: *` — but it is still handled
as a failure, because it is one whenever the network is not there.

## Modes, and the Elizabeth line that was missing

The board serves eight modes, switchable from the header and carried in the URL
(`?modes=dlr,bus`), so a filtered board is a link a reader can send. Each has a
name, a colour and a glyph in `src/modes.rs`, whose module documentation also
tells the story of the prefix filter that hid the Elizabeth line, the
Overground, the DLR and the cable car — and why
`every_mode_maps_to_a_known_api_name` is the test that keeps that from coming
back.

### One station, one id per mode

An interchange is one row holding an id per mode, merged by name — the
Paddington story, and why bus stops keep their own id instead, is told on
`station_index` in `src/departures.rs`.

### Buses are reached by search, not by download

`StopPoint/Mode/bus` is an HTTP 400 and there are ~19,000 bus stops, so the
buses toggle turns on search instead; the measurements and the mechanics
(merging results into the list, the 250 ms debounce) live on `SEARCH_URL` and
`spawn_search` in `src/ui.rs`.

## Three bugs this rewrite found by looking at the page

All three were invisible to the test suite, and the first two cost a real user
their app; each is written up beside the code that fixed it:

1. **Clicking a suggestion chose the top one** — `change` fires on blur, which
   a click on a suggestion causes; it is Enter-only now. See `on_keydown` in
   `src/ui.rs`.
2. **Search found nothing at all, ever** — `StopPoint/Search` sends `id`, not
   the documented `stopPointId`. See `SearchMatch` in `src/departures.rs`.
3. **Fuzzy search, and what "fuzzy" does not mean** — ranked four ways, the
   lowest a subsequence. See `score` in `src/departures.rs`. One stated limit:
   a **transposition** does not match — the characters are the right ones in
   the wrong order — because edit-distance scoring over ~2,700 stations on
   every keystroke is the wrong trade for a field that has to feel instant.

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

**3. `StopPoint/Mode/{mode}` returns a wrapper, not an array** — deserialising
it as a `Vec<StopPoint>` fails to parse and the board reports it could not load
any stations, with every HTTP status at 200. `StopPointList` and its test in
`src/departures.rs` exist so that cannot recur quietly.

**4. Only `940GZZ…` and `910G…` ids return arrivals.** Among 1,751 named tube
stop points the prefixes are `9400ZZLU` (918), `4900ZZLU` (479), `940GZZLU` (270),
`2100ZZLU` (7), `0400ZZLU` (4). Measured one of each against `/Arrivals`:

| prefix | example | arrivals |
|---|---|---|
| `940GZZLU` | `940GZZLUACT` | **live** |
| `9400ZZLU` | `9400ZZLUACT` | 0 |
| `4900ZZLU` | `4900ZZLUACT1` | 0 |
| `2100ZZLU` | `2100ZZLUCXY0` | 0 |
| `0400ZZLU` | `0400ZZLUAMS0` | 0 |

Amersham appears twice in one response: `940GZZLUAMS` returns live Metropolitan
trains, `0400ZZLUAMS0` returns none. So an id being present in a stop-point list
is **no evidence it is worth requesting** — `is_requestable` is the predicate
that stands between a station appearing in the picker and it having a board.

**5. `StopPoint/Search` also sends `id`, not `stopPointId`** — and is a *literal
phrase matcher*, not a fuzzy one. Measured, all HTTP 200:

| query | matches |
|---|---|
| `Stratford International Rail` | 1 — `910GSTFODOM`, national-rail |
| `Stratford International` | 1 — the DLR stop |
| `Stratford Int Rail` | 0 |
| `Stratford` | 20, including `910GSTFODOM` |

So the local search is fuzzy and the network search is not, and the gap is
bridged by falling back to the **longest word** of the query: for "stratford int"
that is "stratford", which finds the rail stop. Falling back to "int" instead
would find Ashford, Braintree and Fintringham. `fallback_queries` encodes that
ordering and is tested on the host.

**6. National rail has no departure data at all.** Every national-rail stop point
answers `/Arrivals` with an empty array. Measured at Stratford International:
the parent `910GSTFODOM`, the child `9100STFODOM`, the numbered `9100STFODOM0`
and the bus-side `4900STFODOM1` all return `[]`. `StopPoint/Mode/national-rail`
is a **504** and `StopPoint/Mode/national-rail,tube` is a **500**.

So the mode exists, the stations exist, and choosing one says so plainly rather
than showing a blank board. That is better than leaving Stratford International
out of the picker: a reader asking about it now learns something true about this
service instead of concluding it does not exist.

**7. The stop-point list sends `id`, not `stopPointId`.** The older TfL Unified
API documentation shows `stopPointId`; the live endpoint does not send it. The
full measured story, and the fixtures that pin it
(`the_wire_formats_still_parse`, `a_real_stop_point_response_yields_stations`),
are on `StopPoint` in `src/departures.rs`.

### Why the fix is per-line, and parse from bytes

Both changes are needed and neither suffices alone — bytes-only still parses
17 MB, per-line only still builds a JavaScript object out of 3 MB — so the
largest thing the UI thread ever deserialises is one line's stop points. The
full why is on `fetch_bytes` in `src/ui.rs`.

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
eight files present, and a page that did nothing: the entry point was marked
`#[wasm_bindgen]` — an ordinary *export*, which the generated glue never
re-exports — so nothing was running and nothing that only inspects the build
could fail. The full autopsy is on `start` in `src/ui.rs`; the fix is
`#[wasm_bindgen(start)]`, and what now catches it is
`the_board_declares_a_start_function` and
`the_loader_only_asks_for_the_initializer` in `tests/shell.rs`, plus the CI
check that `dist/app.js` contains `__wbindgen_start` — the property of the
*built* artefact that no source-level test can see.

### The third bug, found the same way

With the app finally running, the picker held **383 options for 270 stations**.
The per-line fetch returns a station once per line that serves it, and
`station_index` now deduplicates by id, first sighting wins. Two things are
worth recording about how it got there:

- **It was documented and tested, and never implemented.** A comment said a
  shared station "is deduplicated here" and a test asserted no duplicates — but
  over a single response, which cannot contain one. The comment and the test
  both *looked* like coverage and covered nothing. The test now concatenates
  two real per-line responses that genuinely share Acton Town and Alperton, so
  it fails without the dedupe.
- **A unit test fed from one response can never catch this.** It is a property
  of the fetch *strategy*, not of any one payload. Reading the options out of
  the rendered page is what found it, three bugs running.

### The second bug, hiding behind the first

Once the app started, it **froze the renderer**: a polling loop awaited a
resolved `Promise` between polls, and a microtask is drained before the browser
returns to its event loop, so the fetches it waited for never progressed. It is
now `join_all` over boxed futures with a real macrotask yield; the autopsy is
on `join_all` and `yield_to_browser` in `src/ui.rs`.

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

- `modes.rs`: the eight modes — name, short name, glyph, TfL colour — and the
  `ModeSet` bitmask behind the switches. Pure, and the unit tests pin that every
  mode round-trips through `from_api_name` and is readable on its own chip.
- `departures.rs`: the whole board, with no browser in it. `is_station` (a name is
  all it takes), `is_requestable` (does the arrivals endpoint answer for this id),
  `station_index` (merge per-mode responses by name into one row per place with an
  id per mode), `Search` (ranked station matching), the sort by `timeToStation`,
  the grouping by `platformName` (falling back to "Unknown Platform"), the
  platform ordering by the first run of digits in the name, `floor(timeToStation
  / 60)`, the ten-per-platform cut, and the line-colour table with its `#666`
  default. Pure, and the unit tests at the bottom of the file drive all of it
  with fixture data taken from real responses.
- `ui.rs`: wasm-only. The fetches to TfL — one per line, joined concurrently,
  parsed from bytes — the DOM, the thirty-second refresh timer, the
  `?station=` parameter, the page title, the connectivity notice and the
  service-worker registration — with its scope stated, and the release of any
  stale wider one. Every question of *what* to show is answered by
  `departures.rs`. The entry point is `#[wasm_bindgen(start)]`; see above for
  why that is not interchangeable with `#[wasm_bindgen]`.
- `ui.html`: the static shell, copied to `dist/index.html` byte for byte. One
  inline `<style>`, one `<script type="module">`, and empty containers Rust
  fills: the toggle row, the search field, the suggestion list, the station
  heading, the freshness stamp and the board. It is dense on purpose — 22px rows,
  one column header for the whole board, and the minutes as the only bright
  thing on the page.
- `service-worker.js`: the shell cache and nothing else; see "Offline" above.
- `build.rs`: writes the six files it owns into `dist/`, rasterizes the icon,
  assembles the manifest, and derives the worker's content-derived cache
  version. It leaves `dist/app.js` and `dist/app_bg.wasm` to the `wasm-bindgen`
  step, so it writes files in place rather than replacing the directory.

## Fidelity notes

Porting JavaScript to Rust changes a few things by accident rather than by
choice. Each of these was decided deliberately, is pinned by a test, and is
told beside the code that embodies it:

- **`Math.floor` is not `/`** — `div_euclid(60)` in `src/departures.rs`. Rust's
  `/` truncates toward zero, so a departed train would read "0 min" where the
  original read "-1". This is the single most likely silent behaviour change in
  any numeric port.
- **`localeCompare` is not `Ord`** — the station sort on `station_index`.
- **A truthy lookup is not a lookup** — `line_colour`, where a `lineName` of
  `""` reaches the grey through being unknown, not through `||`.
- **`Number` is not `i32`** — `first_number`, which reads platform digits as
  `i64` so an absurd digit run sorts as nothing rather than wrapping negative.
- **A deserialised field that is not there is not an error** —
  `#[serde(default)]` turns a loud parse failure into a silent empty board, so
  the wire-format fixtures are transcribed from real responses: a fixture
  written to match the code cannot catch this.
- **`innerHTML` is not `set_text_content`** — the board is built from elements.

The original's `940GZZLU`/`940GZZCR` prefix-filter story — and what the rewrite
now does with the Elizabeth line — is on the module documentation of
`src/modes.rs` and on `station_index` in `src/departures.rs`.

## User-visible text never names the implementation

No string a reader can see may contain "Rust", "WebAssembly", "wasm",
"JavaScript", "bindings" or "compile": a reader told a failure happened "in
Rust" has been told something they cannot act on and did not ask for. Two
things are deliberately allowed — a fact about the service, and the
`<noscript>` notice, whose only reader needs the word "JavaScript" — and the
whole enforcement, exceptions included, is specified by `tests/shell.rs`. The
line is the **interface**, not the codebase: this file, `README.md` and every
source comment may say "Rust" in as much detail as they like.

## Tests

`src/departures.rs` carries its unit tests inline, at the bottom of the file,
driven by fixtures transcribed from real API responses; they are the valuable
half of this rewrite. `tests/shell.rs` is the executable specification of the
committed shell's invariants; read it.

There are no browser tests, no Node and no Chromium. The board's decisions are
pure and are covered above; everything that touches the DOM is Rust building
elements rather than a string being parsed, so a browser would be testing the
DOM library.

## The CI check step, and a trap in it

The check step is a heredoc, `python3 - <<'PY'`, never `python3 -c`: an
indented multi-line body is an `IndentationError` before a statement runs, and
an apostrophe anywhere in the body ends the shell quote instead. The full trap
is written up on the step in `.github/workflows/build.yml`, and
`tests/shell.rs` asserts it stays fixed — no inline `python3` flag, every
heredoc terminated, the seven build steps present under their exact names. To
rehearse a workflow change without waiting for a runner, extract the `run:`
scripts and execute them under `bash -euo pipefail`, which is what GitHub does.

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
