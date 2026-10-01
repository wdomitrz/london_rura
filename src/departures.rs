// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! The departures board's logic, with no browser in it.
//!
//! Everything here is a pure function over the JSON TfL hands back: which
//! stop points are stations, how they are ordered, how arrivals are sorted,
//! grouped by platform and coloured by line. It is the whole of the original
//! `app.js` minus the `fetch` calls and the DOM writes, and it is what the unit
//! tests drive with fixture data — no network, no `web-sys`, no browser.
//!
//! The browser layer (`ui`, compiled only for wasm) owns the two things that
//! cannot be pure: the request itself and the DOM it renders into. It feeds
//! this module the deserialised JSON and renders what comes back, so the two
//! cannot drift.

use crate::modes::{Mode, ModeSet};

/// The stop-point list for one mode. `{mode}` is a [`Mode::api_name`].
///
/// This is the big one, and it is why the board fetches per mode. Measured on
/// 2026-09-30, uncompressed:
///
/// | mode | bytes | named stops |
/// |---|---|---|
/// | `tube` | 16,228,451 | 1,751 |
/// | `overground` | 3,110,679 | 547 |
/// | `dlr` | 1,373,727 | 274 |
/// | `elizabeth-line` | 945,904 | 119 |
/// | `cable-car` | 117,455 | 8 |
///
/// `?detail=false` is accepted and does essentially nothing — it saved eleven
/// bytes on the tube response — so the way to make this affordable is to ask for
/// a smaller *slice* of the network, not for less detail.
pub const MODE_STOPS_URL: &str = "https://api.tfl.gov.uk/StopPoint/Mode/{mode}";

/// The arrivals endpoint for one stop point. `{id}` is a stop-point id.
pub const ARRIVALS_URL: &str = "https://api.tfl.gov.uk/StopPoint/{id}/Arrivals";

/// The name-search endpoint, used when a reader types a station or street name.
///
/// `StopPoint/Search/{query}` returns a `matches` array of `{id, name, modes}`,
/// and it is the only way to reach a **bus** stop: `StopPoint/Mode/bus` is an
/// HTTP 400, and a bus stop's id (`490000…`, `490G000…`, `400G…`) is not
/// derivable from anything a reader can see. Measured: `490000173RC` returns 13
/// live arrivals with real route numbers, while a `490G000…` id returns none.
pub const SEARCH_URL: &str = "https://api.tfl.gov.uk/StopPoint/Search/{query}";

/// The base the arrivals URL hangs off, so the id can be substituted without
/// pulling the whole template around.
pub const ARRIVALS_BASE: &str = "https://api.tfl.gov.uk/StopPoint/";

/// The base the search URL hangs off.
pub const SEARCH_BASE: &str = "https://api.tfl.gov.uk/StopPoint/Search/";

/// The modes whose stop points are fetched up front, in fetch order.
///
/// The four rail modes and the cable car. **Not** the bus: `StopPoint/Mode/bus`
/// is an HTTP 400, so there is nothing to fetch, and there are ~19,000 bus stops
/// in London against 270 Underground stations — a picker of bus stops is not a
/// departures board. Buses are reached by search instead, which is how a reader
/// who wants "the 88 from here" would actually look one up.
pub const FETCHED_MODES: &[Mode] = &[
    Mode::Tube,
    Mode::Elizabeth,
    Mode::Overground,
    Mode::Dlr,
    Mode::Cable,
];

/// What a station looks like on the board.
///
/// One row, one place, however many modes serve it. That is the difference from
/// the previous version, which carried a single id per station and so could only
/// ever show one mode's board for an interchange.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Station {
    /// The stop-point id to request arrivals for, per mode.
    ///
    /// One station has several stop-point ids, because TfL models a station as a
    /// set of stop points and each mode hangs off a different one: Paddington is
    /// `940GZZLUPA` for the Bakerloo, the Circle and the Hammersmith & City,
    /// `910GABWDXR` for the Elizabeth line, and a dozen bus stops in the station
    /// forecourt besides. Requesting the Underground's id for the Elizabeth
    /// line returns **zero** arrivals — measured, not assumed — so one id per
    /// station would show an interchange as permanently half-empty.
    pub ids: Vec<(Mode, String)>,
    pub name: String,
    /// Every name TfL gave this place, including the one in `name`.
    ///
    /// The row is *shown* under its place name — "Bank", not "Bank Underground
    /// Station" — because one interchange has one name for a reader. TfL still
    /// calls each half after its own mode, and a reader arriving from a sign, a
    /// search engine or an old bookmark types what TfL calls it. So the full
    /// names are kept here and the search matches against them as well as the
    /// display name, which is the difference between "Brixton Underground
    /// Station" finding Brixton and silently finding nothing.
    pub names: Vec<String>,
    /// Where the station is, when TfL says. Used to tell apart the several bus
    /// stops that share a street name.
    pub locality: Option<String>,
    /// What tells apart two stops that share a name, for the modes where that
    /// happens.
    ///
    /// **This is what stops the search listing "Oxford Circus Station" three
    /// times over.** A bus stop is named for the street, so a single station
    /// forecourt has a dozen rows with one name and nothing to tell them apart:
    /// a reader typing "oxford circus" got four identical lines and no way to
    /// pick the one they were standing at.
    ///
    /// TfL answers a `StopPoint/{id}` for each, and it carries the three things
    /// that actually distinguish them: the stop's **letter** (`RC`, `RG`, `OH` —
    /// the code printed on the flag at the kerb, so it is the one thing a reader
    /// standing there can match against), the **routes** served, and the
    /// **direction** they go in. Any of the three tells two same-named stops
    /// apart, and together they are unambiguous.
    ///
    /// `None` while the detail has not been fetched, and for every non-bus
    /// station — nothing else on this board has two stops with one name.
    pub detail: Option<StopDetail>,
    /// The modes this station is served by.
    pub modes: ModeSet,
}

/// The facts that tell one same-named stop from another.
///
/// Only the modes where a name is not a unique address need this: a bus stop is
/// named for its street, so "Oxford Circus Station" is a dozen different places.
/// Everything a reader needs to tell them apart is here, and each field is
/// optional because TfL does not send all of them for every stop.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StopDetail {
    /// The stop's flag letter, as printed at the kerb: "RC", "RG", "OH".
    pub letter: Option<String>,
    /// The routes that call here, in the order TfL lists them.
    pub routes: Vec<String>,
    /// Where the buses from here go: "Towards Marble Arch".
    pub towards: Option<String>,
}

impl StopDetail {
    /// The line to put under a stop's name when several share one.
    ///
    /// The **letter first**, and that ordering is the whole judgement. A reader
    /// standing at a bus stop is looking at a flag with a letter on it, and it
    /// is the one identifier on that flag that they can match against anything;
    /// the route numbers on it they have to read off, and the destination is
    /// text that repeats across stops. So when the letter is known it leads,
    /// the routes follow because they are what the reader is looking for, and
    /// the destination is the last resort for a stop that has neither.
    ///
    /// An empty string when there is nothing to add — a stop with no letter, no
    /// routes and no direction adds a line that says only that the board knows
    /// nothing, which is worse than saying nothing.
    pub fn describe(&self) -> String {
        let routes = self.routes.join(", ");
        let towards = self.towards.as_deref().unwrap_or_default().trim();
        let letter = self.letter.as_deref().unwrap_or_default().trim();
        match (letter.is_empty(), routes.is_empty(), towards.is_empty()) {
            (false, false, _) => format!("{letter} · {routes}"),
            (false, true, false) => format!("{letter} · {towards}"),
            (false, true, true) => letter.to_string(),
            (true, false, _) => routes,
            (true, true, false) => towards.to_string(),
            (true, true, true) => String::new(),
        }
    }
}

impl Station {
    /// The id to request arrivals for, given which modes are on.
    ///
    /// The enabled mode earliest in [`crate::modes::MODES`], so a reader who has
    /// everything on sees the Underground's board for an interchange and a
    /// reader who has only buses on sees the buses. `None` when no enabled mode
    /// serves this station, which is how the picker filters it out.
    pub fn id_for(&self, enabled: ModeSet) -> Option<&str> {
        self.ids
            .iter()
            .filter(|(mode, _)| enabled.contains(*mode))
            .min_by_key(|(mode, _)| position(*mode))
            .map(|(_, id)| id.as_str())
    }

    /// Every id this station has, for fetching a board that spans modes.
    pub fn all_ids(&self) -> impl Iterator<Item = &str> {
        self.ids.iter().map(|(_, id)| id.as_str())
    }

    /// Whether any enabled mode serves this station.
    pub fn serves(&self, enabled: ModeSet) -> bool {
        self.ids.iter().any(|(mode, _)| enabled.contains(*mode))
    }
}

/// One upcoming arrival, as far as this app cares: which line, where it is
/// going, which platform, how far away it is in seconds, and **which mode it
/// belongs to**.
///
/// The mode is the field this board was missing, and it is the answer to two
/// separate questions. TfL sends it as `modeName` on every arrival, so it costs
/// nothing to read and it is the only reliable way to know whether a row is a
/// bus or a train — the line name cannot say so, because "88" is a bus route and
/// there is no Underground line numbered 88, while a river pier's "RB1" and a
/// bus's "R" would both be ambiguous on a board keyed by name. It is also what
/// the mode toggles filter on, and what picks the chip colour for a row whose
/// line has no colour of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Arrival {
    pub line_name: String,
    pub destination: String,
    /// Seconds until arrival, as TfL reports it. `None` is a train that is
    /// standing at the platform: `timeToStation` is `0` or missing and the
    /// train is counted as arriving now.
    pub time_to_station: Option<i32>,
    /// `platformName`, absent on some lines — hence the "Unknown Platform"
    /// grouping the original app had.
    pub platform: Option<String>,
    /// Which mode this service runs on, as TfL's `modeName`.
    ///
    /// `Option` because the field is genuinely optional on the wire and this
    /// module takes its input as deserialised JSON, where a missing key is a
    /// missing mode. It is **not** defaulted to a mode here: a row with no mode
    /// is a row this app cannot colour or filter honestly, and inventing one
    /// would put it under a switch the reader never touched. [`Departure`] is
    /// where it becomes concrete.
    pub mode: Option<Mode>,
}

/// One row of a departures table: a line, its destination, and the minutes
/// until it arrives — and the mode it runs on, which is what colours it and
/// what the toggles filter on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Departure {
    pub line: String,
    pub destination: String,
    pub minutes: i32,
    /// The mode this service runs on, when TfL named one.
    pub mode: Option<Mode>,
}

/// A platform and the next few trains leaving it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Platform {
    pub name: String,
    /// The mode every departure on this platform runs on, when they agree.
    ///
    /// One platform table can hold more than one mode — an interchange's buses
    /// and its Underground both answer "Platform 1" at some stops — so this is
    /// the mode of the rows in the table, and `None` when they disagree. It is
    /// what lets the board group its tables under a mode heading without
    /// guessing.
    pub mode: Option<Mode>,
    pub departures: Vec<Departure>,
}

/// A station's whole board: every platform that has a train on the way, in
/// platform order, each with at most [`DEPARTURES_PER_PLATFORM`] trains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    pub platforms: Vec<Platform>,
}

impl Board {
    /// True when TfL had nothing at all for this station.
    pub fn is_empty(&self) -> bool {
        self.platforms.is_empty()
    }

    /// This board with every platform whose mode is switched off removed.
    ///
    /// **This is the mode toggles' whole effect on the timetable.** Before
    /// this, a toggle changed which *stations* the picker offered and nothing
    /// about the board in front of the reader: you could turn the Underground
    /// off while looking at Paddington and still get the Underground's trains,
    /// because the board was fetched per stop point and rendered unfiltered.
    ///
    /// Two rules keep the filter honest:
    ///
    /// * a platform is kept when its mode is on, or when the wire did not name
    ///   its mode. A row TfL would not classify is not something to hide: the
    ///   reader's toggles are about *which services they want*, and a service
    ///   this app cannot name is not one they asked to switch off. Hiding it
    ///   would make the board silently incomplete in a way no message explains.
    /// * platforms that lose every departure are dropped, so switching a mode
    ///   off removes its tables rather than leaving empty headings behind.
    pub fn filtered(&self, enabled: ModeSet) -> Board {
        let platforms: Vec<Platform> = self
            .platforms
            .iter()
            .filter(|platform| platform.mode.is_none_or(|mode| enabled.contains(mode)))
            .filter(|platform| !platform.departures.is_empty())
            .cloned()
            .collect();
        Board { platforms }
    }

    /// Whether every platform on this board has a mode the switches name.
    ///
    /// Whether filtering by `enabled` would hide anything. The board says so
    /// rather than showing an empty table, because "you have switched this off"
    /// and "there is nothing running" are different answers.
    pub fn has_hidden_platforms(&self, enabled: ModeSet) -> bool {
        self.platforms
            .iter()
            .any(|platform| platform.mode.is_some_and(|mode| !enabled.contains(mode)))
    }
}

/// How many trains each platform table shows.
///
/// The original app sliced each platform's list at ten. That is a board, not a
/// timetable: ten is enough to see what is coming and few enough to read.
pub const DEPARTURES_PER_PLATFORM: usize = 10;

