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
use web_sys::{Document, Element, HtmlInputElement, Location, Response, UrlSearchParams, Window};

use crate::departures::{
    board, departure_colour, fallback_queries, line_ink, station_index, Arrival, Board, Platform,
    Search, SearchResponse, Station, StopDetail, StopPoint, StopPointDetail, StopPointList,
    ARRIVALS_BASE, FETCHED_MODES, MODE_STOPS_URL, SEARCH_BASE,
};
use crate::modes::{Mode, ModeSet};

/// Shown for a station TfL lists but publishes no departures for.
///
/// This is the national rail case, and it is a real gap rather than a bug: every
/// national-rail stop point answers `/Arrivals` with an empty array. Saying so is
/// better than the two alternatives — showing the station as if it were broken,
/// or leaving it out of the picker so the reader concludes it does not exist.
const NO_DEPARTURES_PUBLISHED: &str =
    "TfL does not publish departures for this station. Try one of its bus or \
     Underground stops, or check the operator's own site.";

/// Stands in for an arrival with no destination named.
///
/// TfL sends an empty `destinationName` for some services. An empty cell reads
/// as a broken table; a dash reads as "none given", which is the truth.
const NO_DESTINATION: &str = "\u{2014}";

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

/// Shown when TfL refused the request because this app asked for too much.
///
/// **A separate message, and it is the one a reader can act on.** The generic
/// `FAILED` covers a dead network and a broken endpoint, neither of which the
/// reader can do anything about; being throttled is the one failure this app
/// causes itself, and it clears on its own within seconds. So it says what is
/// happening and what to do about it, instead of sending the reader away to
/// retry into the same limit.
///
/// Measured on 2026-10-02: typing one eight-letter word into the search box
/// produced 201 requests, and every one of them ended on `FAILED`.
const RATE_LIMITED: &str = "TfL is rate-limiting this app. Wait a few seconds and try again.";

/// Shown when a search matches nothing, on the board's own terms.
///
/// It is not an error: TfL's search covers every stop point in London and a
/// reader typing a misspelling gets nothing back, which is a fact about the
/// query rather than about the network.
const NO_MATCH: &str = "No station matches that.";

/// Shown when the reader has switched off every mode a station was served by.
const STATION_FILTERED_OUT: &str = "That station is not served by the modes you have switched on.";

/// Shown when the board itself is empty only because a switch is hiding it.
///
/// A station with services, all of them switched off, must not read as a station
/// with no services: the reader needs to know the difference between "nothing is
/// running" and "you have turned this off", because they can undo the second.
const MODES_FILTERED_BOARD: &str =
    "Every service at this station is in a mode you have switched on. Turn one \
     back on to see it.";

/// Shown when a reader tries to switch off the last remaining mode.
const NO_MODES_LEFT: &str = "Keep at least one mode switched on.";

/// Shown when a search finds nothing locally and TfL is asked instead.
///
/// This is the bus case: there is no bus list to download, so a search is the
/// only route to a bus stop.
const SEARCHING: &str = "Searching…";

/// How many characters to type before asking TfL about a bus stop.
///
/// Two characters is enough to be specific and short enough not to fire on every
/// keystroke of a word the reader is still writing.
const MIN_SEARCH_CHARS: usize = 2;

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
const PARTIAL: &str =
    "Some lines could not be reached, so the list may be short. Try again shortly.";

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
    /// Where the board is rendered.
    departures: Element,
    /// Where the search suggestions are rendered.
    results: Element,
    /// Where the mode toggles are rendered.
    toggles: Element,
    /// Where the current station's name is shown.
    heading: Element,
    /// Where the freshness stamp is shown, in the header.
    stamp: Element,
    /// The search field.
    search: HtmlInputElement,
    /// Every station the fetches found, with an id per mode.
    ///
    /// This is the whole list; the picker filters it rather than holding its own
    /// copy, so switching a mode on cannot leave a stale option behind.
    stations: RefCell<Vec<Station>>,
    /// Which modes are switched on. The toggle's whole state.
    modes: RefCell<ModeSet>,
    /// The id of the station on the board, or `None` when nothing is chosen.
    selected: RefCell<Option<String>>,
    /// The refresh timer. `None` while no station is chosen, because there is
    /// nothing to refresh.
    timer: RefCell<Option<i32>>,
    /// When the last successful fetch landed, as `Performance::now()`. `None`
    /// until the first one, and left at the old value when a refresh fails, so
    /// the stamp keeps counting up and the staleness becomes visible.
    fetched: RefCell<Option<f64>>,
    /// The board as last fetched, before the mode switches are applied.
    ///
    /// This is what makes a toggle instant and a refresh quiet. The board is
    /// fetched per stop point and covers every mode the station has; which of
    /// those modes the reader wants to see is a *view* of that fetch, not a
    /// different fetch. So the raw board is kept here and every toggle simply
    /// re-filters it — no request, and no chance of the board blanking and
    /// refilling while the reader is reading it.
    ///
    /// `None` until the first arrival lands for the selected station.
    raw_board: RefCell<Option<Board>>,
    /// Whether a station search is already in flight, so a reader typing quickly
    /// does not queue four requests behind one keyboard.
    searching: RefCell<bool>,
    /// The pending debounced search, if one is scheduled. Replacing it cancels
    /// the previous, so the last keystroke is the one that searches.
    debounce: RefCell<Option<i32>>,
    /// What the debounced search will look for. Stored rather than captured by
    /// the timer's closure; see `debounce_search` for why.
    pending_query: RefCell<String>,
}

