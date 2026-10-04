// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! London Rura: a London Underground departures board, in Rust.
//!
//! The app is a browser application and only a browser application: it is a
//! single static page whose station picker, departures tables, line colours,
//! page title and query parameter are all built and owned by Rust, compiled to
//! WebAssembly. `cargo build` writes the whole publishable site into `./dist`.
//! There is no server, no `serve` subcommand and no runtime dependency on
//! anything this crate ships as a binary — there is no binary.
//!
//! The split is deliberate and is what makes the app testable:
//!
//! * [`departures`] is the domain — which stop points are stations, how they
//!   sort, how arrivals group into platforms, the minutes, the line colours.
//!   It is pure, has no `web-sys` in it, and is what the unit tests drive.
//! * `ui` is the browser — the `fetch` to TfL, the DOM, the timer, the
//!   query parameter, the service-worker registration. Named as plain text, not
//!   a doc link: the module exists only on wasm, and the rustdoc the gate builds
//!   runs on the host.
//!
//! So the original `app.js` is now `departures.rs` for its decisions and
//! `ui.rs` for its side effects, and neither can quietly change the other's
//! behaviour.

pub mod departures;
pub mod modes;

#[cfg(target_arch = "wasm32")]
mod ui;