/// The platform name used for an arrival that does not carry one.
pub const UNKNOWN_PLATFORM: &str = "Unknown Platform";

/// The colour a line with no entry in [`line_colour`] is drawn in.
pub const DEFAULT_LINE_COLOUR: &str = "#666";

/// The line colours, as the original app's table had them.
///
/// These are the published line colours, and they are kept as one table
/// because the board's whole colour key is this: a line name that is not in it
/// falls back to [`DEFAULT_LINE_COLOUR`], which is the one thing the original
/// got wrong (see the note on `line_colour`).
const LINE_COLOURS: &[(&str, &str)] = &[
    ("Bakerloo", "#B36305"),
    ("Central", "#E32017"),
    ("Circle", "#FFD300"),
    ("District", "#00782A"),
    // The wire says "Elizabeth line", and that is the key. It read "Elizabeth"
    // before, which matched nothing TfL sends, so every Elizabeth line arrival
    // fell through to the grey fallback like a bus route did — the mode's own
    // colour is now the fallback, so it would still have been right, but only by
    // accident and only since this change. `every_line_has_its_published_colour`
    // asserts the name TfL actually sends.
    ("Elizabeth line", "#6950a1"),
    ("Hammersmith & City", "#F3A9BB"),
    ("Jubilee", "#6A7278"),
    ("Metropolitan", "#9B0056"),
    ("Northern", "#000000"),
    ("Piccadilly", "#003688"),
    ("Victoria", "#00A0E2"),
    ("Waterloo & City", "#95CDBA"),
];

/// The published colour of a line, or the grey default for a line this table
/// has never heard of.
///
/// The original app wrote `lineColors[dep.lineName] || "#666"`. That is not
/// the same thing: a key that exists but is empty — a `lineName` of `""`,
/// which TfL does send for the terminating services on some interchanges —
/// is falsy in JavaScript, so it fell through to the grey as well. The Rust
/// treats the empty name as a line it has no colour for, which is what the
/// original meant, and everything else the table knows keeps its own colour.
/// The published colour of a line, or the grey default for a line this table
/// has never heard of.
///
/// The original app wrote `lineColors[dep.lineName] || "#666"`. That is not the
/// same thing: a key that exists but is empty — a `lineName` of `""`,
/// which TfL does send for the terminating services on some interchanges —
/// is falsy in JavaScript, so it fell through to the grey as well. The Rust
/// treats the empty name as a line it has no colour for, which is what the
/// original meant, and everything else the table knows keeps its own colour.
///
/// Prefer [`departure_colour`] on a board: this function alone is why every
/// non-Underground row was grey.
pub fn line_colour(line: &str) -> &'static str {
    LINE_COLOURS
        .iter()
        .find(|(name, _)| *name == line)
        .map_or(DEFAULT_LINE_COLOUR, |(_, colour)| colour)
}

/// The colour to draw a departure's chip in, given its line and its mode.
///
/// **This is why every non-Underground row on the board used to be grey.** The
/// chip colour came from [`line_colour`] alone, and that table is keyed by the
/// names of the eleven Underground lines plus the Elizabeth line. Every bus
/// route number, every river service ("RB1"), every cable car leg and every
/// cycle hire is a `lineName` with no entry there, so they all fell through to
/// [`DEFAULT_LINE_COLOUR`] — one grey, for every mode, on a board whose whole
/// visual language is "what colour is this service".
///
/// A reader looking at a board of buses could not tell one mode from another,
/// because colour was the only thing that could have told them.
///
/// So the mode's own published colour is the fallback, in this order:
///
/// 1. the **line's** colour when the line has one — Underground and Elizabeth
///    line services keep their published line colour, which is what a reader
///    knows them by;
/// 2. the **mode's** colour otherwise, from [`Mode::colour`] — a bus is red
///    because TfL paints buses red, the Overground orange because TfL paints
///    the Overground orange, a river pier blue for the same reason. Every mode
///    colour clears 4.5:1 against its own chip ink, which
///    `every_mode_is_nameable_and_coloured` asserts.
///
/// So no row is grey except one whose mode is genuinely unknown, which the wire
/// says with `modeName: null` — the honest answer for a service this app does
/// not recognise.
pub fn departure_colour(line: &str, mode: Option<Mode>) -> &'static str {
    let colour = line_colour(line);
    if colour == DEFAULT_LINE_COLOUR {
        mode.map_or(colour, Mode::colour)
    } else {
        colour
    }
}

/// The ink to draw a line's name in, on a chip of that line's colour.
///
/// This is the one bit of presentation maths in the domain, and it is here
/// rather than in a stylesheet because it is arithmetic, it is per line, and it
/// is the part that has to be right for every colour on the board.
///
/// **Why it is needed.** The board's whole visual signal is the published line
/// colours, and those colours are not all legible on any single background.
/// Circle is `#FFD300` on white at 1.44:1 and Northern is `#000000` on white
/// at 21:1, but Victoria is `#00A0E2` at 2.95:1 and the four pale lines sit
/// under 2:1. Flipping to a dark background does not fix it either: District
/// drops to 3.49:1 and Metropolitan to 2.37:1. **No background works for all
/// twelve**, which is why the original's plain black-on-white table had three
/// lines nobody could read.
///
/// So the line's name is drawn on a chip of its own colour, in whichever of
/// black or white is legible on it, and the two are chosen together. Every
/// line then clears 4.5:1 — the WCAG AA threshold for text — and the worst,
/// Bakerloo, still manages 4.70:1. The colour is still the first thing the eye
/// goes to; the text is the thing that survives when the colour cannot be
/// seen, which also makes the board readable for a reader who is colour blind
/// or on a dimmed screen.
pub fn line_ink(colour: &str) -> &'static str {
    if contrast_ratio(colour, DARK_INK) >= contrast_ratio(colour, LIGHT_INK) {
        DARK_INK
    } else {
        LIGHT_INK
    }
}

/// The dark page background, as the shell sets it.
pub const DARK_PAGE: &str = "#0b0b0c";

/// The light page background, as the shell sets it.
pub const LIGHT_PAGE: &str = "#f6f7f9";

/// Black, named for what it is on a chip.
pub const DARK_INK: &str = "#000000";

/// White, named for what it is on a chip.
pub const LIGHT_INK: &str = "#ffffff";

/// The WCAG 2.1 relative luminance of an sRGB hex colour.
///
/// The coefficients and the 0.03928 threshold are the ones WCAG defines; the
/// linearisation is what makes the ratio below match what a reader sees rather
/// than what the hex codes suggest.
pub fn relative_luminance(colour: &str) -> f64 {
    let hex = colour.trim().trim_start_matches('#');
    let Some(rgb) = (0..3).map(|i| channel(hex, i)).collect::<Option<Vec<u8>>>() else {
        // An unparsable colour is treated as black: the conservative end, and
        // the one that keeps the ratio finite.
        return 0.0;
    };
    let [r, g, b] = rgb[..] else {
        return 0.0;
    };
    0.2126 * linearise(r) + 0.7152 * linearise(g) + 0.0722 * linearise(b)
}

/// One colour channel out of a hex string, 0–255.
///
/// Accepts both the six-digit form and the three-digit shorthand, because a
/// stylesheet will happily hand over `#fff` and a colour function that quietly
/// reads that as black puts white text on white.
fn channel(hex: &str, index: usize) -> Option<u8> {
    let width = if hex.len() == 3 { 1 } else { 2 };
    let start = index * width;
    let pair = hex.get(start..start.checked_add(width)?)?;
    let value = u8::from_str_radix(pair, 16).ok()?;
    Some(if width == 1 { value * 17 } else { value })
}

/// One channel's contribution to relative luminance.
fn linearise(value: u8) -> f64 {
    let channel = f64::from(value) / 255.0;
    if channel <= 0.03928 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

/// The WCAG contrast ratio between two sRGB hex colours, from 1.0 to 21.0.
pub fn contrast_ratio(a: &str, b: &str) -> f64 {
    let (one, other) = (relative_luminance(a), relative_luminance(b));
    let lighter = one.max(other);
    let darker = one.min(other);
    (lighter + 0.05) / (darker + 0.05)
}

/// Whether a stop point is worth offering a reader.
///
/// A name is the only requirement now, and dropping the id check is the whole
/// point of this rewrite.
///
/// The previous filter also required the id to start with `940GZZLU` or
/// `940GZZCR`, which is a filter for *the Underground* wearing the costume of a
/// filter for *a station*. It discarded the Elizabeth line (`910G…`), the
/// Overground (`910G…`), the DLR (`940GZZDL…`) and the cable car — silently, with
/// no error anywhere — while the app's own request asked for
/// `tube,elizabeth-line` and threw the Elizabeth line stops away.
///
/// `commonName: null` is the check that still earns its place. TfL sends it for
/// the unnamed legs of an interchange, and those legs carry their station's own
/// id shape, so without this the picker fills with rows that have no name.
pub fn is_station(common_name: Option<&str>) -> bool {
    common_name.is_some_and(|name| !name.is_empty())
}

/// Whether an id is one the arrivals endpoint will answer for.
///
/// Measured, and it is not the whole namespace. A stop point like Amersham
/// appears in the mode response twice: as `940GZZLUAMS`, which returns four live
/// Metropolitan trains, and as `0400ZZLUAMS0`, which returns **none** — an empty
/// array, HTTP 200, no error. The outer-zone and stop-number-suffixed ids
/// (`0400ZZ…`, `2100ZZ…`, `4900ZZ…`, anything ending in a digit after position
/// nine) sit in the same responses and behave the same way.
///
/// So an id appearing in a stop-point list is no evidence that it is worth
/// requesting, and taking the first one a response happens to list produces a
/// station that looks listed and shows no trains at all. That failure mode is
/// what this predicate exists to prevent, and it is a rule learned by measuring
/// the endpoint rather than read off a document.
pub fn is_requestable(id: &str) -> bool {
    id.starts_with("940GZZ") || id.starts_with("910G")
}

/// The modes TfL says a stop point is served by, mapped onto this app's set.
///
/// A mode this app does not know is ignored rather than fatal: TfL's mode list
/// grows, and an unknown entry should narrow nothing rather than break the list.
pub fn modes_of(names: &[String]) -> ModeSet {
    let mut set = ModeSet::empty();
    for name in names {
        if let Some(mode) = Mode::from_api_name(name) {
            set.insert(mode);
        }
    }
    set
}

/// The stations to offer, in the order to offer them.
///
/// TfL's own order is the order it stores them in, which is neither the
/// alphabet nor anything a reader would choose. Sorting by `commonName` is what
/// the original did. The comparison is not `localeCompare`, which the browser
/// implements with the reader's locale and which the unit tests could not
/// pin down: it is a case-insensitive comparison with ties broken on the exact
/// code points, so "Acton Town" and "Acton Warren" keep a stable order and the
/// result does not depend on the machine the tests run on.
/// A stop point exactly as TfL sends it, from one mode's response.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct StopPoint {
    /// The field is `id`, not `stopPointId`.
    ///
    /// The stop-point list and the arrivals list disagree about this, and that
    /// is the whole trap. `StopPoint/…` sends `"id"`; a `TflArrival` sends
    /// `"stationName"`, `"platformName"` and no id at all; and the *older* TfL
    /// Unified API documentation shows a `stopPointId` that the live endpoint
    /// does not send — of 1,858 stop points, **zero** carry it. Renaming this to
    /// `stopPointId` leaves every id an empty string and the picker permanently
    /// empty, reporting no error anywhere.
    #[serde(rename = "id", default)]
    pub id: String,
    #[serde(rename = "commonName", default)]
    pub name: Option<String>,
    /// The modes TfL says this stop point is served by. Empty on some responses,
    /// in which case the mode that produced the response is used.
    #[serde(default)]
    pub modes: Vec<String>,
    /// The neighbourhood, when TfL knows one.
    #[serde(rename = "localityName", default)]
    pub locality: Option<String>,
}