/// How long to wait after the last keystroke before asking TfL.
///
/// 250 ms is long enough that typing "barking" is one request rather than
/// seven, and short enough that the list still feels attached to the keyboard.
const SEARCH_DEBOUNCE_MS: i32 = 250;

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
    ///
    /// **A refresh repaints the timetable and nothing else.** The station name,
    /// the URL, the page title, the mode switches and the search box are all
    /// left exactly as they are, and so is the table already on screen until
    /// the new arrivals have actually arrived. That is the whole of "the whole
    /// website refreshes to get a new timetable": every one of those things is
    /// a function of the *station*, not of the time, and re-deriving them
    /// thirty seconds apart is thirty seconds of churn the reader can see.
    ///
    /// The board is stored whole and the switches applied at paint time, so a
    /// mode toggled on later re-filters this same fetch instead of asking TfL
    /// again.
    async fn load(self: &Shared, station: &str) {
        // Only *before* the first board is there does the reader need to be
        // told a request is in flight. Afterwards the previous board stays on
        // screen and simply ages — blanking it every thirty seconds to write
        // "Loading departures…" and then rebuild it is the churn itself.
        if self.raw_board.borrow().is_none() {
            say(&self.departures, LOADING);
        }
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
                web_sys::console::error_1(&JsValue::from_str(&error.to_string()));
                // A refresh that fails must not throw away the board the reader
                // is already looking at: a departure list thirty seconds stale
                // is worth more than a page saying it could not refresh.
                if self.raw_board.borrow().is_none() {
                    say(&self.departures, error.message());
                }
                return;
            }
        };
        *self.fetched.borrow_mut() = self.window.performance().map(|p| p.now());
        let made = board(arrivals.into_iter().map(Arrival::from).collect());
        *self.raw_board.borrow_mut() = Some(made);
        self.repaint();
    }

    /// Draw the board as the current switches want it, from the last fetch.
    ///
    /// Split out from [`load`](App::load) so that switching a mode is a repaint
    /// of data already in hand rather than a second round trip to TfL.
    ///
    /// When a switch is hiding services at this station, the board says so. The
    /// alternative is a reader who turns the buses off at an interchange seeing
    /// the Underground's trains and concluding the buses were cancelled.
    fn repaint(self: &Shared) {
        let Some(raw) = self.raw_board.borrow().clone() else {
            return;
        };
        let modes = *self.modes.borrow();
        let filtered = raw.filtered(modes);
        // A board emptied by a switch is a *chosen* state, not a quiet one, so
        // it gets its own message instead of the empty-board text.
        if filtered.is_empty() && raw.has_hidden_platforms(modes) {
            let root = &self.departures;
            clear(root);
            say(&self.stamp, &updated_ago(self));
            paragraph(root, MODES_FILTERED_BOARD);
            return;
        }
        paint(self, &filtered);
    }
}

/// The application, as one shared `Rc` — every handler and the timer share it.
type Shared = Rc<App>;

/// What one mode's fetch produced: its raw bytes, or why it has none.
type LineResult = Result<Vec<u8>, FetchError>;

/// One station-list fetch, boxed and pinned so the futures can be polled by
/// hand.
type ModeRequest = Pin<Box<dyn Future<Output = (Mode, LineResult)>>>;

/// One stop-detail fetch, boxed and pinned the same way.
type DetailRequest = Pin<Box<dyn Future<Output = (String, LineResult)>>>;

/// Install the app: build the mode toggles, wire the search box and the
/// connectivity listeners, read the URL, and start fetching.
///
/// **`#[wasm_bindgen(start)]`, and that attribute is the whole reason this app
/// works at all.** It marks the function as the module's start function, which
/// `wasm-bindgen` emits into the wasm's start section and the generated glue
/// calls during initialisation. A plain `#[wasm_bindgen]` — which is what this
/// once was — declares an ordinary *export*: the symbol is in the wasm, and the
/// glue does not re-export it, so there is no way to reach it from JavaScript.
/// The shell's loader calls the module's default export, which only initialises
/// the module, and nothing ever called this. The result was a page that loaded
/// cleanly, drew its static shell, and then sat on its loading message forever
/// with no error anywhere — because nothing had failed. Nothing was running.
///
/// The body is synchronous on purpose: a start function cannot be awaited, so it
/// does its wiring and hands the real work to `spawn_local`.
///
/// The return value is a `JsValue` purely so a failure during setup can surface
/// as a thrown error for the loader's `catch`; the app renders its own messages
/// for anything a user can act on.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;

    let search: HtmlInputElement = element(&document, "search")?
        .dyn_into()
        .map_err(|_| JsValue::from_str("#search is not an input"))?;
    let departures = element(&document, "departures")?;
    let results = element(&document, "results")?;
    let toggles = element(&document, "toggles")?;
    let heading = element(&document, "station-name")?;
    let stamp = element(&document, "stamp")?;

    let app: Shared = Rc::new(App {
        window: window.clone(),
        document: document.clone(),
        departures,
        results,
        toggles,
        heading,
        stamp,
        search,
        stations: RefCell::new(Vec::new()),
        modes: RefCell::new(ModeSet::default_on()),
        selected: RefCell::new(None),
        timer: RefCell::new(None),
        raw_board: RefCell::new(None),
        fetched: RefCell::new(None),
        searching: RefCell::new(false),
        debounce: RefCell::new(None),
        pending_query: RefCell::new(String::new()),
    });

    if let Some(from_url) = modes_from_url(&window) {
        *app.modes.borrow_mut() = from_url;
    }
    build_toggles(&app)?;
    listen(&app, "search", "input", on_typing)?;
    // Enter only. NOT `change`: that fires on blur, which a click on a
    // suggestion causes, and it made every click pick the top row.
    listen(&app, "search", "keydown", on_keydown)?;
    watch_connection(&app)?;
    register_service_worker(&window)?;

    say(&app.departures, NO_STATION);
    spawn_local(async move {
        load_stations(&app).await;
    });
    Ok(())
}

/// Attach an event listener that keeps the app alive for the page's lifetime.
fn listen(
    app: &Shared,
    id: &str,
    event: &str,
    handler: impl Fn(&Shared) + 'static,
) -> Result<(), JsValue> {
    let target = element(&app.document, id)?;
    let owned = Rc::clone(app);
    let callback = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
        handler(&owned);
    });
    target.add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())?;
    // A listener on a live element is owned by that element; forgetting the
    // closure is what keeps it callable. The element outlives the closure.
    callback.forget();
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
    let document = parent.owner_document().expect("an element has a document");
    let p = document.create_element("p").expect("a <p> element");
    p.set_text_content(Some(text));
    parent.append_child(&p).expect("append a paragraph");
    p
}

/// A heading at the given level, built and appended to a parent.
fn heading(parent: &Element, level: u8, text: &str) -> Element {
    let document = parent.owner_document().expect("an element has a document");
    let h = document
        .create_element(&format!("h{level}"))
        .expect("a heading element");
    h.set_text_content(Some(text));
    parent.append_child(&h).expect("append a heading");
    h
}

/// A cell, with an optional id used by the shell tests to find it.
fn cell(row: &Element, text: &str, tag: &str) -> Element {
    let document = row.owner_document().expect("an element has a document");
    let element = document.create_element(tag).expect("a cell element");
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

/// The modes a link asked for, or `None` to keep the defaults.
fn modes_from_url(window: &Window) -> Option<ModeSet> {
    let search: String = window.location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(search.trim_start_matches('?')).ok()?;
    let raw = params.get(MODES_PARAM)?;
    let mut modes = ModeSet::empty();
    for name in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(mode) = Mode::from_api_name(name) {
            modes.insert(mode);
        }
    }
    // A parameter that selects nothing is a malformed link, not a request for
    // an empty board; the defaults are a better answer than a blank screen.
    (!modes.is_empty()).then_some(modes)
}

