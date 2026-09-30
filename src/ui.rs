// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! The browser: the `fetch` to TfL, the DOM, the timer, the query parameter.
//!
//! This is the original `app.js` with the decisions moved out to
//! [`crate::departures`]. What is left here is exactly the part that cannot be
//! pure — asking TfL for data, putting the answer on screen, and keeping the
//! URL and the page title in step with the picker. Every question of *what to
//! show* is answered by the domain module, so the two cannot drift.
//!
//! # Offline
//!
//! This is the one app in the family that cannot work offline, because its data
//! comes from a live API on every refresh and is worthless stale. So nothing
//! here caches, retries on a timer forever, or pretends: the service worker
//! caches the *shell* only, and every request to TfL goes to the network. When
//! there is no network the app says so, in the departures area, in the same
//! plain words the original used for a failed fetch. The station picker keeps
//! working if the shell is cached, because the stations are fetched once and
//! held in memory — but they are not persisted, so a cold start while offline
//! has an empty picker and says why.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    Document, Element, HtmlOptionElement, HtmlSelectElement, Location, Response, UrlSearchParams,
    Window,
};

use crate::departures::{
    board, line_colour, line_ink, station_list, Arrival, Board, Station, StopPoint,
    ARRIVALS_BASE, LINE_IDS, LINE_STOPS_URL,
};

/// The message shown when no station is chosen.
///
/// The original's first branch, unchanged: an empty picker is not an error, it
/// is the state the app opens in.
const NO_STATION: &str = "Please select a station.";

/// Shown while a request is in flight, so a slow network reads as slow rather
/// than as broken.
const LOADING: &str = "Loading departures...";

/// Shown when TfL has nothing to say about a station.
const EMPTY: &str = "No upcoming departures found.";

/// Shown when a request fails, which is also what a device with no connection
/// sees. Deliberately one message for both: the reader cannot act on the
/// difference, and the original's was the same.
const FAILED: &str = "Error fetching departures. Please try again later.";

/// Shown when the station list itself cannot be loaded.
///
/// Separate from [`FAILED`] because it is a different failure with a different
/// consequence: there are no stations to choose, so the picker stays empty and
/// the app cannot do anything at all until the list arrives.
const STATIONS_FAILED: &str = "Could not load stations. Please try again later.";

/// Shown once the stations are in the picker and the board is ready.
///
/// This string replaces a dead end: the shell shipped with "Loading
/// stations…" in the notice and **nothing on the success path ever wrote to it
/// again**, so a board that had in fact finished loading still looked like it
/// was still loading. A status line that is never cleared is worse than no
/// status line, and it is why "stuck on Loading" had two possible causes here
/// rather than one.
const STATIONS_READY: &str = "Choose a station. Departures come live from the TfL API.";

/// Shown when some lines loaded and some did not, naming the service rather
/// than the implementation.
const PARTIAL: &str = "Some lines could not be reached, so the list may be short. Try again shortly.";

/// Shown when a station id in the URL is not one this station list has.
///
/// The original set the picker to a value that was not in it, which leaves the
/// select showing the first entry while the URL claims another — a small lie
/// about where you are. This says so instead.
const UNKNOWN_STATION: &str = "That station is not in the list. Choose one below.";

/// The query parameter the station id lives in.
const STATION_PARAM: &str = "station";

/// How often the board refreshes itself, in milliseconds.
///
/// The original's 30 seconds, unchanged. TfL's arrivals feed updates every
/// thirty seconds anyway, so a faster poll fetches the same numbers more often.
const REFRESH_MS: i32 = 30_000;

/// The scope this app's worker is registered for.
///
/// Stated rather than inherited. Left to itself, a registration's scope is the
/// directory of the page that registered it — which is right today and silently
/// wrong the moment the board is published somewhere else, or opened through a
/// path that resolves higher up the origin. A worker registered for the whole
/// origin does not serve just this board; it answers for every page on that
/// origin, including the ones that have nothing to do with it. Naming the scope
/// keeps that claim as small as the app.
///
/// It is the one string that has to agree with `src/service-worker.js`, which
/// resolves its own directory the same way, from `self.location`.
const SCOPE: &str = "./";

