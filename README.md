# London Rura

A London Underground departures board for every station on the TfL network.
Pick a station and it lists the next trains from each platform, colour-coded by
line, refreshed every thirty seconds from the live TfL API.

The whole board is a browser app written in Rust and compiled to WebAssembly.
There is no server, no account, and nothing stored: the only network traffic is
the departures request, to `api.tfl.gov.uk`, on the station you chose.

## Build

Two steps, because there are two targets. Nothing generated is committed.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked

# 1. the board: compile to wasm and generate the JS bindings
cargo build --locked --lib --target wasm32-unknown-unknown --release
wasm-bindgen --target web --no-typescript --out-dir dist --out-name app \
  target/wasm32-unknown-unknown/release/london_rura.wasm

# 2. the rest of the site
cargo build --release --locked
```

That gives you a publishable `dist/` of exactly eight files. **The order
matters**: step 2 derives the service worker's cache name from the bytes of
everything else in `dist/`, including the wasm, so running it first would pin
that to whatever the previous build left behind.

The `touch build.rs` in the documented build (see `AGENTS.md`) is not
redundant: `build.rs` writes into the source tree rather than `OUT_DIR`, so
cargo cannot see that anything changed and will not re-run it for a second,
otherwise identical invocation — leaving `dist/` with the two wasm artefacts
and none of the six shell files.

`cargo test --locked` needs none of the above — it runs without the wasm target
or the bindings generator, and needs no network.

## Publishing

`dist/` is a static site and mounts anywhere: every URL in it is relative and
`start_url` is `"./"`. Any file host will do — nginx, Caddy, GitHub Pages,
`python3 -m http.server`.

## Stations

The station list comes from twelve small requests — one per line — rather than
the single 17 MB request the original made. The same 270 stations, 81% fewer
bytes, and the largest thing the browser has to parse is one line instead of
the whole network. Stations on several lines arrive more than once and are
deduplicated. The measurements are in `AGENTS.md`.

## Checking it actually works

`cargo test` and CI do not prove this page runs — see the two bugs recorded in
`AGENTS.md`, both of which passed every automated check. To see the board for
real:

```bash
cd dist && setsid python3 -m http.server 8099 &
chromium --headless --disable-gpu --no-sandbox \
         --virtual-time-budget=40000 --dump-dom http://127.0.0.1:8099/index.html
```

The notice must leave "Loading stations…" and the station list must fill.

## Offline

This is the one app in the family that is **not** fully offline, because its
data is a live feed and a stale copy would be worse than none. The service
worker caches the eight shell files and nothing else; it never touches
`api.tfl.gov.uk`. With a connection the board refreshes itself; without one, the
page still loads from the cache and says plainly that it cannot reach TfL.
See `AGENTS.md` for why that is the right trade.

## Icon

`assets/icon.svg` is the Underground roundel, fetched from Wikimedia Commons
(public domain). It is the committed, authoritative icon; the 192 and 512
install PNGs are rasterized from it at build time and are never committed.
Full credit in `AGENTS.md`.

AGPL-3.0-only. See `LICENSE`.