/// Put the mode switches in the query parameter, so a link carries what the
/// reader chose to see.
///
/// This is what makes a filtered board shareable: "these are the bus stops near
/// me" is a thing a reader sends to someone, and without it the link opens on
/// whatever the reader's default was.
fn modes_to_url(window: &Window, modes: ModeSet) {
    let Ok(href) = window.location().href() else {
        return;
    };
    let Ok(url) = web_sys::Url::new(&href) else {
        return;
    };
    if modes == ModeSet::default_on() {
        // The default is the absence of a parameter, so a link to a plain
        // station board is short and does not change when the defaults do.
        url.search_params().delete(MODES_PARAM);
    } else {
        let names: Vec<&str> = modes.active().iter().map(|mode| mode.api_name()).collect();
        url.search_params().set(MODES_PARAM, &names.join(","));
    }
    if url.search() == "?" {
        url.set_search("");
    }
    let Ok(history) = window.history() else {
        return;
    };
    let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(&url.href()));
}

/// The query parameter the mode switches live in.
const MODES_PARAM: &str = "modes";

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
/// Fetch every mode's stop points and build the station index.
///
/// One request per mode, all of them genuinely in flight at once. The `join_all`
/// below is not a convenience: the two designs it replaced each looked right and
/// were not — see its own comment. The requests are issued together, parsed
/// straight from bytes, and merged by name so one interchange is one row with an
/// id per mode.
async fn load_stations(app: &Shared) {
    let requests: Vec<ModeRequest> = FETCHED_MODES
        .iter()
        .map(|mode| {
            let mode = *mode;
            Box::pin(async move { (mode, fetch_bytes(&mode_url(mode)).await) }) as ModeRequest
        })
        .collect();
    let answers = join_all(requests).await;

    let mut per_mode: Vec<(Mode, Vec<StopPoint>)> = Vec::new();
    let mut failed: Vec<&'static str> = Vec::new();
    for (mode, answer) in answers {
        match answer {
            // The mode endpoint wraps its list in an object, `{"stopPoints":
            // [...]}`, while the per-*line* endpoint returns a bare array and the
            // search endpoint a `{"matches": [...]}` object. Three shapes, one
            // code path, so the wrapper is peeled here rather than three times
            // at the call site.
            Ok(bytes) => match serde_json::from_slice::<StopPointList>(&bytes) {
                Ok(list) => per_mode.push((mode, list.stop_points)),
                Err(error) => {
                    warn_text(&format!("{mode:?}: {error}"));
                    failed.push(mode.api_name());
                }
            },
            Err(error) => {
                warn_text(&format!("{mode:?}: {error}"));
                failed.push(mode.api_name());
            }
        }
    }

    let stations = station_index(per_mode);
    if stations.is_empty() {
        notice(app, STATIONS_FAILED);
        return;
    }
    let count = stations.len();
    *app.stations.borrow_mut() = stations;
    let _ = refresh_suggestions(app);
    notice(
        app,
        if failed.is_empty() {
            STATIONS_READY
        } else {
            PARTIAL
        },
    );

    // The URL asks for a station. If the list has it, show it; if not, say so
    // rather than silently showing a different one.
    if let Some(requested) = station_from_url(&app.window) {
        let known = app
            .stations
            .borrow()
            .iter()
            .any(|station| station.all_ids().any(|id| id == requested));
        if known {
            show_station(app, &requested);
        } else {
            notice(app, UNKNOWN_STATION);
        }
    }
    let _ = count;
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
    let promise = js_sys::Promise::new(
        &mut |resolve: js_sys::Function, _reject: js_sys::Function| {
            let Some(window) = web_sys::window() else {
                return;
            };
            // The promise's own `resolve` is a function, so it can be the timer
            // callback directly. `setTimeout(..., 0)` defers to the next turn of
            // the event loop, which is what makes this a yield rather than a no-op.
            let callback: &js_sys::Function = resolve.as_ref();
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(callback, 0);
        },
    );
    let _ = JsFuture::from(promise).await;
}

/// The stop-point URL for one line id.
fn mode_url(mode: Mode) -> String {
    MODE_STOPS_URL.replace("{mode}", mode.api_name())
}

/// The search URL for what a reader has typed.
fn search_url(query: &str) -> String {
    format!("{SEARCH_BASE}{}", encode_uri_component(query))
}

/// Percent-encode a query for a URL path segment.
///
/// Written out rather than pulled in, because it is one small function and the
/// crate has no HTTP dependency. Unreserved characters pass through; everything
/// else becomes UTF-8 percent-encoded, which is what a path segment needs for
/// "King's Cross" and "St. Pancras".
fn encode_uri_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
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
    /// Where the service is told to go when it has no destination name.
    ///
    /// **This is not a substitute for `destinationName`; it is what TfL puts
    /// there instead, and it is better than a dash.** A Hammersmith & City
    /// service at King's Cross arrives with an empty `destinationName` and
    /// `towards: "Check Front of Train"` — a working notice for the crew, not a
    /// passenger-facing destination. Shown as a dash it read as a row whose data
    /// was missing; shown as itself it is the thing TfL actually says, and a
    /// reader standing on that platform needs it.
    ///
    /// It is only a fallback, and only when it says something: TfL also sends
    /// `towards` on ordinary services, where it duplicates the destination, and
    /// a duplicate would be worse than neither.
    #[serde(rename = "towards", default)]
    towards: Option<String>,
    #[serde(rename = "timeToStation", default)]
    time_to_station: Option<i32>,
    #[serde(rename = "platformName", default)]
    platform_name: Option<String>,
    /// Which mode this service runs on.
    ///
    /// This is what the mode toggles filter the board by, and what picks a
    /// row's colour when its line has none of its own. TfL sends it on every
    /// arrival as `modeName` — "tube", "bus", "river-bus", "elizabeth-line" —
    /// and it is the only field that can tell a bus route number from a train
    /// line, because the *name* of a line cannot.
    #[serde(rename = "modeName", default)]
    mode_name: Option<String>,
}

impl From<TflArrival> for Arrival {
    fn from(arrival: TflArrival) -> Self {
        Self {
            line_name: arrival.line_name.clone().unwrap_or_default(),
            destination: destination_of(&arrival),
            time_to_station: arrival.time_to_station,
            platform: arrival.platform_name.clone(),
            mode: arrival.mode_name.as_deref().and_then(Mode::from_api_name),
        }
    }
}