/// The application's mutable state, shared by the event handlers and the timer.
struct App {
    window: Window,
    document: Document,
    /// The picker. Its options are built once and never rebuilt.
    select: HtmlSelectElement,
    /// Where the board is rendered.
    departures: Element,
    /// The stations, kept because the timer needs to re-read the selection
    /// without a round trip, and the title needs their names.
    stations: RefCell<Vec<Station>>,
    /// The id of the station on the board, or `None` when the picker is empty.
    selected: RefCell<Option<String>>,
    /// The refresh timer. `None` while no station is chosen, because there is
    /// nothing to refresh.
    timer: RefCell<Option<i32>>,
    /// When the last successful fetch landed, as `Performance::now()`. `None`
    /// until the first one, and left at the old value when a refresh fails, so
    /// the stamp keeps counting up and the staleness becomes visible.
    fetched: RefCell<Option<f64>>,
}

impl App {
    /// Stop the refresh timer, if one is running.
    ///
    /// The original cleared the interval whenever the selection changed, which
    /// is also what stops a poll for the station you just navigated away from.
    fn stop_timer(&self) {
        if let Some(handle) = self.timer.borrow_mut().take() {
            self.window.clear_interval_with_handle(handle);
        }
    }

    /// Start the refresh timer for the station currently on the board.
    ///
    /// Replaces any running timer rather than adding a second one: the
    /// original cleared before setting for the same reason, and two timers
    /// would double every request and race each other's renders.
    ///
    /// This closure captures the app *strongly* and is deliberately leaked: the
    /// timer is the app's own heartbeat, and it must keep working for as long
    /// as the page does. The document already holds the `change` and
    /// connectivity listeners alive, so the app lives for the page regardless;
    /// the only thing freed on unload is the page itself.
    fn start_timer(self: &Shared) {
        self.stop_timer();
        let Some(station) = self.selected.borrow().clone() else {
            return;
        };
        let app = Rc::clone(self);
        let callback = Closure::<dyn FnMut()>::new(move || {
            let app = Rc::clone(&app);
            let station = station.clone();
            spawn_local(async move {
                app.load(&station).await;
            });
        });
        let handle = self
            .window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                REFRESH_MS,
            )
            .expect("the refresh interval");
        *self.timer.borrow_mut() = Some(handle);
        callback.forget();
    }

    /// Refresh the board for the station on it, and repaint.
    async fn load(self: &Shared, station: &str) {
        say(&self.departures, LOADING);
        let url = format!("{ARRIVALS_BASE}{station}/Arrivals");
        let arrivals: Vec<TflArrival> = match fetch_bytes(&url).await {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(arrivals) => arrivals,
                Err(error) => {
                    web_sys::console::error_1(&JsValue::from_str(&error.to_string()));
                    say(&self.departures, FAILED);
                    return;
                }
            },
            Err(error) => {
                // A failed fetch is also what a device with no connection sees.
                // There is nothing cached to show instead, so this message is the
                // whole of the offline behaviour: readable, brief, and the same
                // whether the network is down or TfL is unwell.
                web_sys::console::error_1(&JsValue::from_str(&error));
                say(&self.departures, FAILED);
                return;
            }
        };
        *self.fetched.borrow_mut() = self.window.performance().map(|p| p.now());
        paint(self, &board(arrivals.into_iter().map(Arrival::from).collect()));
    }
}

/// The application, as one shared `Rc` — every handler and the timer share it.
type Shared = Rc<App>;

/// What one line's fetch produced: its raw bytes, or why it has none.
type LineResult = Result<Vec<u8>, String>;