/// The stations to offer, in the order to offer them, merged across modes.
///
/// TfL's own order is the order it stores them in, which is neither the alphabet
/// nor anything a reader would choose. Sorting by name is what the original did.
/// The comparison is not `localeCompare`, which the browser implements with the
/// reader's locale and which the unit tests could not pin down: it is a
/// case-insensitive comparison with ties broken on the exact code points, so
/// "Acton Town" and "Acton Warren" keep a stable order and the result does not
/// depend on the machine the tests run on.
///
/// **Merging by name is the whole trick.** A station comes back once per mode
/// that serves it, under a *different* stop-point id each time, and a reader
/// thinks of Paddington as one place. The stop points for a name are folded
/// together into one [`Station`] holding an id per mode, so the board can show
/// the Underground's trains *and* the Elizabeth line's from one pick — which it
/// could not before, because one id per station meant one mode's board and the
/// rest silently returned nothing.
///
/// Bus, cycle and river stops are keyed by their own id instead of their name. A
/// bus stop is named for the street it stands on, so "Oxford Circus Station" is
/// a dozen different rows in any real list; folding them together would invent a
/// station that does not exist, and keeping them apart is what lets a reader
/// choose the one they are standing at.
pub fn station_index(per_mode: Vec<(Mode, Vec<StopPoint>)>) -> Vec<Station> {
    let mut merged: Vec<Station> = Vec::new();
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for (mode, points) in per_mode {
        for point in points {
            let Some(name) = point.name.clone().filter(|name| !name.is_empty()) else {
                continue;
            };
            // A stop point the arrivals endpoint cannot answer for is only worth
            // keeping if nothing requestable exists for this row. Amersham
            // arrives as both `940GZZLUAMS` and `0400ZZLUAMS0`; keeping the
            // second would let `id_for` pick it and show no trains.
            let requestable = is_requestable(&point.id);
            if !requestable && !matches!(mode, Mode::Bus | Mode::Cycle | Mode::River) {
                continue;
            }

            // The mode the response came from is authoritative; TfL's own
            // `modes` array only widens it, because it is empty on some
            // responses and a stop point reachable by two fetches is reachable
            // by both.
            let mut modes = modes_of(&point.modes);
            modes.insert(mode);

            let key = match mode {
                Mode::Bus | Mode::Cycle | Mode::River => {
                    format!("{}:{}", mode.api_name(), point.id)
                }
                // Rail modes merge on the bare place name, not the raw
                // `commonName`: that is what folds "Bank Underground Station"
                // and "Bank DLR Station" into one row. Bus, cycle and river
                // stops keep their own id, because a bus stop is named for the
                // street and several share it — folding those together would
                // invent a stop that does not exist.
                _ => interchange_key(&name),
            };
            match index.get(&key).copied() {
                Some(at) => {
                    let station = &mut merged[at];
                    // A requestable id replaces a non-requestable one for the
                    // same station, rather than sitting beside it.
                    match station
                        .ids
                        .iter()
                        .position(|(m, id)| *m == mode && requestable && !is_requestable(id))
                    {
                        Some(slot) => station.ids[slot] = (mode, point.id.clone()),
                        None => {
                            if !station.ids.iter().any(|(_, id)| *id == point.id) {
                                station.ids.push((mode, point.id.clone()));
                            }
                        }
                    }
                    station.modes = station.modes.union(modes);
                    if !station.names.contains(&name) {
                        station.names.push(name);
                    }
                    if station.locality.is_none() {
                        station.locality = point.locality.clone();
                    }
                }
                None => {
                    // The row is *named* after the interchange, not after
                    // whichever mode happened to be fetched first: once Bank's
                    // Underground and DLR stops are one row, calling it "Bank
                    // Underground Station" would name one half of it. The
                    // bare place name is what a reader knows it by, and the
                    // mode chips beside it say which halves it has. Bus, cycle
                    // and river stops keep the street name TfL gave them.
                    let display = match mode {
                        Mode::Bus | Mode::Cycle | Mode::River => name.clone(),
                        _ => place_name(&name),
                    };
                    merged.push(Station {
                        ids: vec![(mode, point.id.clone())],
                        name: display,
                        names: vec![name],
                        locality: point.locality.clone(),
                        detail: None,
                        modes,
                    });
                    index.insert(key, merged.len() - 1);
                }
            }
        }
    }

    for station in &mut merged {
        // `id_for` takes the earliest enabled mode, so the order has to be the
        // mode table's and not the fetch order, or the same station would show a
        // different board depending on which response landed first.
        station.ids.sort_by_key(|(mode, _)| position(*mode));
    }
    merged.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.locality.cmp(&b.locality))
    });
    merged
}

/// The place a stop point names, with the mode's word removed.
///
/// **This is what collapses "Bank Underground Station" and "Bank DLR Station"
/// into one Bank.** TfL names a rail stop point after the mode as well as the
/// place — `Bank Underground Station`, `Bank DLR Station`, `Whitechapel Rail
/// Station` beside `Whitechapel Underground Station` — so merging on the raw
/// `commonName`, as this index used to, never folded an interchange together. A
/// reader searching "bank" got three rows for the one station on Bank: the
/// Underground, the DLR, and the National Rail beside it. That is the duplicate
/// in the search results, and it is one interchange, not three places.
///
/// TfL's tails are the mode's own name plus "station", in a fixed vocabulary.
/// Removing that tail turns each variant back into the bare place name, and
/// every variant of one interchange then collapses to the same key.
///
/// **Casing is preserved**, because this is also the name shown to the reader
/// and "King's Cross St. Pancras" must not become "king's cross st. pancras".
/// Use [`interchange_key`] for the matching key and this for display.
///
/// Deliberately conservative: only these exact tails are removed, and only a
/// trailing one, so a place genuinely named after a mode survives and only
/// something left behind can be the result.
pub fn place_name(name: &str) -> String {
    let tidy = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut words: Vec<&str> = tidy.split(' ').collect();
    let lowered: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    for suffix in MODE_STATION_SUFFIXES {
        let tail: Vec<&str> = suffix.trim_start().split(' ').collect();
        if words.len() > tail.len()
            && lowered[lowered.len() - tail.len()..]
                .iter()
                .map(String::as_str)
                .eq(tail.iter().copied())
        {
            words.truncate(words.len() - tail.len());
            return words.join(" ");
        }
    }
    tidy
}

/// The matching key for [`place_name`]: the same string, case-folded.
pub fn interchange_key(name: &str) -> String {
    place_name(name).to_lowercase()
}

/// The tails TfL puts after a rail station's place name, as whole words.
///
/// **Matched word by word, case-insensitively, and matched longest-first.** All
/// three parts matter. Word-wise, because cutting on a byte offset would split
/// a multi-byte name; case-insensitively, because these are lower case and TfL
/// capitalises them, so a `strip_suffix` on the raw name matches nothing at all
/// and returns the name unchanged — which looks exactly like there being no
/// rule; longest-first, because "national rail station" has to be tried before
/// "rail station" or a national-rail stop point keeps the word "National" and is
/// keyed on "London Liverpool National".
///
/// A tail never consumes the whole name: `words.len() > tail.len()` is what
/// stops "Station" becoming an empty station.
const MODE_STATION_SUFFIXES: &[&str] = &[
    "national rail station",
    "elizabeth line station",
    "international station",
    "underground station",
    "overground station",
    "dlr station",
    "rail station",
    "underground",
    "overground",
    "dlr",
    "station",
];

/// A mode's position in [`crate::modes::MODES`].
fn position(mode: Mode) -> u8 {
    crate::modes::MODES
        .iter()
        .position(|candidate| *candidate == mode)
        .unwrap_or(usize::MAX) as u8
}

/// How a reader types a station name, and what they get back.
///
/// A `<select>` of 270 alphabetical stations is a board nobody scrolls, and it
/// is unusable for the two modes whose stops are not in the list at all. So the
/// picker is a text field with a result list under it, and this is the matching.
///
/// Matching is a prefix match on any word in the name, so "cross" finds "King's
/// Cross St. Pancras" and "ark" does not find "Barking" — a reader remembers a
/// station by one of its words, not by where its first letters fall. Ranking
/// puts an exact match first and a prefix match second, because a reader who has
/// typed a station's name in full wants it offered first, not fourth.
///
/// This is where the original was worst: it had no search at all, and a reader
/// who wanted Lewisham scrolled a list of 270.
pub struct Search<'a> {
    stations: &'a [Station],
}

impl<'a> Search<'a> {
    /// A search over a station list.
    pub fn new(stations: &'a [Station]) -> Self {
        Self { stations }
    }

    /// The stations matching `query`, best first, capped at [`SEARCH_LIMIT`].
    ///
    /// `enabled` filters as well as ranks, so a reader with only the
    /// Underground on is never offered a bus stop they cannot board.
    pub fn query(&self, query: &str, enabled: ModeSet) -> Vec<&'a Station> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(u8, &Station)> = self
            .stations
            .iter()
            .filter(|station| station.serves(enabled))
            .filter_map(|station| best_score(station, &needle).map(|s| (s, station)))
            .collect();
        // `sort_by` is stable, so equal-scoring stations keep the alphabetical
        // order the index gave them and the list never reshuffles while typing.
        // Reverse order: highest score first.
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        scored
            .into_iter()
            .take(SEARCH_LIMIT)
            .map(|(_, station)| station)
            .collect()
    }
}

/// How many suggestions a search offers.
///
/// Twelve fits on a phone above the keyboard without the list pushing the board
/// off the screen, and it is more than enough to see the right name.
pub const SEARCH_LIMIT: usize = 12;

/// The best rank any of a station's names gives the query.
///
/// A station is matched on the name it is **shown** under and on every name TfL
/// gave it, and the best of those wins. Both matter, and for opposite reasons:
///
/// * the display name is the bare place — "Brixton", "Bank" — which is what a
///   reader types when they have a place in mind, and matching it exactly is
///   what puts the right row first rather than fourth;
/// * the full names are the mode-stamped ones — "Brixton Underground
///   Station" — which is what a sign, a search engine and an old bookmark hand
///   the reader. Those are *longer* than the display name, so no prefix or
///   subsequence rule can bridge them: without this, typing what the board
///   itself used to say would find nothing at all.
///
/// The display name is scored first so that a query matching both exactly —
/// "Brixton" is both the display name and a prefix of the full one — is decided
/// by the name the reader will read.
fn best_score(station: &Station, needle: &str) -> Option<u8> {
    let display = score(&station.name.to_lowercase(), needle);
    let named = station
        .names
        .iter()
        .filter_map(|name| score(&name.to_lowercase(), needle))
        .max();
    match (display, named) {
        (Some(display), Some(named)) => Some(display.max(named)),
        (Some(display), None) => Some(display),
        (None, Some(named)) => Some(named),
        (None, None) => None,
    }
}

/// How well a station name matches a query: higher is better, `None` is no
/// match at all.
///
/// Four ranks, best first. The first three are what a matching search box does;
/// the fourth is the one a reader actually wants and no substring search has.
///
/// * `4` — the name **is** the query. You know exactly where you are going.
/// * `3` — the name **starts with** the query. "bri" is clearly Brixton.
/// * `2` — the query starts a **word** inside the name. "cross" is King's Cross.
/// * `1` — the query's letters appear **in order, anywhere**. "kxs" is King's
///   Cross, "sm" is Stratford **I**nter**n**ational… and, more to the point,
///   "stfint" finds Stratford International where a substring search finds
///   nothing at all.
///
/// That last rank is what makes this fuzzy rather than substring. A reader typing
/// on a phone mis-keys constantly, and a search that insists on exact substrings
/// returns nothing for "stretford" when the station is "Stratford". Matching
/// **characters in order** tolerates a mistyped letter and still ranks the
/// obvious answer first.
///
/// The cost is false positives — "barking" matches "Barking" and a handful of
/// others — which is why the rank is lowest and why the list is capped. A reader
/// who typed a query and got five suggestions, the right one first, has been
/// helped; a reader who got nothing has not.
fn score(name: &str, needle: &str) -> Option<u8> {
    if name == needle {
        return Some(4);
    }
    if name.starts_with(needle) {
        return Some(3);
    }
    // Word-initial: the query opens a word, either at the start or after a
    // separator. Apostrophes and hyphens count, so "kings" finds "King's Cross".
    let boundaries = std::iter::once(0).chain(
        name.char_indices()
            .filter(|(_, c)| *c == ' ' || *c == '\'' || *c == '-')
            .map(|(at, c)| at + c.len_utf8()),
    );
    if boundaries.clone().any(|at| name[at..].starts_with(needle)) {
        return Some(2);
    }
    subsequence(name, needle).then_some(1)
}

/// Whether every character of `needle` appears in `name`, in order.
///
/// The cheap version, and deliberately so: it answers "could this be it" and lets
/// the rank order decide, rather than scoring how *close* a match is. A full
/// edit-distance would rank better, at the cost of scoring thousands of stations
/// on every keystroke — and the thing this has to be is fast, because it runs
/// while someone is typing.
///
/// Not anchored at either end, and not required to start at a word boundary: that
/// is what lets "stfint" reach Stratford International.
fn subsequence(name: &str, needle: &str) -> bool {
    let mut rest = name;
    for wanted in needle.chars() {
        match rest.find(wanted) {
            Some(at) => rest = &rest[at + wanted.len_utf8()..],
            None => return false,
        }
    }
    true
}

/// The whole departures board for one station's arrivals.
///
/// **Grouping is by mode as well as by platform.** This is what lets a mode
/// toggle filter a board rather than blank it. TfL answers one platform for a
/// bus and another for a train and they share a name more often than you would
/// think — "Platform 1" on both — so a board that grouped by platform name
/// alone would interleave a bus and a train in one table and give a reader no
/// way to turn one of them off. Keying each platform by its mode *and* its name
/// keeps them apart, and lets the board carry the mode out to the renderer,
/// which puts a mode heading over each block and colours its chips.
pub fn board(arrivals: Vec<Arrival>) -> Board {
    let mut sorted = arrivals;
    sorted.sort_by_key(|arrival| arrival.time_to_station.unwrap_or(0));

    let mut grouped: Vec<Platform> = Vec::new();
    for arrival in &sorted {
        let name = arrival
            .platform
            .clone()
            .filter(|platform| !platform.is_empty())
            .unwrap_or_else(|| UNKNOWN_PLATFORM.to_string());
        match grouped
            .iter_mut()
            .find(|platform| platform.name == name && platform.mode == arrival.mode)
        {
            Some(platform) => platform.departures.push(departure_of(arrival)),
            None => grouped.push(Platform {
                name,
                mode: arrival.mode,
                departures: vec![departure_of(arrival)],
            }),
        }
    }

    // Platforms sort by the number in the name, then by mode so two stops'
    // "Platform 1"s keep a stable order between refreshes. The mode ordering is
    // the mode table's, so the Underground's platform leads the interchange's
    // bus stand rather than the two swapping each tick.
    grouped.sort_by(|a, b| {
        platform_number(&a.name)
            .cmp(&platform_number(&b.name))
            .then_with(|| mode_position(a.mode).cmp(&mode_position(b.mode)))
    });
    for platform in &mut grouped {
        platform.departures.truncate(DEPARTURES_PER_PLATFORM);
    }
    Board { platforms: grouped }
}