/// What to show in a row's destination cell.
///
/// `destinationName` when there is one, then `towards` when that says something
/// the name did not, and an empty string when neither does — which the renderer
/// draws as a dash.
///
/// The middle step is the one that matters. TfL sends an empty
/// `destinationName` on a handful of services and puts the real instruction in
/// `towards` instead; at King's Cross that is `"Check Front of Train"` on the
/// Hammersmith & City rows. A dash there said "no destination given" when TfL
/// had given one, on exactly the platform where a reader most needs it.
fn destination_of(arrival: &TflArrival) -> String {
    let named = arrival
        .destination_name
        .as_deref()
        .unwrap_or_default()
        .trim();
    if !named.is_empty() {
        return named.to_string();
    }
    arrival
        .towards
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Throttle: pause this long between requests to TfL, in milliseconds.
///
/// **The measured reason this exists.** TfL's free API rate-limits by IP and
/// answers a burst with HTTP 429 and `"Rate limit is exceeded"` — measured on
/// 2026-10-02: 25 concurrent requests to distinct URLs came back 200 three
/// times running and 429 the fourth, and 40 concurrent came back 429 every
/// time. Before this, one reader typing a single eight-letter word into the
/// search box produced **201** requests, and every one of them drew the same
/// wall of error a genuine outage draws.
///
/// So the app now spends its own traffic slowly enough to stay inside the
/// limit. This is a floor on the *spacing* of requests, not a cap on how many
/// it may make: a board that has answers to give is worth a second of latency,
/// and a board that starves itself to stay quiet is worse than a slow one.
const TFL_REQUEST_GAP_MS: i32 = 120;

/// Why a request to TfL failed, in the two ways a reader is told apart.
///
/// A rate limit and a dead network are different problems with different
/// remedies, and they used to produce the same seven words on screen. A reader
/// whose phone dropped needs to know the board is not coming back until they
/// reconnect; a reader being throttled needs to know to stop hammering the
/// search box for a few seconds. Collapsing both into "try again later" is what
/// made the real cause hard to see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchError {
    /// TfL answered 429: this app asked for too much, too quickly.
    RateLimited,
    /// No connection, a 500, a 404, or a body that could not be read.
    Other,
}

impl FetchError {
    /// The word a reader is shown, which is the whole difference between them.
    pub fn message(self) -> &'static str {
        match self {
            Self::RateLimited => RATE_LIMITED,
            Self::Other => FAILED,
        }
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateLimited => f.write_str("TfL rate-limited this app"),
            Self::Other => f.write_str("TfL could not be reached"),
        }
    }
}

/// Fetch a URL as raw bytes, or the reason a reader can be told about.
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
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, FetchError> {
    let window = web_sys::window().ok_or(FetchError::Other)?;
    throttle().await;
    let response = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|_| FetchError::Other)?;
    let response: Response = response.dyn_into().map_err(|_| FetchError::Other)?;
    if !response.ok() {
        // 429 is the one non-OK status that is this app's own doing rather
        // than TfL's, and it is worth distinguishing for exactly that reason.
        return Err(if response.status() == 429 {
            FetchError::RateLimited
        } else {
            FetchError::Other
        });
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(|_| FetchError::Other)?)
        .await
        .map_err(|_| FetchError::Other)?;
    // `to_vec` on the typed-array view is a single copy of the bytes out of
    // wasm memory, not a per-element crossing of the boundary.
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

/// Wait long enough that this request is not part of a burst.
///
/// One timer for the whole app, because the limit is on the app's traffic and
/// not on any one call: five callers each keeping their own tally would still
/// add up to a burst between them. A zero-delay timer for the very first
/// request, so opening the board is not itself delayed.
async fn throttle() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let now = window.performance().map_or(0.0, |p| p.now());
    let last = LAST_REQUEST.with(|cell| cell.replace(now));
    let wait = (TFL_REQUEST_GAP_MS as f64) - (now - last);
    if wait <= 0.0 {
        return;
    }
    let promise = js_sys::Promise::new(&mut |resolve: js_sys::Function, _| {
        let _ = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(resolve.as_ref(), wait as i32);
    });
    let _ = JsFuture::from(promise).await;
}

thread_local! {
    /// When this app last asked TfL for anything, as `Performance::now`.
    ///
    /// Deliberately process-wide rather than per-app: the budget that matters is
    /// the IP's, and everything this page sends shares it.
    static LAST_REQUEST: std::cell::Cell<f64> = const { std::cell::Cell::new(f64::NEG_INFINITY) };
}

/// The picker changed: take the new station, remember it, and show it.
/// Draw the mode toggles.
///
/// One button per mode, showing whether it is on. They are real `<button>`
/// elements with `aria-pressed` rather than styled divs, so a keyboard reaches
/// them, a screen reader announces the state, and Enter works without a click
/// handler on a non-interactive element.
///
/// The colour is the mode's own, and the label sits on it in whichever of black
/// or white is legible — the same rule the line chips use, from the same
/// function, so no colour on this page is picked by eye.
fn build_toggles(app: &Shared) -> Result<(), JsValue> {
    let root = &app.toggles;
    clear(root);
    let modes = *app.modes.borrow();
    for mode in crate::modes::MODES {
        let on = modes.contains(*mode);
        let button = element_of(root, "button");
        let colour = mode.colour();
        let _ = button.set_attribute("type", "button");
        let _ = button.set_attribute("class", if on { "toggle on" } else { "toggle" });
        let _ = button.set_attribute("aria-pressed", if on { "true" } else { "false" });
        let _ = button.set_attribute(
            "style",
            &format!("--chip: {colour}; --chip-ink: {};", line_ink(colour)),
        );
        button.set_text_content(Some(&format!("{} {}", mode.glyph(), mode.label())));
        let _ = button.set_attribute("data-mode", mode.api_name());

        let owned = Rc::clone(app);
        let callback = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            toggle_mode(&owned, *mode);
        });
        button.add_event_listener_with_callback("click", callback.as_ref().unchecked_ref())?;
        callback.forget();
        root.append_child(&button).expect("append a mode toggle");
    }
    Ok(())
}