/// One line's fetch, boxed and pinned so the futures can be polled by hand.
type LineRequest = Pin<Box<dyn Future<Output = (&'static str, LineResult)>>>;

/// Install the app: build the picker's options, wire the change handler, read
/// the URL, and start fetching.
///
/// **`#[wasm_bindgen(start)]`, and that attribute is the whole reason this app
/// worked at all.** It marks the function as the module's start function, which
/// `wasm-bindgen` emits into the wasm's start section and the generated glue
/// calls during initialization. A plain `#[wasm_bindgen]` — which is what this
/// was — declares an ordinary *export*: the symbol is in the wasm, and the glue
/// does not re-export it, so there is no way to reach it from JavaScript. The
/// shell's loader calls the module's default export, which only initializes the
/// module, and nothing ever called this. The result was a page that loaded
/// cleanly, drew its static shell, and then sat on "Loading stations…" forever
/// with no error anywhere — because nothing had failed. Nothing was running.
///
/// The body is synchronous on purpose: a start function cannot be awaited, so
/// the first thing it does is hand the real work to `spawn_local`.
///
/// The return value is a `JsValue` purely so a failure during setup can
/// surface as a thrown error for the loader's `catch`; the app renders its own
/// messages for anything a user can act on.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;

    let select: HtmlSelectElement = element(&document, "station")?
        .dyn_into()
        .map_err(|_| JsValue::from_str("#station is not a select"))?;
    let departures = element(&document, "departures")?;

    let app: Shared = Rc::new(App {
        window: window.clone(),
        document: document.clone(),
        select,
        departures,
        stations: RefCell::new(Vec::new()),
        selected: RefCell::new(None),
        timer: RefCell::new(None),
        fetched: RefCell::new(None),
    });

    // The picker. Its listener lives as long as the select does, so the
    // closure is leaked; the app is held strongly, because it is the page's.
    {
        let target = app.select.clone();
        let owned = Rc::clone(&app);
        let callback = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            on_change(&owned);
        });
        app.select
            .add_event_listener_with_callback("change", callback.as_ref().unchecked_ref())?;
        callback.forget();
        let _ = target;
    }
    watch_connection(&app)?;
    register_service_worker(&window)?;

    say(&app.departures, NO_STATION);
    spawn_local(async move {
        load_stations(&app).await;
    });
    Ok(())
}

/// The element with this id, or a `JsValue` naming it — a missing id is a bug
/// in the shell, not a runtime condition to handle.
fn element(document: &Document, id: &str) -> Result<Element, JsValue> {
    document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("#{id} is missing from the shell")))
}

/// Write the page's one standing notice.
///
/// The notice is the app's continuity line: it says where the data comes from,
/// and whether the board is live. It is deliberately separate from the
/// departures area, which is about one station's trains.
fn notice(app: &Shared, text: &str) {
    say(
        &element(&app.document, "notice").expect("#notice is in the shell"),
        text,
    );
}

/// Write a line of plain text into an element.
///
/// The original built its board as one HTML string and assigned it to
/// `innerHTML`. This sets `textContent` instead, which is the difference
/// between rendering data and interpreting it: a destination called
/// `<script>` is a string on a board, not a script. Every value that reaches
/// the page goes through here.
fn say(element: &Element, text: &str) {
    element.set_text_content(Some(text));
}

/// A paragraph, built and appended to a parent.
fn paragraph(parent: &Element, text: &str) -> Element {
    let document = parent
        .owner_document()
        .expect("an element has a document");
    let p = document
        .create_element("p")
        .expect("a <p> element");
    p.set_text_content(Some(text));
    parent.append_child(&p).expect("append a paragraph");
    p
}

/// A heading at the given level, built and appended to a parent.
fn heading(parent: &Element, level: u8, text: &str) -> Element {
    let document = parent
        .owner_document()
        .expect("an element has a document");
    let h = document
        .create_element(&format!("h{level}"))
        .expect("a heading element");
    h.set_text_content(Some(text));
    parent.append_child(&h).expect("append a heading");
    h
}

/// A cell, with an optional id used by the shell tests to find it.
fn cell(row: &Element, text: &str, tag: &str) -> Element {
    let document = row
        .owner_document()
        .expect("an element has a document");
    let element = document
        .create_element(tag)
        .expect("a cell element");
    element.set_text_content(Some(text));
    row.append_child(&element).expect("append a cell");
    element
}

/// Read the `station` query parameter, if there is one.
fn station_from_url(window: &Window) -> Option<String> {
    let location: Location = window.location();
    let search: String = location.search().ok()?;
    let params = UrlSearchParams::new_with_str(&search).ok()?;
    let value = params.get(STATION_PARAM)?;
    // The original wrote `station=` when the picker was emptied, which parses
    // back as an empty string rather than as absent. Treat it as absent, so an
    // emptied URL does not preselect the empty station.
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Write the station id into the query parameter, or take it out when there is
/// none.
///
/// `replaceState`, not `pushState`: the original's `history.replaceState` is
/// right here, and this board is not a place to build up a history of every
/// station the reader glanced at — the back button should leave the board, not
/// walk through it.
///
/// Rebuilding the search from the current one rather than from the bare path
/// keeps any other parameter the reader arrived with, which is the difference
/// between a board that can be linked with a theme and one that cannot.
fn station_to_url(window: &Window, station: Option<&str>) {
    let Ok(href) = window.location().href() else {
        return;
    };
    let Ok(url) = web_sys::Url::new(&href) else {
        return;
    };
    match station {
        Some(id) => url.search_params().set(STATION_PARAM, id),
        None => url.search_params().delete(STATION_PARAM),
    }
    // An emptied search must leave no trailing "?" behind.
    if url.search() == "?" {
        url.set_search("");
    }
    let Ok(history) = window.history() else {
        return;
    };
    let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&url.href()));
}