/// Where a mode sits in [`crate::modes::MODES`], with an unknown mode last.
///
/// `None` sorts after every known mode: a service TfL did not name is not
/// something to put above the Underground's trains.
fn mode_position(mode: Option<Mode>) -> usize {
    mode.map_or(usize::MAX, |mode| position(mode) as usize)
}

/// One arrival as a table row.
fn departure_of(arrival: &Arrival) -> Departure {
    Departure {
        line: arrival.line_name.clone(),
        destination: arrival.destination.clone(),
        minutes: minutes_until(arrival),
        mode: arrival.mode,
    }
}

/// The minutes until a train arrives, rounded the way the board rounds.
///
/// The original switched from `Math.round` to `Math.floor` in commit 36f71eb,
/// "floor to match the stations": a train 90 seconds out reads "1", not "2",
/// because a departure board that says two minutes for a train in ninety
/// seconds is a board that overstates how long you have. `floor` of a
/// non-negative time is truncation, and TfL's negative `timeToStation` — the
/// trains counted as gone but still listed — still floors towards minus
/// infinity, so a train already departed reads a negative minute count, exactly
/// as the original did.
pub fn minutes_until(arrival: &Arrival) -> i32 {
    arrival_minutes(arrival.time_to_station.unwrap_or(0))
}

/// The minutes until a train arrives, from a raw `timeToStation` in seconds.
///
/// Floors rather than rounds, for the reason above, and floors the way
/// `Math.floor` does rather than the way Rust's `/` does. That difference is
/// not academic: `-30 / 60` is `0` in Rust and `Math.floor(-30 / 60)` is `-1`
/// in JavaScript, so a departed train would read "0 min" where the original
/// read "-1 min". `div_euclid` is floor division for a positive divisor, which
/// is exactly `Math.floor` over the integers, and it is the one operator here
/// that gets this right. A missing time is handled by the caller, as zero.
pub fn arrival_minutes(time_to_station: i32) -> i32 {
    time_to_station.div_euclid(60)
}

/// The number in a platform name, for ordering platforms.
///
/// The original read the first run of digits in the name and sorted on it,
/// which is what makes "Platform 10" come after "Platform 9" rather than
/// before it. A name with no digits — "Unknown Platform" — sorts as 0, so it
/// leads, and two platforms that both read 0 keep the order they arrived in,
/// because this sort is stable.
pub fn platform_number(name: &str) -> i32 {
    first_number(name).unwrap_or(0) as i32
}

/// The first run of digits in a string, read as an integer.
///
/// `None` when there are none. This is the Rust spelling of the original's
/// `parseInt(name.match(/\d+/))`: the first group of digits in the name, not
/// the whole name, so "Platform 10" reads 10 and "Platform 2 / 3" reads 2.
/// The digits are collected as an `i64` so a long run cannot overflow and
/// panic — `parse` would wrap a huge platform number into a negative one and
/// send the table to the wrong end of the screen.
fn first_number(text: &str) -> Option<i64> {
    let digits: String = text
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// The shortest a word may be and still be worth sending to the search API.
///
/// Three characters. Two is where "int" lives, and a two-character query to a
/// rate-limited API is a request that returns most of the network.
pub const MIN_PHRASE_CHARS: usize = 3;

/// The shorter phrases to try when the reader's own query finds nothing.
///
/// Longest word first, and only words long enough to be a phrase worth sending.
/// The ordering is the whole point: for "stratford int" the fallbacks are
/// "stratford" and then nothing, because "int" matches Ashford, Braintree and
/// Fintringham — twenty rail stations that share two letters with the query and
/// none of which is the one the reader meant.
///
/// This lives in the domain rather than beside the fetch so it can be tested on
/// the host; the browser is a poor place to discover that a fallback query is
/// two letters long.
pub fn fallback_queries(query: &str) -> Vec<String> {
    let mut words: Vec<String> = query
        .split_whitespace()
        .filter(|word| word.chars().count() >= MIN_PHRASE_CHARS)
        .map(str::to_string)
        .collect();
    // Longest first, then alphabetically, so the order does not depend on the
    // order the reader happened to type in.
    words.sort_by(|a, b| {
        b.chars()
            .count()
            .cmp(&a.chars().count())
            .then_with(|| a.cmp(b))
    });
    words.dedup();
    words
}

/// One match from `StopPoint/Search`.
///
/// **The field is `id`, not `stopPointId`.** Same trap as the stop-point list, in
/// a new place, with the same quiet consequence: `StopPoint/Search` sends
/// `"id"`, so deserialising the documented name left every match without an id,
/// every match was filtered out, and **search found nothing at all** — with
/// every HTTP status at 200. A search box that silently returns nothing is
/// indistinguishable from one that genuinely has no results.
///
/// The type lives here rather than in `ui.rs` so this test runs on the host, not
/// only on wasm: the bug it guards against is invisible from the browser.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct SearchMatch {
    #[serde(rename = "id", default)]
    pub stop_point_id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(rename = "localityName", default)]
    pub locality: Option<String>,
    #[serde(default)]
    pub modes: Vec<String>,
}

/// A `StopPoint/{id}` detail, as TfL sends it.
///
/// Only the three fields that distinguish same-named stops are read, and the
/// rest of a stop point — which is a large object, mostly identifiers this app
/// has no use for — is ignored. `serde` drops unknown keys by default, so this
/// costs nothing to leave unread.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct StopPointDetail {
    #[serde(rename = "stopLetter", default)]
    pub stop_letter: Option<String>,
    #[serde(default)]
    pub lines: Vec<StopLine>,
    /// The stop's other identifiers, from which the direction is read.
    ///
    /// The direction is not a field of its own: TfL puts it in
    /// `additionalProperties` under the key "Towards" (and a compass point
    /// beside it). It is the only thing there that a reader recognises, and the
    /// value already reads "Marble Arch Or Great Portland Street".
    #[serde(rename = "additionalProperties", default)]
    pub additional: Vec<StopProperty>,
}

/// One of a stop's route identifiers.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct StopLine {
    #[serde(default)]
    pub name: String,
}

/// One `additionalProperties` entry.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct StopProperty {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: String,
}

impl StopPointDetail {
    /// The facts worth showing, from whatever of them TfL sent.
    ///
    /// Routes are de-duplicated and put in the order TfL listed them: a stop
    /// served by both the 22 and the N22 twice would show the number twice,
    /// which reads as a different route rather than the same one twice.
    pub fn into_detail(self) -> StopDetail {
        let mut routes: Vec<String> = Vec::new();
        for line in self.lines {
            let name = line.name.trim().to_string();
            if !name.is_empty() && !routes.contains(&name) {
                routes.push(name);
            }
        }
        let towards = self
            .additional
            .into_iter()
            .find(|property| property.key.eq_ignore_ascii_case("Towards"))
            .map(|property| property.value.trim().to_string())
            .filter(|value| !value.is_empty());
        StopDetail {
            letter: self
                .stop_letter
                .map(|letter| letter.trim().to_string())
                .filter(|letter| !letter.is_empty()),
            routes,
            towards,
        }
    }
}