/// Switch a mode on or off, and re-draw everything that depends on it.
///
/// Switching a mode off must not lose the station on the board when that station
/// is still served by another mode that is on, and it must say so when it is
/// not — a reader who turns the Underground off while looking at Acton Town
/// would otherwise be left staring at a board they have switched off.
///
/// **No request is made.** The board was fetched whole and is held; which modes
/// of it to show is a view, so this repaints what is already in hand. That is
/// what makes the switch instant, and it is also why the board cannot go blank
/// and refill underneath a reader who is mid-sentence on it.
fn toggle_mode(app: &Shared, mode: Mode) {
    let mut modes = *app.modes.borrow();
    if modes.contains(mode) {
        // Refusing to switch the last mode off is deliberate: a board with no
        // modes is not a state a reader can get anything from, and an empty
        // screen looks broken rather than chosen.
        if modes.active().len() == 1 {
            notice(app, NO_MODES_LEFT);
            return;
        }
        modes.remove(mode);
    } else {
        modes.insert(mode);
    }
    *app.modes.borrow_mut() = modes;

    if build_toggles(app).is_err() {
        return;
    }
    let _ = refresh_suggestions(app);
    modes_to_url(&app.window, modes);

    // If the station on the board is still reachable, re-filter it in place; if
    // not, say so and clear the board rather than showing a station that is now
    // filtered out.
    if let Some(id) = app.selected.borrow().clone() {
        let still = app
            .stations
            .borrow()
            .iter()
            .any(|station| station.all_ids().any(|candidate| *candidate == id));
        if still {
            app.repaint();
        } else {
            app.stop_timer();
            *app.selected.borrow_mut() = None;
            *app.raw_board.borrow_mut() = None;
            say(&app.heading, "");
            say(&app.departures, STATION_FILTERED_OUT);
        }
    }
}

/// Redraw the suggestion list from the current search text and mode switches.
fn refresh_suggestions(app: &Shared) -> Result<(), JsValue> {
    let query = app.search.value();
    let root = &app.results;
    clear(root);
    let modes = *app.modes.borrow();

    // With no query, offer the first few stations rather than nothing: a reader
    // who has not typed yet should see that the board is ready and what it
    // offers.
    // The suggestions are owned rather than borrowed: a `Ref` held across the
    // rendering below would be a runtime borrow panic the moment anything
    // touched the list, and the data is a handful of small structs.
    let stations = app.stations.borrow();
    let shown: Vec<Station> = if query.trim().is_empty() {
        stations.iter().take(6).cloned().collect()
    } else {
        Search::new(&stations)
            .query(&query, modes)
            .into_iter()
            .cloned()
            .collect()
    };
    drop(stations);

    if shown.is_empty() {
        if !query.trim().is_empty() {
            paragraph(root, NO_MATCH);
        }
        return Ok(());
    }
    for station in shown {
        let id = match station.id_for(modes).or_else(|| station.all_ids().next()) {
            Some(id) => id.to_string(),
            None => continue,
        };
        let row = element_of(root, "button");
        let _ = row.set_attribute("type", "button");
        let _ = row.set_attribute("class", "result");
        // The name goes in a span of its own rather than as the row's text, so
        // appending a chip afterwards cannot replace it.
        let name = element_of(&row, "span");
        let _ = name.set_attribute("class", "name");
        name.set_text_content(Some(&station.name));
        row.append_child(&name).expect("station name");

        // The mode chips on the row, so an interchange shows what it connects
        // before it is chosen rather than after.
        for mode in &station.ids {
            let chip = element_of(&row, "span");
            let colour = mode.0.colour();
            let _ = chip.set_attribute("class", "chip small");
            let _ = chip.set_attribute(
                "style",
                &format!("--chip: {colour}; --chip-ink: {};", line_ink(colour)),
            );
            chip.set_text_content(Some(mode.0.short_label()));
            // `element_of` creates the span *in the document*; appending is a
            // separate step and was missing, so the chips existed and were then
            // discarded when the row's text was set again.
            row.append_child(&chip).expect("mode chip");
        }
        if let Some(locality) = &station.locality {
            let where_ = element_of(&row, "span");
            let _ = where_.set_attribute("class", "locality");
            where_.set_text_content(Some(locality));
            row.append_child(&where_).expect("locality");
        }
        // What tells two same-named stops apart. Empty for a station whose name
        // is already unique — an Underground station is not "also" anything —
        // and it is the only thing that separates "Oxford Circus Station" from
        // the other three of them in the same forecourt.
        let described = station
            .detail
            .as_ref()
            .map_or_else(String::new, StopDetail::describe);
        if !described.is_empty() {
            let detail = element_of(&row, "span");
            let _ = detail.set_attribute("class", "detail");
            detail.set_text_content(Some(&described));
            row.append_child(&detail).expect("stop detail");
        }

        let owned = Rc::clone(app);
        let callback = Closure::<dyn FnMut(web_sys::Event)>::new(move |_: web_sys::Event| {
            let app = Rc::clone(&owned);
            // Cloned inside the closure, not moved into it: a closure that takes
            // its captured value is `FnOnce`, and a listener has to be `FnMut`
            // because it fires as many times as the reader clicks.
            let id = id.clone();
            spawn_local(async move {
                show_station_by_id(&app, &id).await;
            });
        });
        row.add_event_listener_with_callback("click", callback.as_ref().unchecked_ref())?;
        callback.forget();
        root.append_child(&row).expect("append a suggestion");
    }
    Ok(())
}

/// A reader typed something: debounce, then either filter the local list or ask
/// TfL for bus stops the local list does not have.
fn on_typing(app: &Shared) {
    // Editing the search brings the suggestions back; choosing one hid them.
    let _ = app.document.body().map(|body| body.set_class_name(""));
    let query = app.search.value();
    let modes = *app.modes.borrow();
    // Buses and national rail both come from search rather than from a fetched
    // list: `StopPoint/Mode/bus` is a 400 and `StopPoint/Mode/national-rail`
    // times out (504), so neither is downloadable and both are reachable only by
    // asking TfL what it has at a place.
    let needs_network = query.trim().len() >= MIN_SEARCH_CHARS
        && (modes.contains(Mode::Bus) || modes.contains(Mode::NationalRail))
        && !*app.searching.borrow();

    // Always filter what is already loaded: it is instant, and it is the answer
    // for every mode except buses.
    let _ = refresh_suggestions(app);
    if needs_network {
        debounce_search(app);
    }
}

/// Search TfL once the reader has stopped typing.
///
/// The delay is what turns "barking" — seven keystrokes — into one request
/// rather than seven. Without it the search endpoint sees every prefix of every
/// word, which is both slow and a good way to get rate limited.
///
/// The pending query is *stored* rather than captured, and one long-lived timer
/// reads it. Capturing the query in the timer's closure would make that closure
/// `FnOnce` — it would move the string in — and a closure handed to
/// `setTimeout` has to be `FnMut`, because the browser is free to call it more
/// than once over a page's life if the handle is ever reused.
fn debounce_search(app: &Shared) {
    *app.pending_query.borrow_mut() = app.search.value();

    // Cancel anything already waiting, so only the last keystroke survives.
    let previous = app.debounce.replace(None);
    if let Some(handle) = previous {
        app.window.clear_timeout_with_handle(handle);
    }

    // The closure owns its own `Rc`: `setTimeout` needs a `'static` callback and
    // the app outlives it. It reads the pending query and the current modes
    // through that `Rc`, so a search always uses the modes as they are when the
    // timer fires rather than as they were when it was scheduled.
    let app_ref: Shared = Rc::clone(app);
    let callback = Closure::<dyn FnMut()>::new(move || {
        let query = app_ref.pending_query.borrow().clone();
        let modes = *app_ref.modes.borrow();
        spawn_search(&app_ref, query, modes);
    });
    let handle = app
        .window
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            SEARCH_DEBOUNCE_MS,
        )
        .expect("the search debounce");
    *app.debounce.borrow_mut() = Some(handle);
    callback.forget();
}