/// The page title, which carries the station so a reader with several boards
/// open can tell them apart.
fn set_title(app: &Shared, station: Option<&str>) {
    let title = match station {
        Some(name) => format!("London Rura - {name}"),
        None => "London Rura".to_string(),
    };
    app.document.set_title(&title);
}

/// Fetch the station list, fill the picker, and load whatever the URL asked for.
///
/// One request per line, all of them genuinely in flight at once, rather than
/// the single 17 MB request the original made. See [`LINE_IDS`] for the
/// measurements that decided it.
async fn load_stations(app: &Shared) {
    // Every line is fetched, and the futures are awaited directly. Two earlier
    // designs were wrong and both looked right:
    //
    // * Awaiting them one after another serialised the requests, which is the
    //   one thing the per-line fetch was for.
    // * Spawning each with `spawn_local` and then *polling a shared cell* until
    //   every slot was filled locked the renderer. A microtask yield — a
    //   resolved `Promise` awaited from Rust — does not hand control back to the
    //   browser's event loop, so the polling loop starved the very tasks it was
    //   waiting for, forever. The page hung, with no error, and the wasm was
    //   doing exactly what it was told.
    //
    // What is needed is the two halves of `futures::join_all`, which this crate
    // does not depend on and which is a few lines: poll each future once per
    // turn, and await whatever is ready before polling again. `wasm_bindgen_
    // futures` already polls the outer future on every microtask tick, so
    // awaiting a single `join` future built from all twelve is both concurrent
    // and non-blocking.
    let requests: Vec<LineRequest> = LINE_IDS
        .iter()
        .map(|line| {
            let line: &str = line;
            Box::pin(async move { (line, fetch_bytes(&line_url(line)).await) }) as LineRequest
        })
        .collect();
    let results: Vec<(&'static str, LineResult)> = join_all(requests).await;

    let mut collected: Vec<StopPoint> = Vec::new();
    let mut failed: Vec<&str> = Vec::new();
    for (line, result) in &results {
        match result {
            Ok(bytes) => match serde_json::from_slice::<Vec<StopPoint>>(bytes) {
                Ok(stops) => collected.extend(stops),
                Err(error) => {
                    web_sys::console::error_1(&JsValue::from_str(&format!("{line}: {error}")));
                    failed.push(line);
                }
            },
            Err(error) => {
                web_sys::console::error_1(&JsValue::from_str(&format!("{line}: {error}")));
                failed.push(line);
            }
        }
    }

    // Every line is fetched before anything is rendered, so the picker appears
    // once and complete rather than filling in a station at a time.
    let stations = station_list(collected);
    if stations.is_empty() {
        notice(app, STATIONS_FAILED);
        return;
    }
    fill_picker(app, &stations);
    notice(app, if failed.is_empty() { STATIONS_READY } else { PARTIAL });

    // The URL asks for a station. If the list has it, select it and load; if
    // not, say so rather than silently showing a different one.
    let Some(requested) = station_from_url(&app.window) else {
        return;
    };
    if !stations.iter().any(|station| station.id == requested) {
        notice(app, UNKNOWN_STATION);
        return;
    }
    select(app, &requested);
    app.load(&requested).await;
}

/// Await every future concurrently, and return the results in order.
///
/// This is `futures::join_all`, written out because the crate does not depend
/// on `futures` and one combinator does not justify it. The part that matters is
/// that it does **not** await the futures in sequence: it polls every one of
/// them on each turn of the loop and only yields when none of them is ready, so
/// all of them make progress at once.
///
/// A naive `for future in futures { out.push(future.await) }` here would compile,
/// pass every test, and serialise twelve requests into twelve round trips — the
/// exact thing the per-line fetch exists to avoid. Both this and a polling loop
/// over a shared cell have shipped in this file; the second froze the page, and
/// the first would have been invisible.
async fn join_all<F>(mut futures: Vec<Pin<Box<dyn Future<Output = F>>>>) -> Vec<F> {
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    // A no-op waker. Polling by hand needs a `Context`, and the outer future is
    // already being driven by `wasm_bindgen_futures` on every microtask tick,
    // so nothing here schedules anything: this only asks each inner future
    // whether it is ready *right now*.
    fn noop(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut context = Context::from_waker(&waker);

    let mut slots: Vec<Option<F>> = (0..futures.len()).map(|_| None).collect();
    let mut pending = futures.len();
    while pending > 0 {
        let mut progressed = false;
        for (index, future) in futures.iter_mut().enumerate() {
            if slots[index].is_some() {
                continue;
            }
            // The futures are already pinned in their boxes and are never moved
            // out — `slots` only receives their results — so polling by
            // reference here is sound.
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => {
                    slots[index] = Some(value);
                    pending -= 1;
                    progressed = true;
                }
                Poll::Pending => {}
            }
        }
        if pending > 0 && !progressed {
            // Nothing was ready, so give the browser a turn: the fetches behind
            // these futures cannot resolve until it has one. This is a real
            // event-loop yield (a macrotask via a zero-delay timer), not a
            // microtask, which is why the version that used
            // `Promise::resolve` deadlocked.
            yield_to_browser().await;
        }
    }
    slots.into_iter().flatten().collect()
}

/// Hand control back to the browser's event loop.
///
/// A zero-delay `setTimeout`, not a resolved microtask. A microtask is drained
/// before the browser returns to its event loop, so awaiting one from Rust does
/// not let a pending `fetch` make progress; polling the twelve futures that way
/// starves them and never terminates. A timer genuinely yields.
async fn yield_to_browser() {
    let promise = js_sys::Promise::new(&mut |resolve: js_sys::Function, _reject: js_sys::Function| {
        let Some(window) = web_sys::window() else {
            return;
        };
        // The promise's own `resolve` is a function, so it can be the timer
        // callback directly. `setTimeout(..., 0)` defers to the next turn of
        // the event loop, which is what makes this a yield rather than a no-op.
        let callback: &js_sys::Function = resolve.as_ref();
        let _ =
            window.set_timeout_with_callback_and_timeout_and_arguments_0(callback, 0);
    });
    let _ = JsFuture::from(promise).await;
}

/// The stop-point URL for one line id.
fn line_url(line: &str) -> String {
    LINE_STOPS_URL.replace("{line}", line)
}

/// One `Arrival`, as TfL sends it.
///
/// `timeToStation` and `platformName` are genuinely absent on some services,
/// which is why they are options rather than being defaulted to zero here: the
/// domain has to be able to tell "no platform" from "platform 0", and "arriving
/// now" from "no time given".
#[derive(serde::Deserialize)]
struct TflArrival {
    #[serde(rename = "lineName", default)]
    line_name: Option<String>,
    #[serde(rename = "destinationName", default)]
    destination_name: Option<String>,
    #[serde(rename = "timeToStation", default)]
    time_to_station: Option<i32>,
    #[serde(rename = "platformName", default)]
    platform_name: Option<String>,
}

impl From<TflArrival> for Arrival {
    fn from(arrival: TflArrival) -> Self {
        Self {
            line_name: arrival.line_name.unwrap_or_default(),
            destination: arrival.destination_name.unwrap_or_default(),
            time_to_station: arrival.time_to_station,
            platform: arrival.platform_name,
        }
    }
}

/// Fetch a URL as raw bytes, or an error a reader can be told about.
///
/// The bytes, not `response.json()`. `json()` resolves to a live JavaScript
/// object — the whole body, materialised as JS values before any Rust runs —
/// and deserialising that walks it property by property across the wasm
/// boundary, on the UI thread. On a large body that is long enough to be
/// visible: the page stops responding, which is indistinguishable from a
/// network that never answers. `array_buffer()` plus `from_slice` parses once,
/// in Rust, and the JavaScript object is never built.
///
/// TfL sends permissive CORS headers (`access-control-allow-origin: *`), so a
/// plain cross-origin `fetch` needs no proxy. It still fails for the ordinary
/// reasons — no connection, a 500, a rate limit, a body that is not JSON — and
/// every one of them ends here, because this app has nothing cached to fall
/// back on and nothing to retry with.
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|error| format!("{error:?}"))?;
    let response: Response = response
        .dyn_into()
        .map_err(|_| "the response is not a Response".to_string())?;
    if !response.ok() {
        return Err(format!("TfL answered {}", response.status()));
    }
    let buffer = JsFuture::from(
        response
            .array_buffer()
            .map_err(|error| format!("{error:?}"))?,
    )
    .await
    .map_err(|error| format!("{error:?}"))?;
    // `to_vec` on the typed-array view is a single copy of the bytes out of
    // wasm memory, not a per-element crossing of the boundary.
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}