/// The `StopPoint/Search` response, as TfL sends it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct SearchResponse {
    #[serde(default)]
    pub matches: Vec<SearchMatch>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stop point, as the list would supply one.
    /// A stop point, as a mode's response would supply one.
    fn stop(id: &str, name: Option<&str>) -> StopPoint {
        StopPoint {
            id: id.to_string(),
            name: name.map(str::to_string),
            modes: Vec::new(),
            locality: None,
        }
    }

    /// A stop point served by named modes.
    fn stop_for(id: &str, name: &str, modes: &[&str]) -> StopPoint {
        StopPoint {
            id: id.to_string(),
            name: Some(name.to_string()),
            modes: modes.iter().map(|m| (*m).to_string()).collect(),
            locality: None,
        }
    }

    /// An arrival, with the fields the board reads.
    fn arrival(line: &str, destination: &str, platform: Option<&str>, seconds: i32) -> Arrival {
        Arrival {
            line_name: line.to_string(),
            destination: destination.to_string(),
            time_to_station: Some(seconds),
            platform: platform.map(str::to_string),
            mode: None,
        }
    }

    /// An arrival on a named mode, which is what every real one carries.
    fn arrival_on(
        line: &str,
        destination: &str,
        platform: Option<&str>,
        seconds: i32,
        mode: Mode,
    ) -> Arrival {
        Arrival {
            mode: Some(mode),
            ..arrival(line, destination, platform, seconds)
        }
    }

    /// The minutes a platform's table shows, in order.
    fn minutes_of(platform: &Platform) -> Vec<i32> {
        platform.departures.iter().map(|d| d.minutes).collect()
    }

    // ------------------------------------------------------- stations and modes

    /// Every mode's api name must be a mode the app knows.
    ///
    /// This is the check that would have caught the Elizabeth line going missing,
    /// and it is worth saying why it is easy to get wrong: `Mode` is an enum, so
    /// adding a mode to the app and forgetting to add it here compiles, runs, and
    /// silently narrows the station list to nothing. That is exactly how the
    /// Elizabeth line disappeared in the first place — not by a failing test, but by
    /// no test at all.
    #[test]
    fn every_mode_maps_to_a_known_api_name() {
        for mode in crate::modes::MODES {
            let name = mode.api_name();
            assert_eq!(
                Mode::from_api_name(name),
                Some(*mode),
                "{name} does not round-trip through from_api_name"
            );
            assert_eq!(Mode::from_api_name(&name.to_uppercase()), Some(*mode));
        }
        assert_eq!(Mode::from_api_name("hovercraft"), None);
    }

    /// Every mode has a label, a short label, a distinct glyph and a readable colour.
    ///
    /// A blank label is a blank button, and a glyph collision makes two modes
    /// indistinguishable on a dense board — which is the mode chips' whole problem.
    #[test]
    fn every_mode_is_nameable_and_coloured() {
        let mut glyphs: Vec<&str> = crate::modes::MODES.iter().map(|m| m.glyph()).collect();
        glyphs.sort_unstable();
        let unique = glyphs.len();
        glyphs.dedup();
        assert_eq!(
            glyphs.len(),
            unique,
            "two modes share a glyph, so a dense board cannot tell them apart"
        );
        for mode in crate::modes::MODES {
            assert!(!mode.label().is_empty(), "{mode:?} has no label");
            assert!(
                !mode.short_label().is_empty(),
                "{mode:?} has no short label"
            );
            assert!(
                mode.colour().starts_with('#'),
                "{mode:?} colour is not a hex"
            );
            assert!(
                contrast_ratio(mode.colour(), line_ink(mode.colour())) >= 4.5,
                "{mode:?} ({}) is not readable on its own chip",
                mode.colour()
            );
        }
    }

    /// A station is anything with a name, in every mode.
    ///
    /// The previous filter required `940GZZLU`/`940GZZCR`, which is what hid the
    /// Elizabeth line, the Overground and the DLR from a board whose own request
    /// asked for them. Every id below is a real one, taken from real responses.
    #[test]
    fn a_station_is_anything_with_a_name_in_any_mode() {
        for (mode, id, name) in [
            (Mode::Tube, "940GZZLUACT", "Acton Town Underground Station"),
            (Mode::Elizabeth, "910GABWDXR", "Abbey Wood"),
            (Mode::Overground, "910GANERLEY", "Anerley Rail Station"),
            (Mode::Dlr, "940GZZDLABR", "Abbey Road DLR Station"),
            (Mode::Cable, "940GZZALGWP", "Greenwich Peninsula"),
            (Mode::Bus, "490000173RC", "Oxford Circus Station"),
        ] {
            assert!(is_station(Some(name)), "{id} ({name}) must be a station");
            let stations = station_index(vec![(mode, vec![stop(id, Some(name))])]);
            assert_eq!(stations.len(), 1, "{name} was filtered out");
        }
        // Only a missing name is disqualifying: TfL sends `commonName: null` for the
        // unnamed legs of an interchange, and those carry their station's own id
        // shape, so without this check the picker fills with blanks.
        assert!(!is_station(None));
        assert!(!is_station(Some("")));
    }

    /// An interchange becomes **one** station holding an id per mode.
    ///
    /// This is the test the previous version could not have. One id per station
    /// meant picking Paddington showed the Underground's board and the Elizabeth
    /// line's trains were never requested — and requesting the Underground's id for
    /// the Elizabeth line returns zero arrivals, so the omission was invisible
    /// rather than obviously wrong.
    #[test]
    fn an_interchange_is_one_station_with_an_id_per_mode() {
        let index = station_index(vec![
            (
                Mode::Tube,
                vec![stop("940GZZLUACT", Some("Acton Town Underground Station"))],
            ),
            (
                Mode::Tube,
                vec![stop("940GZZLUACT", Some("Acton Town Underground Station"))],
            ),
            (
                Mode::Overground,
                vec![stop("910GACTNML", Some("Acton Town Underground Station"))],
            ),
        ]);
        assert_eq!(index.len(), 1, "one place is one row: {index:?}");
        let station = &index[0];
        assert_eq!(station.ids.len(), 2, "one id per mode");
        assert_eq!(station.id_for(ModeSet::all()), Some("940GZZLUACT"));
        let mut overground_only = ModeSet::empty();
        overground_only.insert(Mode::Overground);
        assert_eq!(station.id_for(overground_only), Some("910GACTNML"));
        assert_eq!(station.id_for(ModeSet::empty()), None);
        assert!(!station.serves(ModeSet::empty()));
    }

    /// A bus stop keeps its own row, because a bus stop is named for its street.
    ///
    /// Folding bus stops together by name would invent a station that does not
    /// exist: "Oxford Circus Station" is a dozen separate stops, and a reader who
    /// wants the one outside the Tube entrance is not served by a merged row that
    /// silently picked one of them.
    #[test]
    fn bus_stops_are_not_merged_by_name() {
        let index = station_index(vec![(
            Mode::Bus,
            vec![
                stop("490000173RC", Some("Oxford Circus Station")),
                stop("490000173RG", Some("Oxford Circus Station")),
                stop("490000173Z", Some("Oxford Circus Station")),
            ],
        )]);
        assert_eq!(index.len(), 3, "three stops, three rows: {index:?}");
        for station in &index {
            // A bus stop is keyed by its id, so however many modes TfL says it has,
            // each stop is its own row rather than being folded into one.
            assert_eq!(station.ids.len(), 1);
            assert_eq!(station.ids[0].0, Mode::Bus);
            assert!(
                station.modes.contains(Mode::Bus),
                "a bus stop is served by the bus"
            );
        }
    }

    /// A stop point the arrivals endpoint cannot answer for is not offered.
    ///
    /// Amersham arrives as `0400ZZLUAMS0`, which returns an empty array with HTTP
    /// 200 and no error — a station that looks listed and shows no trains. The
    /// requestable `940GZZLUAMS` for the same place returns live Metropolitan
    /// trains, so when both are present the requestable one wins.
    #[test]
    fn an_unrequestable_id_is_dropped_or_replaced() {
        let index = station_index(vec![(
            Mode::Tube,
            vec![stop("0400ZZLUAMS0", Some("Amersham"))],
        )]);
        assert!(index.is_empty(), "an unrequestable id offers no board");

        let index = station_index(vec![(
            Mode::Tube,
            vec![
                stop("0400ZZLUAMS0", Some("Amersham Underground Station")),
                stop("940GZZLUAMS", Some("Amersham Underground Station")),
            ],
        )]);
        assert_eq!(index.len(), 1, "one place is one row: {index:?}");
        assert_eq!(index[0].id_for(ModeSet::all()), Some("940GZZLUAMS"));

        assert!(is_requestable("940GZZLUACT"));
        assert!(is_requestable("910GABWDXR"));
        assert!(!is_requestable("0400ZZLUAMS0"));
        assert!(!is_requestable("2100ZZLUCXY0"));
        assert!(!is_requestable("490000173RC"));
    }

    /// The response's mode is authoritative and TfL's own array widens it.
    #[test]
    fn the_response_mode_wins_and_the_modes_array_widens() {
        let index = station_index(vec![(
            Mode::Tube,
            vec![stop_for(
                "940GZZLUACT",
                "Acton Town Underground Station",
                &["tube", "dlr"],
            )],
        )]);
        assert_eq!(index[0].modes.active(), vec![Mode::Tube, Mode::Dlr]);
        let index = station_index(vec![(
            Mode::Cable,
            vec![stop("940GZZALGWP", Some("Greenwich Peninsula"))],
        )]);
        assert_eq!(index[0].modes.active(), vec![Mode::Cable]);
    }

    /// A mode the app does not know is ignored, not fatal.
    #[test]
    fn an_unknown_mode_does_not_break_the_list() {
        let index = station_index(vec![(
            Mode::Tube,
            vec![stop_for(
                "940GZZLUACT",
                "Acton Town",
                &["tube", "hovercraft", "river-bus"],
            )],
        )]);
        // `river-bus` is an alias for the river, so it maps; hovercraft is ignored.
        assert_eq!(index[0].modes.active(), vec![Mode::Tube, Mode::River]);
    }

    // -------------------------------------------------------------- station sort

    /// The picker is alphabetical by name.
    #[test]
    fn stations_sort_alphabetically_by_common_name() {
        let index = station_index(vec![(
            Mode::Tube,
            vec![
                stop("940GZZLULST", Some("Liverpool Street Underground Station")),
                stop(
                    "940GZZLUKSX",
                    Some("King's Cross St. Pancras Underground Station"),
                ),
                stop("940GZZLUBZW", Some("Brixton Underground Station")),
                stop("940GZZLUWLO", Some("Wood Lane Underground Station")),
            ],
        )]);
        let names: Vec<&str> = index.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Brixton",
                "King's Cross St. Pancras",
                "Liverpool Street",
                "Wood Lane"
            ]
        );
    }

    /// The order is a comparison of names, not of ids, and it is case-insensitive
    /// with an exact tie-break — so it cannot depend on the machine's locale the way
    /// the original's `localeCompare` did.
    #[test]
    fn the_station_sort_is_case_insensitive_and_stable() {
        let index = station_index(vec![(
            Mode::Tube,
            vec![
                stop("940GZZLUAAA", Some("Acton Town")),
                stop("940GZZLUBBB", Some("acton warren")),
                stop("940GZZLUCCC", Some("Acton Town")),
            ],
        )]);
        let names: Vec<&str> = index.iter().map(|s| s.name.as_str()).collect();
        // "Acton Town" is given twice and the merge is by name, so it is one row.
        // Lowercase compares equal for it and for "acton warren", so the
        // exact-code-point tie-break decides: 'T' (0x54) < 'w' (0x77).
        assert_eq!(names, vec!["Acton Town", "acton warren"]);
    }

    // ------------------------------------------------------------- arrivals sort

    /// Arrivals come off the wire in whatever order TfL computes them, which is
    /// not the order they are due. The board sorts on `timeToStation` ascending,
    /// and that sort happens before the grouping, so a platform's own table is in
    /// due order too.
    #[test]
    fn arrivals_sort_by_time_to_station() {
        let arrivals = vec![
            arrival("Central", "Oxford Circus", Some("Platform 2"), 600),
            arrival("Central", "Bank", Some("Platform 1"), 120),
            arrival("Central", "White City", Some("Platform 2"), 300),
        ];
        let made = board(arrivals);
        // Platform 1 has one train, at 2 minutes.
        assert_eq!(minutes_of(&made.platforms[0]), vec![2]);
        // Platform 2 has two, and they are in due order: 300 before 600.
        assert_eq!(minutes_of(&made.platforms[1]), vec![5, 10]);
    }

    // --------------------------------------------------------- platform grouping

    /// Every platform becomes its own table, and the tables are ordered by the
    /// number in the platform name — so "Platform 10" comes after "Platform 9".
    #[test]
    fn platforms_group_and_sort_by_their_number() {
        let arrivals = vec![
            arrival("Piccadilly", "Heathrow", Some("Platform 10"), 120),
            arrival("Piccadilly", "Uxbridge", Some("Platform 9"), 120),
            arrival("Piccadilly", "Cockfosters", Some("Platform 1"), 120),
        ];
        let made = board(arrivals);
        let names: Vec<&str> = made.platforms.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Platform 1", "Platform 9", "Platform 10"]);
    }

    /// An arrival with no `platformName` is grouped under the fallback name, not
    /// dropped, and it leads the board because it has no number to sort by.
    #[test]
    fn an_arrival_without_a_platform_name_groups_as_unknown() {
        let arrivals = vec![
            arrival("Central", "Bank", None, 120),
            arrival("Central", "Oxford Circus", Some("Platform 2"), 300),
        ];
        let made = board(arrivals);
        assert_eq!(made.platforms[0].name, UNKNOWN_PLATFORM);
        assert_eq!(made.platforms[1].name, "Platform 2");
        // TfL also sends an empty string for some services; that is "unknown" too.
        let blank = board(vec![arrival("Central", "Bank", Some(""), 120)]);
        assert_eq!(blank.platforms[0].name, UNKNOWN_PLATFORM);
    }

    /// The platform number is the first run of digits in the name, or zero.
    #[test]
    fn the_platform_number_is_the_first_run_of_digits() {
        assert_eq!(platform_number("Platform 1"), 1);
        assert_eq!(platform_number("Platform 10"), 10);
        assert_eq!(platform_number("Platform 2 / 3"), 2);
        assert_eq!(platform_number(UNKNOWN_PLATFORM), 0);
        assert_eq!(platform_number("Eastbound"), 0);
        // A very long run of digits must not wrap round into a negative platform.
        assert_eq!(platform_number("Platform 99999999999999999999"), 0);
    }

    /// A board with no arrivals is empty, and the UI says so rather than showing
    /// an empty table.
    #[test]
    fn an_empty_arrival_list_is_an_empty_board() {
        let board = board(vec![]);
        assert!(board.is_empty());
        assert!(board.platforms.is_empty());
    }

    // ------------------------------------------------------------ arrival minutes

    /// The minutes are `floor(timeToStation / 60)`, which is not rounding: a train
    /// ninety seconds out reads one minute, because the board must not overstate
    /// how long the reader has.
    #[test]
    fn arrival_minutes_floor_rather_than_round() {
        assert_eq!(arrival_minutes(0), 0);
        assert_eq!(arrival_minutes(59), 0);
        assert_eq!(arrival_minutes(60), 1);
        assert_eq!(
            arrival_minutes(90),
            1,
            "ninety seconds is one minute, not two"
        );
        assert_eq!(arrival_minutes(119), 1);
        assert_eq!(arrival_minutes(150), 2);
        assert_eq!(arrival_minutes(3599), 59);
        assert_eq!(arrival_minutes(3600), 60);
    }

    /// A negative `timeToStation` — a train that has gone but is still listed —
    /// floors towards minus infinity and so shows a negative count, as the
    /// original did.
    #[test]
    fn a_departed_train_still_shows_a_negative_count() {
        assert_eq!(arrival_minutes(-30), -1);
        assert_eq!(arrival_minutes(-1), -1);
    }

    /// An arrival with no `timeToStation` at all is counted as arriving now, which
    /// is the `0` the original's arithmetic produced for `undefined`.
    #[test]
    fn a_missing_time_to_station_is_zero_minutes() {
        let mut missing = arrival("Central", "Bank", Some("Platform 1"), 0);
        missing.time_to_station = None;
        assert_eq!(minutes_until(&missing), 0);
        assert_eq!(minutes_until(&missing), 0, "a missing time is not a minute");
    }

    // --------------------------------------------------------------- truncation

    /// Each platform shows at most ten trains. This is a board, not a timetable.
    #[test]
    fn each_platform_shows_ten_trains() {
        let arrivals: Vec<Arrival> = (1..=25)
            .map(|n| {
                arrival(
                    "Central",
                    &format!("Terminus {n}"),
                    Some("Platform 1"),
                    n * 60,
                )
            })
            .collect();
        let made = board(arrivals);
        assert_eq!(made.platforms.len(), 1);
        assert_eq!(made.platforms[0].departures.len(), DEPARTURES_PER_PLATFORM);
        // And they are the ten soonest, not ten arbitrary ones.
        assert_eq!(
            minutes_of(&made.platforms[0]),
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
        );
    }

    /// Truncation happens per platform: a busy platform does not push a quiet one
    /// off the board.
    #[test]
    fn truncation_is_per_platform_not_global() {
        let mut arrivals: Vec<Arrival> = (1..=15)
            .map(|n| arrival("Central", "Busy", Some("Platform 1"), n * 60))
            .collect();
        arrivals.push(arrival("Northern", "Quiet", Some("Platform 2"), 300));
        let made = board(arrivals);
        let busy = made
            .platforms
            .iter()
            .find(|p| p.name == "Platform 1")
            .expect("busy platform");
        let quiet = made
            .platforms
            .iter()
            .find(|p| p.name == "Platform 2")
            .expect("quiet platform");
        assert_eq!(busy.departures.len(), DEPARTURES_PER_PLATFORM);
        assert_eq!(minutes_of(quiet), vec![5]);
    }

    // -------------------------------------------------------------- line colours

    /// Every line in the table has its published colour, and the two lines whose
    /// colours are dark — Northern, which is black, and the greys — are still
    /// listed rather than defaulted.
    #[test]
    fn every_line_has_its_published_colour() {
        for (line, colour) in [
            ("Bakerloo", "#B36305"),
            ("Central", "#E32017"),
            ("Circle", "#FFD300"),
            ("District", "#00782A"),
            ("Elizabeth line", "#6950a1"),
            ("Hammersmith & City", "#F3A9BB"),
            ("Jubilee", "#6A7278"),
            ("Metropolitan", "#9B0056"),
            ("Northern", "#000000"),
            ("Piccadilly", "#003688"),
            ("Victoria", "#00A0E2"),
            ("Waterloo & City", "#95CDBA"),
        ] {
            assert_eq!(line_colour(line), colour, "{line}");
        }
    }

    /// A line the table has never heard of is grey, and the grey is the published
    /// default from the original.
    #[test]
    fn an_unknown_line_is_grey() {
        assert_eq!(line_colour("Trams"), DEFAULT_LINE_COLOUR);
        assert_eq!(DEFAULT_LINE_COLOUR, "#666");
        // The empty line name TfL sends for some terminating services is unknown
        // too, and takes the same grey.
        assert_eq!(line_colour(""), DEFAULT_LINE_COLOUR);
    }

    /// The lookup is by exact name, not by prefix: "Northern" must not match
    /// "Northern City".
    #[test]
    fn the_line_lookup_is_exact() {
        assert_eq!(line_colour("Northern City"), DEFAULT_LINE_COLOUR);
        assert_eq!(line_colour("central"), DEFAULT_LINE_COLOUR);
    }

    // ----------------------------------------------------------------- fixtures

    /// A whole board, assembled the way TfL's response assembles it, so the shape
    /// the UI renders is the one the domain produces.
    #[test]
    fn a_whole_board_renders_the_platforms_in_order() {
        let arrivals = vec![
            arrival("Northern", "Barking", Some("Platform 9"), 300),
            arrival("Northern", "Edgware", Some("Platform 1"), 120),
            arrival("Northern", "Mill Hill East", Some("Platform 2"), 180),
            arrival("Northern", "Belsize Park", Some("Platform 2"), 600),
        ];
        let made = board(arrivals);
        let names: Vec<&str> = made.platforms.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Platform 1", "Platform 2", "Platform 9"]);
        assert_eq!(minutes_of(&made.platforms[1]), vec![3, 10]);
    }

    /// The JSON TfL sends, as fixtures, parsed through the same types the browser
    /// uses. This is the one thing a hand-built Rust fixture cannot check: that
    /// the field names on the wire still match what the app asks for.
    ///
    /// These fixtures are transcribed from a real response, and they carry the
    /// fields the app ignores as well as the two it reads — a fixture trimmed to
    /// the two fields it needs proves nothing about whether it is reading the
    /// right ones. They also carry the heavy `children` and `additionalProperties`
    /// arrays, because those are 95% of the payload and the reason the fetch path
    /// changed.
    #[test]
    fn the_wire_formats_still_parse() {
        // The `StopPoint/Mode/...` shape: an object wrapping a `stopPoints` array.
        let stop_points: serde_json::Value = serde_json::from_str(
        r#"{
          "$type": "TflStopPoint",
          "stopPoints": [
            {"$type":"TflStopPoint","id":"940GZZLUKSX","commonName":"King's Cross St. Pancras Underground Station","icsCode":"kgx","lat":51.5308,"lon":-0.1238,"modes":["tube"],"children":[{"$type":"TflStopPoint","id":"940GZZLUKSX:1","commonName":"King's Cross St. Pancras Rail Station"}]},
            {"$type":"TflStopPoint","id":"940GZZLULST","commonName":"Liverpool Street Underground Station","icsCode":"lst"},
            {"$type":"TflStopPoint","id":"940GZZCRBOW","commonName":"Bow Road Underground Station","icsCode":"bow"},
            {"$type":"TflStopPoint","id":"4900000934","commonName":"Tottenham Court Road Underground Station"},
            {"$type":"TflStopPoint","id":"940GZZLUBZW","commonName":"Brixton Underground Station"},
            {"$type":"TflStopPoint","id":"940GZZLUKSX:2","commonName":null,"children":[]},
            {"$type":"TflStopPoint","id":"0400ZZLUAMS0","commonName":"Amersham Underground Station"}
          ]
        }"#,
    )
    .expect("stop point list");
        let list: Vec<StopPoint> = serde_json::from_value(stop_points["stopPoints"].clone())
            .expect("the stop points, deserialised exactly as the browser does");
        let kept = station_index(vec![(Mode::Tube, list)]);
        let names: Vec<&str> = kept.iter().map(|s| s.name.as_str()).collect();
        // Amersham is in this fixture as `0400ZZLUAMS0`, an id the arrivals
        // endpoint answers with an empty array, so it is not offered: a station
        // that cannot produce a board has no business in the picker. That is a
        // different question from the old prefix filter, which rejected it — along
        // with the Elizabeth line, the Overground and the DLR — for merely not
        // starting with `940GZZLU`.
        assert_eq!(
            names,
            vec![
                "Bow Road",
                "Brixton",
                "King's Cross St. Pancras",
                "Liverpool Street",
            ],
            // Two are absent, and both for a reason worth keeping: Amersham carries
            // only `0400ZZ…`, which the arrivals endpoint answers with an empty
            // array; and `4900000934`, the old Tottenham Court Road id, is not a
            // requestable prefix at all. A row that cannot produce a board is not
            // offered.
        );
        // The ids must be the real ones, or every later request 404s. This is the
        // assertion that would have caught the `stopPointId` mistake: deserialising
        // that name leaves every id empty, the filter drops everything, and the
        // board quietly offers no stations at all.
        assert!(
            kept.iter()
                .all(|s| s.id_for(ModeSet::all()).is_some_and(is_requestable)),
            "every station kept must carry a requestable id"
        );
        assert!(
            kept.iter()
                .any(|s| s.id_for(ModeSet::all()) == Some("940GZZLUBZW")),
            "Brixton by id"
        );

        let arrivals: serde_json::Value = serde_json::from_str(
        r#"[
          {"$type":"TflArrival","stationName":"Brixton","platformName":"Platform 1","lineName":"Victoria","destinationName":"Walthamstow Central","timeToStation":119},
          {"$type":"TflArrival","stationName":"Brixton","platformName":"Platform 2","lineName":"Victoria","destinationName":"Brixton","timeToStation":-30},
          {"$type":"TflArrival","stationName":"Brixton","lineName":"","destinationName":"Stockwell","timeToStation":300},
          {"$type":"TflArrival","stationName":"Brixton","platformName":"Platform 1","lineName":"Victoria","destinationName":"Oxford Circus"}
        ]"#,
    )
    .expect("arrivals");
        let parsed: Vec<Arrival> = arrivals
            .as_array()
            .expect("arrivals array")
            .iter()
            .map(|a| Arrival {
                line_name: a["lineName"].as_str().unwrap_or_default().to_string(),
                destination: a["destinationName"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                time_to_station: a["timeToStation"].as_i64().map(|s| s as i32),
                platform: a["platformName"].as_str().map(str::to_string),
                mode: a["modeName"].as_str().and_then(Mode::from_api_name),
            })
            .collect();
        let board = board(parsed);
        let names: Vec<&str> = board.platforms.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Unknown Platform", "Platform 1", "Platform 2"]);
        // The unknown-platform train is 300s out (5 min), the platform-1 trains
        // are 119s (1 min) and missing (0 min) — sorted, so 0 then 1.
        assert_eq!(minutes_of(&board.platforms[0]), vec![5]);
        assert_eq!(minutes_of(&board.platforms[1]), vec![0, 1]);
        // A departed train keeps its negative count.
        assert_eq!(minutes_of(&board.platforms[2]), vec![-1]);
    }

    /// A real `Line/<id>/StopPoints` response, parsed the way the browser parses
    /// it, through the same type the picker is filled from.
    ///
    /// The fixture is a verbatim slice of a real response, trimmed to eight stop
    /// points but otherwise untouched — the heavy `children` and
    /// `additionalProperties` arrays are still there, because the size of what
    /// arrives is the whole point of the per-line fetch. If this test ever fails
    /// with an empty station list, the id field has been renamed again: that is the
    /// failure that leaves the picker permanently empty while reporting nothing.
    #[test]
    fn a_real_stop_point_response_yields_stations() {
        let bytes = include_bytes!("../tests/fixtures_line_stoppoints.json");
        let stops: Vec<StopPoint> =
            serde_json::from_slice(bytes).expect("a real line stop-point response");
        let stations = station_index(vec![(Mode::Tube, stops)]);

        assert!(
            !stations.is_empty(),
            "a real response must yield stations; an empty list means the id field \
         is wrong again"
        );
        // All eight of the fixture's stop points have a requestable id, and Amersham
        // is among them: its `940GZZLUAMS` form returns live Metropolitan trains even
        // though the same response also lists a `0400ZZLUAMS0` form that returns none.
        // That pair is the whole reason `is_requestable` exists, and this count is
        // what says the filter is not throwing away stations that do have a board.
        assert_eq!(stations.len(), 8, "unexpected station count: {stations:?}");
        let names: Vec<&str> = stations.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Acton Town",
                "Aldgate",
                "Aldgate East",
                "Alperton",
                "Amersham",
                "Angel",
                "Archway",
                "Arnos Grove",
            ]
        );
        // And the ids are the ones the arrivals endpoint will accept.
        for station in &stations {
            assert!(
                is_requestable(station.id_for(ModeSet::all()).unwrap_or_default()),
                "{} has an id the arrivals endpoint will not answer for",
                station.name
            );
        }
        // A station on three lines arrives in three responses; the picker must not
        // show it three times.
        let unique: std::collections::HashSet<&str> = stations
            .iter()
            .map(|s| s.id_for(ModeSet::all()).unwrap_or_default())
            .collect();
        assert_eq!(
            unique.len(),
            stations.len(),
            "duplicate stations in the picker"
        );
    }

    /// Every line must be readable on its own chip, in whichever ink `line_ink`
    /// chose. This is the assertion that makes the colour scheme a property of the
    /// code rather than a matter of taste: if a line is added with a colour that
    /// cannot reach 4.5:1 against either black or white, this fails and the fix is
    /// a chip, not a font size.
    #[test]
    fn every_line_is_readable_on_its_own_chip() {
        for line in LINE_COLOURS.iter().map(|(name, _)| *name) {
            let colour = line_colour(line);
            let ink = line_ink(colour);
            let ratio = contrast_ratio(colour, ink);
            assert!(
                ratio >= 4.5,
                "{line} ({colour}) is only {ratio:.2}:1 against {ink}; a line name \
             the reader cannot read is not a colour scheme"
            );
        }
        // And the default grey, which is what an unknown line gets.
        assert!(contrast_ratio(DEFAULT_LINE_COLOUR, line_ink(DEFAULT_LINE_COLOUR)) >= 4.5);
    }

    /// The two extremes of the ratio, which is what pins the whole function: the
    /// ratio is 21:1 between black and white, and 1:1 between a colour and itself.
    #[test]
    fn the_contrast_ratio_is_the_wcag_one() {
        let ratio = contrast_ratio("#000000", "#ffffff");
        assert!(
            (ratio - 21.0).abs() < 0.01,
            "black on white is 21:1, got {ratio}"
        );
        assert!((contrast_ratio("#FFD300", "#FFD300") - 1.0).abs() < 0.01);
        // Case-insensitive, and the short form.
        assert_eq!(relative_luminance("#fff"), relative_luminance("#FFFFFF"));
        // Unparsable input must not produce NaN and take the chip with it.
        assert_eq!(relative_luminance("not a colour"), 0.0);
        assert!(contrast_ratio("nonsense", "#ffffff").is_finite());
    }

    /// The two inks must be chosen correctly for the awkward cases, which are the
    /// ones a test on "is it high contrast" would wave through.
    #[test]
    fn the_ink_follows_the_chip() {
        // Pale chips want black: Circle's yellow is 14.6:1 on black and 1.4:1 on
        // white.
        assert_eq!(line_ink("#FFD300"), DARK_INK);
        assert_eq!(line_ink("#95CDBA"), DARK_INK);
        assert_eq!(line_ink("#F3A9BB"), DARK_INK);
        // Dark chips want white: Northern is black, so it must not be black on
        // black.
        assert_eq!(line_ink("#000000"), LIGHT_INK);
        assert_eq!(line_ink("#003688"), LIGHT_INK);
        // The middles, where it is not obvious, are pinned so a change to the
        // arithmetic cannot quietly flip them.
        assert_eq!(line_ink("#B36305"), DARK_INK, "Bakerloo, 4.70:1 on black");
        assert_eq!(line_ink("#E32017"), LIGHT_INK, "Central, 4.68:1 on white");
        assert_eq!(line_ink("#00A0E2"), DARK_INK, "Victoria, 7.13:1 on black");
    }

    /// A chip also has to be *findable*: its fill must not disappear into the page
    /// behind it.
    ///
    /// The `line_ink` test covers the text against its own chip, and that is not
    /// the same question. Northern's black is `#000000`, which is 1.07:1 against the
    /// dark page and would be an invisible rectangle; Circle's yellow is 1.34:1
    /// against the light page. No fill colour can satisfy both schemes at once, so
    /// the shell gives every chip a hairline ring instead — see `--ring` in
    /// `ui.html` — and this test pins the numbers that decision was made on, so
    /// whoever changes the page background knows what they are testing.
    #[test]
    fn the_chip_edge_is_a_decision_the_numbers_record() {
        // The two fills that are invisible against a page, one per scheme. If a
        // future palette change makes a third one, this test is the place to notice.
        for line in ["Northern", "Circle"] {
            let colour = line_colour(line);
            let against_dark = contrast_ratio(colour, DARK_PAGE);
            let against_light = contrast_ratio(colour, LIGHT_PAGE);
            assert!(
                against_dark < 1.5 || against_light < 1.5,
                "{line} ({colour}) is now visible on both pages ({against_dark:.2} \
             dark, {against_light:.2} light); the ring may no longer be needed"
            );
        }
        // And the rest of the palette does not need the ring to be seen, which is
        // why the ring is decoration rather than the accessibility mechanism.
        for line in ["Central", "Victoria", "Piccadilly", "Metropolitan"] {
            let colour = line_colour(line);
            assert!(
                contrast_ratio(colour, DARK_PAGE) >= 1.5
                    || contrast_ratio(colour, LIGHT_PAGE) >= 1.5,
                "{line} is dim on both pages"
            );
        }
    }

    /// The twelve per-line responses, concatenated the way the browser does, must
    /// give one option per station.
    ///
    /// This is the shape that shipped the bug: every line's response in one list,
    /// so an interchange on three lines is present three times. The fixture is a
    /// real slice — Acton Town really is served by the Bakerloo and the Piccadilly,
    /// and really does come back from both.
    #[test]
    fn concatenating_the_line_responses_does_not_repeat_a_station() {
        let bakerloo = include_bytes!("../tests/fixtures_bakerloo_stoppoints.json");
        let piccadilly = include_bytes!("../tests/fixtures_piccadilly_stoppoints.json");
        let mut all: Vec<StopPoint> =
            serde_json::from_slice(bakerloo).expect("the Bakerloo response");
        all.extend(
            serde_json::from_slice::<Vec<StopPoint>>(piccadilly).expect("the Piccadilly response"),
        );
        let before = all.len();
        let stations = station_index(vec![(Mode::Tube, all)]);
        assert!(
            stations.len() < before,
            "concatenating responses must collapse repeats: {} in, {} out",
            before,
            stations.len()
        );
        let mut ids: Vec<&str> = stations
            .iter()
            .map(|s| s.id_for(ModeSet::all()).unwrap_or_default())
            .collect();
        let unique = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(
            ids.len(),
            unique,
            "the picker must hold no repeated station"
        );
        // Acton Town is on both lines, so it must appear exactly once.
        assert_eq!(
            stations.iter().filter(|s| s.name == "Acton Town").count(),
            1,
            "Acton Town is served by the Bakerloo and the Piccadilly and must be \
         offered once"
        );
    }

    // -------------------------------------------------------------- the search

    /// A small station list to search. The ranking tests all want the same one, and
    /// a shared fixture keeps them comparable.
    fn searchable() -> Vec<Station> {
        station_index(vec![(
            Mode::Tube,
            vec![
                stop("940GZZLUACT", Some("Acton Town Underground Station")),
                stop("940GZZLUBZW", Some("Brixton Underground Station")),
                stop("940GZZLUBKG", Some("Barking Underground Station")),
                stop(
                    "940GZZLUKSX",
                    Some("Kings Cross St. Pancras Underground Station"),
                ),
                stop("940GZZLULEY", Some("Leyton Underground Station")),
                stop("940GZZLULST", Some("Liverpool Street Underground Station")),
            ],
        )])
    }

    /// Typing a fragment finds the station whose name starts with it.
    #[test]
    fn search_finds_a_name_by_its_start() {
        let stations = searchable();
        let found = Search::new(&stations).query("brix", ModeSet::all());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Brixton");
    }

    /// A word from the middle of a name finds it too, which is what makes search
    /// usable: a reader remembers one word of a four-word station name, not where
    /// its first letters fall.
    #[test]
    fn search_matches_a_word_inside_the_name() {
        let stations = searchable();
        let found = Search::new(&stations).query("cross", ModeSet::all());
        assert_eq!(found.len(), 1, "'cross' must find Kings Cross");
        assert!(found[0].name.contains("Cross"));
    }

    /// An exact name outranks a prefix, which outranks a word match — so typing a
    /// station's name in full puts it first rather than fourth.
    #[test]
    fn an_exact_match_ranks_first() {
        let stations = searchable();
        // The name TfL gave the stop point, mode suffix and all. The board now
        // *shows* "Brixton", but a reader arriving from a search engine or an old
        // bookmark types what this station used to be called, and that must still
        // find it and put it first.
        let found = Search::new(&stations).query("Brixton Underground Station", ModeSet::all());
        assert_eq!(found[0].name, "Brixton");
    }

    /// Fuzzy search finds the station a mistyped query meant.
    ///
    /// These are the queries a reader produces on a phone, not the ones they mean.
    #[test]
    fn a_mistyped_query_still_finds_the_station() {
        let stations = searchable();
        let search = Search::new(&stations);
        // "kngs crs" is King's Cross with vowels and an apostrophe dropped.
        let found = search.query("kngs crs", ModeSet::all());
        assert!(
            found.iter().any(|s| s.name.contains("Kings Cross")),
            "a subsequence must find Kings Cross from \"kngs crs\": {:?}",
            found.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
        // The letters must come in order. "zzngs" cannot match anything: no station
        // name in this list has two consecutive `z`s in the wrong places.
        assert!(
            search.query("zzngs", ModeSet::all()).is_empty(),
            "a subsequence must be in order"
        );
        // A dropped letter still reaches its station: "brxton" is Brixton with the
        // `i` left out.
        let found = search.query("brxton", ModeSet::all());
        assert!(
            found.iter().any(|s| s.name.starts_with("Brixton")),
            "a dropped letter must still find Brixton: {:?}",
            found.iter().map(|s| &s.name).collect::<Vec<_>>()
        );
    }

    /// An empty query offers nothing, rather than all 2,700 stations.
    #[test]
    fn an_empty_query_offers_nothing() {
        let stations = searchable();
        let search = Search::new(&stations);
        assert!(search.query("", ModeSet::all()).is_empty());
        assert!(search.query("   ", ModeSet::all()).is_empty());
    }

    /// Fuzzy matching is ranked below every exact match, so it never crowds out the
    /// station the reader actually asked for.
    #[test]
    fn a_fuzzy_match_ranks_below_a_prefix_match() {
        let stations = station_index(vec![(
            Mode::Tube,
            vec![
                stop("940GZZLUAAA", Some("Brixton Underground Station")),
                stop("940GZZLUAAB", Some("Barking Riverside Underground Station")),
            ],
        )]);
        let search = Search::new(&stations);
        // "bri" is a prefix of Brixton and a subsequence of neither the other.
        let found = search.query("bri", ModeSet::all());
        assert_eq!(found[0].name, "Brixton");
    }

    /// The search respects the mode switches: with only the Underground on, a bus
    /// stop is not offered even though it matches perfectly.
    #[test]
    fn search_respects_the_mode_switches() {
        let stations = station_index(vec![
            (
                Mode::Tube,
                vec![stop("940GZZLULEY", Some("Leyton Underground Station"))],
            ),
            (Mode::Bus, vec![stop("4900000944", Some("Leyton Station"))]),
        ]);
        let search = Search::new(&stations);
        assert_eq!(search.query("leyton", ModeSet::all()).len(), 2);

        let mut tube_only = ModeSet::empty();
        tube_only.insert(Mode::Tube);
        let found = search.query("leyton", tube_only);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Leyton");

        let mut bus_only = ModeSet::empty();
        bus_only.insert(Mode::Bus);
        let found = search.query("leyton", bus_only);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Leyton Station");
    }

    /// The list is capped, and it is capped by showing the *best* matches rather
    /// than the first twelve found.
    #[test]
    fn search_is_capped_and_returns_the_best() {
        let mut points: Vec<StopPoint> = (0..40)
            .map(|n| {
                stop(
                    &format!("940GZZLUB{n:02}"),
                    Some(&format!("Brixton Road {n} Underground Station")),
                )
            })
            .collect();
        // One exact match, last in the input.
        points.push(stop("940GZZLUBZZ", Some("Brixton")));
        let stations = station_index(vec![(Mode::Tube, points)]);
        let found = Search::new(&stations).query("Brixton", ModeSet::all());
        assert_eq!(found.len(), SEARCH_LIMIT, "the list is capped");
        assert_eq!(found[0].name, "Brixton", "the exact match is first");
    }

    /// Typing narrows the list, so results do not jump about as a reader types.
    #[test]
    fn results_narrow_as_the_query_grows() {
        let stations = searchable();
        let search = Search::new(&stations);
        let one = search.query("b", ModeSet::all()).len();
        let two = search.query("br", ModeSet::all()).len();
        assert!(
            two <= one,
            "a longer query cannot match more: {one} then {two}"
        );
        assert!(two >= 1, "'br' must still match Brixton");
    }

    // ------------------------------------------------- mode on the timetable

    /// A mode switch hides that mode's platforms and keeps the others.
    ///
    /// This is the toggle's effect on the board, which it did not have: a platform
    /// is kept exactly when its mode is on.
    #[test]
    fn switching_a_mode_off_hides_only_that_mode() {
        let arrivals = vec![
            arrival_on("Central", "Epping", Some("Platform 3"), 120, Mode::Tube),
            arrival_on("12", "Oxford Circus", Some("Platform A"), 300, Mode::Bus),
        ];
        let made = board(arrivals);

        let tube_only = {
            let mut set = ModeSet::empty();
            set.insert(Mode::Tube);
            made.filtered(set)
        };
        assert_eq!(tube_only.platforms.len(), 1);
        assert_eq!(tube_only.platforms[0].mode, Some(Mode::Tube));

        let bus_only = {
            let mut set = ModeSet::empty();
            set.insert(Mode::Bus);
            made.filtered(set)
        };
        assert_eq!(bus_only.platforms.len(), 1);
        assert_eq!(bus_only.platforms[0].mode, Some(Mode::Bus));

        assert_eq!(made.filtered(ModeSet::all()).platforms.len(), 2);
        assert!(made.filtered(ModeSet::empty()).platforms.is_empty());
    }

    /// Two services sharing a platform name stay in separate tables.
    ///
    /// The board filters by mode, so a bus and a train both answering "Platform 1"
    /// must not land in one table: switching either mode off has to remove one
    /// service without touching the other.
    #[test]
    fn one_platform_name_holds_one_mode() {
        let arrivals = vec![
            arrival_on("Northern", "Battersea", Some("Platform 1"), 60, Mode::Tube),
            arrival_on("88", "Acton Green", Some("Platform 1"), 180, Mode::Bus),
        ];
        let made = board(arrivals);
        assert_eq!(made.platforms.len(), 2, "one table per mode");
        let mut modes: Vec<Option<Mode>> = made.platforms.iter().map(|p| p.mode).collect();
        modes.sort_by_key(|m| m.map_or(99, position));
        assert_eq!(modes, vec![Some(Mode::Tube), Some(Mode::Bus)]);
    }

    /// A row TfL did not classify is not hidden by a switch.
    ///
    /// The wire sends `modeName: null` for some services. A reader's toggle is a
    /// choice of *which services to see*; it is not a reason to drop a service this
    /// app cannot name, which would leave the board silently short.
    #[test]
    fn an_unnamed_mode_survives_every_filter() {
        let made = board(vec![arrival("?", "Somewhere", Some("Platform 1"), 60)]);
        assert_eq!(made.platforms.len(), 1);
        assert_eq!(made.platforms[0].mode, None);
        assert_eq!(made.filtered(ModeSet::empty()).platforms.len(), 1);
    }

    /// The board can say that a switch is hiding something, rather than showing an
    /// empty table that reads as "nothing is running".
    #[test]
    fn a_hidden_platform_is_distinguishable_from_an_empty_one() {
        let arrivals = vec![arrival_on(
            "Central",
            "Epping",
            Some("Platform 3"),
            60,
            Mode::Tube,
        )];
        let made = board(arrivals);
        let bus_only = {
            let mut set = ModeSet::empty();
            set.insert(Mode::Bus);
            set
        };
        assert!(made.has_hidden_platforms(bus_only));
        assert!(!made.has_hidden_platforms(ModeSet::all()));
        // And an arrival with no mode is never counted as hidden.
        let unnamed = board(vec![arrival("?", "Somewhere", Some("Platform 1"), 60)]);
        assert!(!unnamed.has_hidden_platforms(bus_only));
    }

    /// Every mode paints its own chip, and the Underground keeps its line colours.
    ///
    /// The bug this answers: the chip colour came from the line table alone, so
    /// every bus route, river service and cable-car leg was grey — one colour for
    /// every mode on a board whose entire visual language is colour.
    #[test]
    fn a_departures_colour_falls_back_to_the_modes_own() {
        // A line TfL names after the Underground keeps its published line colour.
        assert_eq!(
            departure_colour("Central", Some(Mode::Tube)),
            line_colour("Central")
        );
        // The Elizabeth line is named as a line, so it too keeps its line colour.
        assert_eq!(
            departure_colour("Elizabeth line", Some(Mode::Elizabeth)),
            line_colour("Elizabeth line")
        );
        // A bus route number is not a line name, so the bus's own colour stands in.
        assert_eq!(departure_colour("88", Some(Mode::Bus)), Mode::Bus.colour());
        assert_eq!(departure_colour("12", Some(Mode::Bus)), Mode::Bus.colour());
        // Every mode paints itself when its lines are not in the line table.
        for mode in crate::modes::MODES {
            let line = match mode {
                Mode::Tube | Mode::Elizabeth => continue,
                other => other.api_name(),
            };
            assert_eq!(
                departure_colour(line, Some(*mode)),
                mode.colour(),
                "{mode:?} fell back to grey instead of its own colour"
            );
            // And it is readable on its own chip, which is the reason the colour
            // exists at all.
            assert!(
                contrast_ratio(mode.colour(), line_ink(mode.colour())) >= 4.5,
                "{mode:?} chip is not readable"
            );
        }
        // A service with no mode and no known line is the only grey left, and that
        // is the honest answer rather than a wrong colour.
        assert_eq!(departure_colour("mystery", None), DEFAULT_LINE_COLOUR);
    }

    // ------------------------------------------- interchanges are one station

    /// One interchange is one row, whatever the modes call it.
    ///
    /// TfL names each half after its mode, so searching "bank" used to return the
    /// Underground, the DLR and the National Rail as three stations. They are one
    /// place, and this is the test that says so.
    #[test]
    fn an_interchange_named_per_mode_is_one_row() {
        let index = station_index(vec![
            (
                Mode::Tube,
                vec![stop("940GZZLUBNK", Some("Bank Underground Station"))],
            ),
            (
                Mode::Dlr,
                vec![stop("940GZZDLBNK", Some("Bank DLR Station"))],
            ),
            (
                Mode::NationalRail,
                vec![stop("910GBNKLO", Some("Bank National Rail Station"))],
            ),
        ]);
        assert_eq!(index.len(), 1, "one place is one row: {index:?}");
        let station = &index[0];
        assert_eq!(station.name, "Bank");
        assert_eq!(station.modes.active().len(), 3, "all three modes kept");
        assert_eq!(station.ids.len(), 3, "one id per mode");
    }

    /// The mode suffix is removed, and nothing else.
    ///
    /// Over-stripping would fold two genuinely different places together, so this
    /// pins both directions: a name that is only a suffix keeps its whole self, and a
    /// name with no suffix is untouched.
    #[test]
    fn only_a_trailing_mode_word_is_removed() {
        assert_eq!(place_name("Bank Underground Station"), "Bank");
        assert_eq!(place_name("Bank DLR Station"), "Bank");
        assert_eq!(
            place_name("Stratford International DLR Station"),
            "Stratford International"
        );
        assert_eq!(
            place_name("London Liverpool Street"),
            "London Liverpool Street"
        );
        assert_eq!(
            place_name("Heathrow Terminals 2 & 3 Underground Station"),
            "Heathrow Terminals 2 & 3"
        );
        // A longer tail wins over the shorter one it contains, so the name does not
        // keep a stray "National Rail".
        assert_eq!(
            place_name("Kentish Town National Rail Station"),
            "Kentish Town"
        );
        // Casing is display, not matching: the key folds it, the name does not.
        assert_eq!(
            place_name("King's Cross St. Pancras"),
            "King's Cross St. Pancras"
        );
        assert_eq!(
            interchange_key("King's Cross St. Pancras"),
            "king's cross st. pancras"
        );
        // A name that is nothing but a suffix keeps its whole self rather than
        // becoming empty.
        assert_eq!(place_name("Station"), "Station");
        // Bus stops are named for the street and are not merged by this rule.
        assert_eq!(place_name("Oxford Circus Station"), "Oxford Circus");
    }

    /// Two different stations that share a word are still two rows.
    ///
    /// The merge is on the whole place name, never on a word of it, so "Paddington"
    /// and "Paddington Royal" cannot collapse into one station.
    #[test]
    fn a_shared_word_does_not_merge_two_places() {
        let index = station_index(vec![(
            Mode::Tube,
            vec![
                stop("940GZZLUPAD", Some("Paddington Underground Station")),
                stop("940GZZLUPRY", Some("Paddington Royal Underground Station")),
            ],
        )]);
        assert_eq!(index.len(), 2, "two places, two rows: {index:?}");
    }

    // ------------------------------------------- telling same-named stops apart

    /// The three stops at Oxford Circus describe themselves differently.
    ///
    /// This is the complaint behind the field: three rows named "Oxford Circus
    /// Station" and nothing to choose between them. Each must say something the
    /// others do not, or the list is still a list of identical rows.
    #[test]
    fn same_named_stops_describe_themselves_differently() {
        let detail = |json: &str| -> StopDetail {
            serde_json::from_str::<StopPointDetail>(json)
                .expect("stop point detail")
                .into_detail()
        };
        // Measured off the live API, one row of the real response each. Note that
        // `stopLetter` is the bare flag ("RC"); the wordy "Stop RC" is a different
        // field, `indicator`, and is not what a reader reads off the flag.
        let rc = detail(
            r#"{"stopLetter":"RC",
            "lines":[{"name":"12"},{"name":"159"},{"name":"22"}],
            "additionalProperties":[{"key":"CompassPoint","value":"N"},
                                    {"key":"Towards","value":"Marble Arch Or Great Portland Street"}]}"#,
        );
        let rg = detail(
            r#"{"stopLetter":"RG",
            "lines":[{"name":"139"},{"name":"159"},{"name":"22"}],
            "additionalProperties":[{"key":"Towards","value":"Trafalgar Square Or Green Park"}]}"#,
        );
        let oh =
            detail(r#"{"stopLetter":"OH","lines":[{"name":"N137"}],"additionalProperties":[]}"#);

        // The flag letter leads, because that is what a reader standing at the stop
        // can match against the flag in front of them.
        assert_eq!(rc.describe(), "RC · 12, 159, 22");
        // The letter and routes differ, so no two of these describe themselves the
        // same way — which is the entire point of the exercise.
        assert_eq!(rg.describe(), "RG · 139, 159, 22");
        assert_eq!(oh.describe(), "OH · N137");
        let all = [rc.describe(), rg.describe(), oh.describe()];
        let unique: std::collections::HashSet<&String> = all.iter().collect();
        assert_eq!(
            unique.len(),
            3,
            "each stop must say something the others do not"
        );
    }

    /// A stop with nothing to distinguish it says nothing rather than an empty line.
    #[test]
    fn a_stop_with_nothing_to_tell_says_nothing() {
        assert_eq!(StopDetail::default().describe(), "");
        // A letter alone is still a letter.
        let letter_only = StopDetail {
            letter: Some("AB".to_string()),
            ..StopDetail::default()
        };
        assert_eq!(letter_only.describe(), "AB");
        // And a stop with only a direction falls back to it, so it is not blank.
        let direction_only = StopDetail {
            towards: Some("Waterloo".to_string()),
            ..StopDetail::default()
        };
        assert_eq!(direction_only.describe(), "Waterloo");
        // Whitespace-only values are not values.
        let blank = StopDetail {
            letter: Some("   ".to_string()),
            routes: Vec::new(),
            towards: Some("  ".to_string()),
        };
        assert_eq!(blank.describe(), "");
    }

    /// A route repeated on one stop is listed once.
    ///
    /// TfL lists a stop's lines by identifier, and the same route can appear twice
    /// when it runs in both directions. Showing "22, 22" reads as two different
    /// services.
    #[test]
    fn a_repeated_route_is_listed_once() {
        let detail = serde_json::from_str::<StopPointDetail>(
            r#"{"lines":[{"name":"22"},{"name":"22"},{"name":"88"},{"name":"  "}]}"#,
        )
        .expect("stop point detail")
        .into_detail();
        assert_eq!(detail.routes, vec!["22", "88"]);
        assert_eq!(detail.describe(), "22, 88");
    }
}