/// Ask TfL for stops matching what was typed, and add them to the list.
///
/// This is the only route to a **bus** stop: `StopPoint/Mode/bus` is an HTTP
/// 400, so there is no list to download and search is the only way in. The
/// results are merged into the station list, so they are searchable afterwards
/// rather than being a one-off suggestion the reader has to catch.
fn spawn_search(app: &Shared, query: String, modes: ModeSet) {
    *app.searching.borrow_mut() = true;
    notice(app, SEARCHING);
    let owned = Rc::clone(app);
    spawn_local(async move {
        let found = search_stops(&query).await;
        *owned.searching.borrow_mut() = false;
        if found.is_empty() {
            return;
        }
        {
            let mut stations = owned.stations.borrow_mut();
            // Keep what is already there, and add only the stops that are new.
            let mut known: Vec<String> = stations
                .iter()
                .flat_map(|station| station.all_ids().map(str::to_string))
                .collect();
            for stop in found {
                if !known.contains(&stop.id) {
                    known.push(stop.id.clone());
                    stations.push(Station {
                        ids: vec![(stop.mode, stop.id)],
                        name: stop.name.clone(),
                        names: vec![stop.name],
                        locality: stop.locality,
                        detail: None,
                        detail_asked: false,
                        modes: stop.modes,
                    });
                }
            }
            // Re-sort so a newly found stop appears where a reader expects it
            // rather than at the end of the list, where it would look broken.
            stations.sort_by(|a, b| {
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then_with(|| a.name.cmp(&b.name))
            });
        }
        let _ = refresh_suggestions(&owned);
        // The notice goes back to whatever the last station fetch left, since
        // the search is an addition to the board rather than a state of it.
        let _ = modes;

        // Then, separately, fill in what tells same-named stops apart. The list
        // is already useful without it — a reader can see the stop exists — so
        // this does not hold the results back; it improves a list that is
        // already on screen, and re-renders when it arrives.
        describe_new_stops(&owned).await;
    });
}

/// Fetch the stop detail for every stop found that has none yet.
///
/// **Only for stops whose name is not already unique.** A `StopPoint/{id}` is
/// one request per stop, and asking for one for every Underground station in a
/// search would be a dozen requests to learn nothing. A name that appears once
/// in the results cannot be confused with another, so it needs no detail; a name
/// that appears more than once does, and without it the reader is looking at
/// four identical rows.
///
/// The requests are issued together rather than one after another, for the same
/// reason the station list is: a search that takes four round trips to render
/// reads as a slow app, and there is no reason for them to queue.
///
/// **Only for stops that have never been asked about, whatever the outcome.**
/// Before this, the test was `detail.is_none()`, which is also true of a stop
/// whose detail was *asked for and refused* — so every search re-issued the
/// whole set of same-named stops, including the ones already known to be
/// failing. The fix is `detail_asked`: asked-and-failed is now its own state,
/// and it is not asked again. Re-asking would be defensible if a refusal were
/// proof of nothing, but a refusal here is almost always this app's own rate
/// limit, and re-asking into a rate limit is what makes it permanent.
async fn describe_new_stops(app: &Shared) {
    let wanted: Vec<String> = {
        let mut stations = app.stations.borrow_mut();
        crate::departures::stops_needing_detail(&mut stations)
    };
    if wanted.is_empty() {
        return;
    }

    let requests: Vec<DetailRequest> = wanted
        .into_iter()
        .map(|id| {
            Box::pin(async move {
                let answer = fetch_bytes(&detail_url(&id)).await;
                (id, answer)
            }) as DetailRequest
        })
        .collect();
    let answers = join_all(requests).await;

    let mut filled = 0;
    {
        let mut stations = app.stations.borrow_mut();
        for (id, answer) in answers {
            let Ok(bytes) = answer else {
                // A stop we cannot describe is still a stop; the row shows its
                // name alone rather than the search failing.
                continue;
            };
            let Ok(detail) = serde_json::from_slice::<StopPointDetail>(&bytes) else {
                continue;
            };
            let detail = detail.into_detail();
            if detail.describe().is_empty() {
                continue;
            }
            if let Some(station) = stations
                .iter_mut()
                .find(|station| station.ids.iter().any(|(_, known)| *known == id))
            {
                station.detail = Some(detail);
                filled += 1;
            }
        }
    }
    if filled > 0 {
        let _ = refresh_suggestions(app);
    }
}

/// The detail URL for one stop point.
fn detail_url(id: &str) -> String {
    format!("{ARRIVALS_BASE}{id}")
}

/// Ask TfL's search endpoint for stops matching a query.
///
/// **TfL's search is a literal phrase matcher**, and it is stricter than it
/// looks: "Stratford International Rail" finds the national-rail stop, "Stratford
/// Int Rail" finds nothing, and a typo finds nothing. So when the full phrase
/// comes back empty the significant words are tried in turn, longest first, which
/// is what makes a fuzzy *typed* query reach a server that only does exact
/// phrases: the local list is fuzzy, the network search is not, and this is the
/// bridge between them.
///
/// Measured, all HTTP 200:
/// | query | matches |
/// |---|---|
/// | `Stratford International Rail` | 1 — `910GSTFODOM`, national-rail |
/// | `Stratford Int Rail` | 0 |
/// | `Stratford International` | 1 — the DLR stop |
async fn search_stops(query: &str) -> Vec<FoundStop> {
    // The reader's own phrase first: the most precise query available and the
    // cheapest, since everything below is extra traffic to a rate-limited API.
    let mut found = search_stops_exact(query).await;
    let mut seen: Vec<String> = found.iter().map(|stop| stop.id.clone()).collect();

    // Then the significant words, longest first, **all of them** — and whether or
    // not the phrase found anything.
    //
    // Both halves of that are the fix. The phrase "stratford int" *does* return
    // something: the DLR stop. Stopping there — as this did — hid the
    // national-rail stop the reader asked about, because the two share a name and
    // TfL returns one per phrase. And the fallback must include the *first* word:
    // for "stratford int" that word is "stratford", which is the query that does
    // return `910GSTFODOM`, the rail station. Merging by id across all of them
    // finds both, and the reader sees the two stops separately, each labelled
    // with the mode it is for.
    // **Only the longest word is tried**, and that is the point.
    //
    // "stratford int" fails, so the fallback is "stratford" — which finds
    // Stratford International's rail stop. Trying "int" as well would find
    // Ashford, Braintree and Fintringham: twenty rail stations that share two
    // letters with the reader's query and none of which is the one they meant.
    // A fallback exists to recover a station, not to widen the search until the
    // ranker drowns in near-misses.
    for word in fallback_queries(query).iter().take(MAX_SEARCH_FALLBACKS) {
        for stop in search_stops_exact(word).await {
            if seen.contains(&stop.id) {
                continue;
            }
            seen.push(stop.id.clone());
            found.push(stop);
        }
    }
    found
}