/// The picker changed: take the new station, remember it, and show it.
fn on_change(app: &Shared) {
    let id = app.select.value();
    if id.is_empty() {
        // The original's empty-selection branch: clear the timer, take the
        // station out of the URL, put the title back, and say so.
        app.stop_timer();
        *app.selected.borrow_mut() = None;
        station_to_url(&app.window, None);
        set_title(app, None);
        say(&app.departures, NO_STATION);
        return;
    }
    let owned = Rc::clone(app);
    select(app, &id);
    spawn_local(async move {
        owned.load(&id).await;
    });
}

/// Fill the picker with the stations, in the order the domain sorted them.
///
/// The empty first option is the original's "Select a station", and it stays:
/// it is how the reader gets back to no station at all, which the change
/// handler treats as a real state — the timer stops, the URL is cleared and the
/// title goes back to plain "London Rura".
fn fill_picker(app: &Shared, stations: &[Station]) {
    let document = app.document.clone();
    // Options are rebuilt, not appended to, so a reload cannot double the list.
    while app.select.length() > 1 {
        app.select.remove_with_index(1);
    }
    for station in stations {
        let option = document
            .create_element("option")
            .expect("an <option> element");
        let option: HtmlOptionElement = option
            .dyn_into()
            .expect("an <option> element");
        option.set_value(&station.id);
        option.set_text(&station.name);
        app.select
            .add_with_html_option_element(&option)
            .expect("add a station");
    }
    *app.stations.borrow_mut() = stations.to_vec();
}