// ------------------------------------------------- the mode response's shape

/// The wrapper `StopPoint/Mode/{mode}` puts its list in.
///
/// **The endpoint does not return a bare array.** It returns
/// `{"$type": …, "stopPoints": [...]}`.
///
/// That is worth a type and a test of its own, because the mistake is silent in
/// the worst way: deserialising the wrapper as a `Vec<StopPoint>` fails to parse,
/// the failure is caught, and the board reports that it could not load any
/// stations — with every HTTP status at 200 and the network perfectly healthy. It
/// looks exactly like being offline, and it cost a full debugging cycle.
///
/// The previous version had three shapes in play and this one of them; the two
/// that remain are [`StopPointList`] and the bare array `Line/{id}/StopPoints`
/// returns, which is why both are named rather than one being assumed.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct StopPointList {
    #[serde(rename = "stopPoints", default)]
    pub stop_points: Vec<StopPoint>,
}

#[cfg(test)]
mod shape {
    use super::*;

    /// The first two stop points of a real `elizabeth-line` response, including
    /// the fields the app does not read — a fixture trimmed to the two fields it
    /// needs proves nothing about whether it is reading the right ones.
    const MODE_RESPONSE: &str = r#"{
      "$type": "TflStopPoint",
      "stopPoints": [
        {"$type":"TflStopPoint","id":"9100ABWDXR0","name":"Abbey Wood Station","commonName":"Abbey Wood Station","lat":51.4940,"lon":0.0703,"modes":["elizabeth-line"],"children":[],"additionalProperties":{}},
        {"$type":"TflStopPoint","id":"9100ABWDXR1","name":"Abbey Wood Station","commonName":null,"lat":51.4940,"lon":0.0703,"modes":["elizabeth-line"]}
      ]
    }"#;

    #[test]
    fn the_mode_response_is_a_wrapper_not_an_array() {
        let list: StopPointList = serde_json::from_str(MODE_RESPONSE).expect("the wrapper parses");
        assert_eq!(list.stop_points.len(), 2);
        assert_eq!(list.stop_points[0].id, "9100ABWDXR0");
        assert_eq!(
            list.stop_points[0].name.as_deref(),
            Some("Abbey Wood Station")
        );
        assert_eq!(list.stop_points[0].modes, vec!["elizabeth-line"]);
        // The nameless leg parses to `None`, which is what the filter rejects.
        assert_eq!(list.stop_points[1].name, None);

        // And the thing that broke: a bare-array parse of the same bytes fails.
        assert!(
            serde_json::from_str::<Vec<StopPoint>>(MODE_RESPONSE).is_err(),
            "if this ever parses as a bare array the endpoint changed and \
             `load_stations` must be revisited"
        );
    }

    /// An empty or missing list is not a failure — it is an empty picker, and the
    /// board must say so rather than report an error it cannot act on.
    #[test]
    fn an_empty_mode_response_parses_to_nothing() {
        let list: StopPointList =
            serde_json::from_str(r#"{"stopPoints":[]}"#).expect("empty parses");
        assert!(list.stop_points.is_empty());
        let list: StopPointList = serde_json::from_str(r#"{}"#).expect("a missing key parses");
        assert!(list.stop_points.is_empty());
    }

    // ---------------------------------------------------- the search response

    #[cfg(test)]
    mod search_shape {
        use super::*;

        /// A real response for "Stratford International Rail", verbatim.
        const RESPONSE: &str = r#"{
      "$type": "Tfl.Api.Presentation.Entities.SearchResponse, Tfl.Api.Presentation.Entities",
      "query": "Stratford International Rail",
      "total": 1,
      "matches": [
        {"$type":"Tfl.Api.Presentation.Entities.Match, Tfl.Api.Presentation.Entities",
         "icsId":"910GSTFODOM","id":"910GSTFODOM","lat":51.5446,"lon":-0.0133,
         "name":"Stratford International Rail Station","modes":["national-rail"]}
      ]
    }"#;

        #[test]
        fn a_search_match_carries_its_id() {
            let response: SearchResponse =
                serde_json::from_str(RESPONSE).expect("the search parses");
            assert_eq!(response.matches.len(), 1);
            let found = &response.matches[0];
            assert_eq!(
                found.stop_point_id.as_deref(),
                Some("910GSTFODOM"),
                "the national-rail stop must keep its id, or it cannot be requested"
            );
            assert_eq!(found.name, "Stratford International Rail Station");
            assert_eq!(found.modes, vec!["national-rail"]);
            // And the mode maps, which is what labels the row.
            assert_eq!(
                found
                    .modes
                    .iter()
                    .find_map(|name| crate::modes::Mode::from_api_name(name)),
                Some(crate::modes::Mode::NationalRail)
            );
        }

        /// A match with no id is a bus *route* or a line, not a stop point, and has
        /// nothing to request arrivals for. It must be dropped rather than offered.
        #[test]
        fn a_match_without_an_id_is_dropped() {
            let response: SearchResponse = serde_json::from_str(
                r#"{"matches":[{"name":"Route 88","icsId":"88","modes":["bus"]}]}"#,
            )
            .expect("parses");
            assert!(response.matches[0].stop_point_id.is_none());
            assert!(
                response.matches[0].stop_point_id.is_none(),
                "a match with no stop-point id is not a station"
            );
        }

        /// TfL sends `"id"`. Deserialising the documented `stopPointId` instead
        /// leaves every id empty, which is the bug this type exists to prevent.
        #[test]
        fn the_documented_field_name_would_find_nothing() {
            #[derive(serde::Deserialize)]
            struct Wrong {
                #[serde(rename = "stopPointId", default)]
                #[allow(dead_code)]
                stop_point_id: Option<String>,
            }
            let wrong: Wrong = serde_json::from_str(RESPONSE).expect("parses");
            assert!(
                wrong.stop_point_id.is_none(),
                "if this ever parses, the endpoint changed and the type must be revisited"
            );
        }
    }
}