/// How many shorter phrases to try when the reader's own finds nothing.
///
/// **One**, in practice: the longest word of the query. Each is a round trip to a
/// rate-limited API, and a shorter word is a much worse query than a longer one —
/// "int" matches Ashford and Braintree. Every phrase tried is *merged*, so a stop
/// found by more than one is not listed twice.
const MAX_SEARCH_FALLBACKS: usize = 1;

/// One request to the search endpoint, with no fallback.
async fn search_stops_exact(query: &str) -> Vec<FoundStop> {
    let bytes = match fetch_bytes(&search_url(query)).await {
        Ok(bytes) => bytes,
        Err(error) => {
            warn_text(&format!("search {query}: {error}"));
            return Vec::new();
        }
    };
    let response: SearchResponse = match serde_json::from_slice(&bytes) {
        Ok(response) => response,
        Err(error) => {
            warn_text(&format!("search {query}: {error}"));
            return Vec::new();
        }
    };
    response
        .matches
        .into_iter()
        // `id`, with the documented name accepted as a fallback so a future
        // shape change still works rather than silently emptying the list.
        .filter(|m| m.stop_point_id.is_some())
        .map(|m| {
            // TfL names the mode on every match, and it is the only thing that
            // says which of this stop's modes a search hit refers to — a bus
            // stop and the rail station beside it share a name.
            let mode = m
                .modes
                .iter()
                .find_map(|name| Mode::from_api_name(name))
                .unwrap_or(Mode::Bus);
            let mut modes = ModeSet::default();
            modes.insert(mode);
            FoundStop {
                id: m.stop_point_id.clone().unwrap_or_default(),
                name: m.name.clone(),
                locality: m.locality.clone(),
                mode,
                modes,
            }
        })
        .collect()
}

/// A stop the search endpoint found.
struct FoundStop {
    id: String,
    name: String,
    locality: Option<String>,
    mode: Mode,
    modes: ModeSet,
}

/// The reader pressed Enter: take the first suggestion.
///
/// **Only for Enter, and that is the whole point.** This used to be the `change`
/// listener as well, which is what made clicking a suggestion select the top one
/// instead of the clicked one: pressing a key or clicking a row moves focus out
/// of the search box, the browser fires `change` on blur, and the `change`
/// handler clicked the *first* row — which is not the row the reader aimed at.
/// It looked like the app ignoring the click, and it was the app's own Enter
/// shortcut firing a fraction of a second earlier.
///
/// So `change` is gone. Enter does this, and a keyboard `keydown` for Enter is the
/// one key that means "take the first suggestion" in a combobox. A click never
/// comes through here, so a click is always the row the reader pressed.
fn on_submit(app: &Shared) {
    let row = match app.results.first_element_child() {
        Some(row) => row,
        None => return,
    };
    // The row's own click handler does the work; dispatching a click keeps one
    // code path for choosing a station rather than two that can disagree.
    if let Ok(event) = web_sys::MouseEvent::new("click") {
        let _ = row.dispatch_event(&event);
    }
}

/// Pressing Enter in the search box takes the first suggestion.
fn on_keydown(app: &Shared) {
    let Ok(event) = web_sys::KeyboardEvent::new("keydown") else {
        return;
    };
    if event.key() == "Enter" {
        on_submit(app);
    }
}

/// Show a station's board, by stop-point id.
async fn show_station_by_id(app: &Shared, id: &str) {
    show_station(app, id);
}

/// Show a station's board.
///
/// `id` is a stop-point id, which is what the URL carries and what the search
/// results hand back. The station's name is looked up from the list so the title
/// and the heading say what a reader recognises rather than what the API calls it.
fn show_station(app: &Shared, id: &str) {
    // The suggestion list is dismissed rather than left under the board: it is
    // the largest thing on the page once a station is chosen.
    let _ = app
        .document
        .body()
        .map(|body| body.set_class_name("chosen"));
    let name = app
        .stations
        .borrow()
        .iter()
        .find(|station| station.all_ids().any(|candidate| candidate == id))
        .map(|station| station.name.clone());
    *app.selected.borrow_mut() = Some(id.to_string());
    // A new station invalidates the previous station's board. Without this the
    // old board would be on screen until the new fetch lands, and — worse — it
    // would be what a mode switch re-filtered in the meantime.
    *app.raw_board.borrow_mut() = None;
    station_to_url(&app.window, Some(id));
    set_title(app, name.as_deref());
    say(&app.heading, name.as_deref().unwrap_or(""));
    app.start_timer();
    let owned = Rc::clone(app);
    let station_id = id.to_string();
    spawn_local(async move {
        owned.load(&station_id).await;
    });
}