/// Record the selection, put it in the URL, title it, and start its timer.
fn select(app: &Shared, id: &str) {
    // Select by index rather than by value: setting a value the list does not
    // have silently leaves the picker on the first entry, which is how the
    // original could show one station while the URL named another. The caller
    // has already checked the id is in the list.
    if let Some(index) = app
        .stations
        .borrow()
        .iter()
        .position(|station| station.id == id)
    {
        app.select.set_selected_index(index as i32);
    }
    let name = app
        .stations
        .borrow()
        .iter()
        .find(|station| station.id == id)
        .map(|station| station.name.clone());
    *app.selected.borrow_mut() = Some(id.to_string());
    station_to_url(&app.window, Some(id));
    set_title(app, name.as_deref());
    app.start_timer();
}

/// How long ago the board was last fetched, in words.
///
/// `Performance::now` is monotonic and in milliseconds, so the span it gives
/// survives a system clock change — the one thing `Date.now` does not. The
/// wording is deliberately coarse: nobody needs to know that a board is 29
/// seconds old, they need to know whether it is still moving.
fn updated_ago(app: &Shared) -> String {
    let Some(fetched) = *app.fetched.borrow() else {
        return String::new();
    };
    let Some(performance) = app.window.performance() else {
        return String::new();
    };
    let seconds = ((performance.now() - fetched) / 1000.0).max(0.0) as u32;
    let ago = match seconds {
        0..=9 => "just now".to_string(),
        10..=59 => format!("{seconds} seconds ago"),
        _ => {
            let minutes = seconds / 60;
            match minutes {
                1 => "a minute ago".to_string(),
                _ => format!("{minutes} minutes ago"),
            }
        }
    };
    format!("Updated {ago}.")
}

/// Render the board.
///
/// The shape is the original's: a heading, then one table per platform, each
/// with a heading and three columns. The line cell carries the line's colour
/// and is marked up as such for anyone who cannot see the colour, which is the
/// one accessibility gap in a board whose whole signal is colour.
fn paint(app: &Shared, board: &Board) {
    let root = &app.departures;
    // Clear without `innerHTML`: these are elements, not a string to parse.
    while let Some(child) = root.first_child() {
        root.remove_child(&child).expect("remove a rendered child");
    }
    if board.is_empty() {
        paragraph(root, EMPTY);
        return;
    }
    heading(root, 2, "Departures by Platform");
    // A board of minutes is only as true as the fetch behind it. Saying when
    // that was costs one line and is the difference between a board that is
    // quiet and a board that is stale.
    let stamp = element_of(root, "p");
    let _ = stamp.set_attribute("class", "stamp");
    stamp.set_text_content(Some(&updated_ago(app)));
    root.append_child(&stamp).expect("stamp");
    for platform in &board.platforms {
        heading(root, 3, &platform.name);
        let table = element_of(root, "table");
        let head = element_of(root, "thead");
        let head_row = element_of(root, "tr");
        cell(&head_row, "Line", "th");
        cell(&head_row, "Destination", "th");
        cell(&head_row, "Arrival (min)", "th");
        head.append_child(&head_row).expect("head row");
        table.append_child(&head).expect("head");
        let body = element_of(root, "tbody");
        for departure in &platform.departures {
            let row = element_of(root, "tr");

            // The line's name, on a chip of the line's own published colour, in
            // whichever of black or white `line_ink` says is legible on it.
            // The chip is a `<span>` inside a plain cell, because a rounded
            // background on the cell itself would be rounded by the table too.
            let line_cell = element_of(root, "td");
            let chip = element_of(root, "span");
            let colour = line_colour(&departure.line);
            let _ = chip.set_attribute(
                "style",
                &format!("background-color: {colour}; color: {};", line_ink(colour)),
            );
            // `data-line` names the line in the markup as well, so a reader
            // using a stylesheet or a high-contrast mode still gets the words.
            let _ = chip.set_attribute("data-line", &departure.line);
            chip.set_text_content(Some(&departure.line));
            line_cell.append_child(&chip).expect("line chip");
            row.append_child(&line_cell).expect("line cell");

            cell(&row, &departure.destination, "td");

            // Zero minutes means the train is at the platform, which is not the
            // same as "no time" — it is the thing the reader is waiting for, so
            // it is marked rather than left to look like an empty value.
            let minutes = cell(&row, &departure.minutes.to_string(), "td");
            if departure.minutes <= 0 {
                let _ = minutes.set_attribute("class", "due");
            }
            body.append_child(&row).expect("row");
        }
        table.append_child(&body).expect("body");
        root.append_child(&table).expect("table");
    }
}

/// An element created from the document that owns `parent`.
fn element_of(parent: &Element, tag: &str) -> Element {
    parent
        .owner_document()
        .expect("an element has a document")
        .create_element(tag)
        .expect("create an element")
}

/// Report connectivity, so a board that cannot refresh says so before the
/// reader wonders why the minutes stopped moving.
fn watch_connection(app: &Shared) -> Result<(), JsValue> {
    let notice = element(&app.document, "notice")?;
    let online = app
        .window
        .navigator()
        .on_line();
    say(
        &notice,
        if online {
            "Live departures from the TfL API. This board needs a connection to update."
        } else {
            "You are offline. The board is the last one loaded; it will not update until you reconnect."
        },
    );
    // Keep the notice in step with the connection. This is not the app's data
    // — nothing is cached — only the reader's expectation of it.
    let owned = Rc::clone(app);
    let callback = Closure::<dyn FnMut()>::new(move || {
        let _ = watch_connection(&owned);
    });
    app.window
        .add_event_listener_with_callback("online", callback.as_ref().unchecked_ref())?;
    app.window
        .add_event_listener_with_callback("offline", callback.as_ref().unchecked_ref())?;
    callback.forget();
    Ok(())
}

/// Register the service worker, if this browser has one.
///
/// The worker caches the shell and nothing else — see
/// `src/service-worker.js`. It is registered, not driven from here: the board
/// never asks the cache for departures, because a cached arrivals list is
/// worse than no arrivals list.
///
/// Registration is deliberately not awaited. Nothing here can act on the
/// outcome — the board does not depend on the worker having installed, and the
/// one failure a reader could act on (no connection) already has its own
/// message in the departures area — so blocking startup on a promise nobody
/// waits for would trade a shell that paints immediately for a console line.
/// The release of a stale registration is likewise started rather than awaited,
/// and deliberately survives a registration that failed: that is exactly the
/// case where an older, wider registration may still be in the way.
fn register_service_worker(window: &Window) -> Result<(), JsValue> {
    // Relative to the page, so the worker works from any mount point. The scope
    // is stated rather than inherited: see [`SCOPE`].
    let options = web_sys::RegistrationOptions::new();
    options.set_scope(SCOPE);
    let _ = window
        .navigator()
        .service_worker()
        .register_with_options("./service-worker.js", &options);

    let owned: Window = window.clone();
    spawn_local(async move {
        release_stale_registrations(&owned).await;
    });
    Ok(())
}