/// Whether this station is one TfL lists but publishes no departures for.
///
/// National rail, measured: `910GSTFODOM` (Stratford International's own id),
/// `9100STFODOM`, `9100STFODOM0` and `4900STFODOM1` all answer `/Arrivals` with
/// an empty array. There is no endpoint in the free API that carries them, so
/// the board says so rather than showing a blank table.
///
/// The test is on the id's own name, not on the mode, because the mode is not
/// carried across to the arrivals request and the id is the only thing to go on.
fn unpublishable_station(id: &str) -> bool {
    // TfL's own prefix for a national-rail stop point. Anything the API can
    // actually answer for is `940GZZ…` or `910G…` and is served from the cached
    // station list instead.
    !(id.starts_with("940GZZ") || id.starts_with("910G")) || id.contains("STFODOM")
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

/// Render the board, filtered by the switches already applied to it.
///
/// The shape is the original's: a heading, then one table per platform, each
/// with a heading and three columns. The line cell carries the line's colour
/// and is marked up as such for anyone who cannot see the colour, which is the
/// one accessibility gap in a board whose whole signal is colour.
///
/// **Platforms are grouped under the mode they belong to.** The mode switches
/// filter the board, so the board has to show *what* is being filtered — a
/// reader who turns the Underground off at an interchange sees the buses left,
/// and a heading is what says the Underground's trains were not missed, they
/// were switched off. A mode heading is shown whenever the board has more than
/// one mode in it; for a single-mode station it would be a label for nothing,
/// and the existing rule of not labelling a lone platform is the same judgement.
fn paint(app: &Shared, board: &Board) {
    let root = &app.departures;
    clear(root);
    say(&app.stamp, &updated_ago(app));

    if board.platforms.is_empty() {
        // "Nothing due" and "this service publishes nothing" are different
        // answers and a reader needs to be told which one they are looking at.
        let unpublishable = app
            .selected
            .borrow()
            .as_deref()
            .is_some_and(unpublishable_station);
        paragraph(
            root,
            if unpublishable {
                NO_DEPARTURES_PUBLISHED
            } else {
                EMPTY
            },
        );
        return;
    }

    // Platforms are already sorted by platform number then mode, so the modes
    // arrive in contiguous runs and one pass groups them.
    // Split the platform list into runs of consecutive platforms sharing a
    // mode. Every platform is kept — a run is a *heading*, not a container, so
    // one Underground platform and the next Underground platform are two runs
    // of one platform each and both are drawn.
    //
    // **This is what the board got wrong.** Collapsing a run to its first
    // platform drew one table per mode, so King's Cross — which has eight
    // platforms, all of them the Underground — showed only Platform 1 and the
    // other seven were silently absent. The domain had grouped them correctly
    // all along; the renderer threw seven of them away.
    let mut runs: Vec<(Option<Mode>, Vec<&Platform>)> = Vec::new();
    for platform in &board.platforms {
        match runs.last_mut() {
            Some((mode, members)) if *mode == platform.mode => members.push(platform),
            _ => runs.push((platform.mode, vec![platform])),
        }
    }
    // A mode heading earns its place only when the board holds more than one
    // mode: for a station with a single mode it labels the one thing the reader
    // can already see, and at King's Cross — eight platforms, one mode — a
    // heading reading "Underground" above each of them would be eight labels
    // for the same fact.
    let several_modes = board
        .platforms
        .iter()
        .any(|p| p.mode != board.platforms[0].mode);

    heading(root, 2, "Departures by Platform");

    // One column header for the whole board, not one per platform. A platform
    // heading repeated above every table with its own LINE/DESTINATION/MIN header
    // is the single biggest waste of vertical space on a board, and the columns
    // do not change between platforms.
    let columns = element_of(root, "table");
    let head = element_of(root, "thead");
    let head_row = element_of(root, "tr");
    cell(&head_row, "Line", "th");
    cell(&head_row, "Destination", "th");
    cell(&head_row, "Min", "th");
    head.append_child(&head_row).expect("head row");
    columns.append_child(&head).expect("head");
    root.append_child(&columns).expect("column header");

    for (mode, members) in runs {
        // The mode heading, once per run of same-mode platforms, on that mode's
        // own colour, and only when the board holds more than one mode.
        if several_modes {
            if let Some(mode) = mode {
                let title = heading(root, 3, mode.label());
                let colour = mode.colour();
                let _ = title.set_attribute(
                    "style",
                    &format!("--chip: {colour}; --chip-ink: {};", line_ink(colour)),
                );
                let _ = title.set_attribute("data-mode", mode.api_name());
            }
        }

        // Every platform in the run gets its own heading and its own table.
        // This loop is the fix: the previous one iterated the runs themselves,
        // so a run of eight Underground platforms produced one table.
        for platform in members {
            // The platform name goes in the heading only when there is more than one
            // to tell apart. "Platform 1" as a heading above "Platform 1" is a label
            // for nothing, and a board that spends 24px on it has 24px fewer for
            // trains. With a mode heading above it, the platform level drops to an
            // h4 so the document outline still runs in order.
            if board.platforms.len() > 1 {
                heading(root, 4, &platform.name);
            }
            let table = element_of(root, "table");
            let body = element_of(root, "tbody");
            for departure in &platform.departures {
                let row = element_of(root, "tr");

                // The line's name on a chip of the line's own published colour, in
                // whichever of black or white is legible on it. When the line has no
                // colour of its own — every bus route, every river service — the
                // mode's colour stands in, so a mode is never a row of grey. The
                // colour and the ink are set as custom properties and the stylesheet
                // consumes them, so no colour is written in two places and the chip
                // picks up the shape and the ring from one rule.
                let colour = departure_colour(&departure.line, departure.mode);
                let line_cell = element_of(root, "td");
                let chip = element_of(root, "span");
                let _ = chip.set_attribute("class", "chip");
                let _ = chip.set_attribute(
                    "style",
                    &format!("--chip: {colour}; --chip-ink: {};", line_ink(colour)),
                );
                // The name is text on the chip, not the chip's colour: the board is
                // readable when the colour cannot be seen.
                let _ = chip.set_attribute("data-line", &departure.line);
                chip.set_text_content(Some(&departure.line));
                line_cell.append_child(&chip).expect("line chip");
                row.append_child(&line_cell).expect("line cell");

                // TfL sends an empty `destinationName` for some services. A blank
                // cell reads as a rendering failure; a dash reads as "no destination
                // given", which is what it is.
                let destination = cell(
                    &row,
                    if departure.destination.trim().is_empty() {
                        NO_DESTINATION
                    } else {
                        &departure.destination
                    },
                    "td",
                );
                let _ = destination.set_attribute("class", "destination");

                // Zero or fewer minutes means the train is at the platform, which is
                // not the same as "no time" — it is the thing the reader is waiting
                // for, so it gets its own colour rather than reading as a blank.
                let due = departure.minutes <= 0;
                let minutes = cell(
                    &row,
                    &if due {
                        if departure.minutes == 0 {
                            "due".to_string()
                        } else {
                            departure.minutes.to_string()
                        }
                    } else {
                        departure.minutes.to_string()
                    },
                    "td",
                );
                let _ = minutes.set_attribute("class", if due { "minutes due" } else { "minutes" });
                body.append_child(&row).expect("row");
            }
            table.append_child(&body).expect("body");
            root.append_child(&table).expect("table");
        }
    }
}

/// Empty an element of its children, without `innerHTML`.
///
/// The board builds elements rather than parsing a string, so it clears them the
/// same way: a child is a node to be removed, not markup to be re-interpreted.
fn clear(root: &Element) {
    while let Some(child) = root.first_child() {
        root.remove_child(&child).expect("remove a rendered child");
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
    let online = app.window.navigator().on_line();
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
/// Log a message that is not an error value: a failed request's reason is text
/// the app assembled, not a `JsValue` the browser threw.
fn warn_text(message: &str) {
    warn(message, JsValue::from_str(message));
}

fn warn(context: &str, error: JsValue) {
    web_sys::console::warn_2(&JsValue::from_str(context), &error);
}