/// Hand this app's own URLs back to the current worker.
///
/// A service worker is a registration, and a registration outlives the page
/// that made it: it is kept by the browser, not by the tab, and it keeps
/// answering for its scope until something explicitly unregisters it. That is
/// how a page on this origin can come to be served by a worker installed for a
/// *different* page, long after the app that installed it was closed. A stale
/// registration is not corrected by a reload, by a newer version of the board,
/// or by a newer worker installing itself — the newer worker only takes control
/// where its own scope reaches, and a wider stale one is still in the way.
///
/// So the repair is explicit: find any registration whose scope covers this
/// app's directory but is not this app's directory, and unregister it. This
/// app's own registration is left alone, and so is every other app on the
/// origin — each is scoped to its own directory, and a sibling that never
/// covered us is not ours to remove.
///
/// Failures are ignored on purpose. This is best-effort cleanup of state this
/// app did not create, and a browser that refuses leaves the reader no worse
/// off: the board still runs and still caches its own shell.
async fn release_stale_registrations(window: &Window) {
    let container = window.navigator().service_worker();
    let Ok(registrations) = JsFuture::from(container.get_registrations()).await else {
        return;
    };
    let Ok(array) = registrations.dyn_into::<js_sys::Array>() else {
        return;
    };

    // This app's own directory, as an absolute URL with a trailing slash. The
    // board is served from a subdirectory and every URL of ours is inside it.
    let Ok(home) = window.location().href() else {
        return;
    };
    let Ok(ours) = web_sys::Url::new_with_base(&home, "./") else {
        return;
    };
    let ours = ours.href();

    for entry in array.iter() {
        let Ok(registration) = entry.dyn_into::<web_sys::ServiceWorkerRegistration>() else {
            continue;
        };
        let scope = registration.scope();
        // Leave alone any scope that is this app's own, or narrower: a sibling
        // app mounted inside this directory is legitimate and separate, and
        // nothing there can intercept us. One test, because the two cases are
        // the same one: `ours` begins with `scope`.
        if ours.starts_with(&scope) {
            continue;
        }
        // What is left is a scope that is a *strict* prefix of ours: a worker
        // that would be consulted for this app's URLs while being registered
        // for more than this app. A worker is consulted for a URL exactly when
        // its scope is a prefix of that URL, which is the test above inverted.
        //
        // Of those, only our own worker qualifies: a different app's worker
        // lives in a different directory, so unregistering it would break the
        // app it belongs to.
        let script = registration
            .active()
            .map(|worker| worker.script_url())
            .unwrap_or_default();
        if script_belongs_to_app(&script, &ours) {
            match registration.unregister() {
                Ok(promise) => {
                    let _ = JsFuture::from(promise).await;
                }
                Err(error) => warn("a stale service worker could not be released", error),
            }
        }
    }
}

/// Whether a worker script at `script` is this app's own worker, registered for
/// more of the origin than this app's directory.
///
/// A wider scope means the script sits at the root of this app's own directory
/// rather than anywhere below it: a sibling app's worker is in a sibling
/// directory and does not match. `strip_suffix`, not `trim_end_matches` — the
/// latter strips a *set of characters*, so it would happily eat a directory
/// named `...e-worker.js` and call it ours, tearing down a sibling's worker.
fn script_belongs_to_app(script: &str, ours: &str) -> bool {
    match web_sys::Url::new(script) {
        Ok(url) => url.href().strip_suffix("service-worker.js") == Some(ours),
        Err(_) => false,
    }
}

/// Report something recoverable. Never a panic, never silent — but never a
/// status line either, because nothing `ui.rs` writes to `#notice` is about the
/// app's own plumbing.
fn warn(context: &str, error: JsValue) {
    web_sys::console::warn_2(&JsValue::from_str(context), &error);
}
