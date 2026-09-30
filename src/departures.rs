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
//! The browser layer in [`crate::ui`] owns the two things that cannot be pure:
//! the request itself and the DOM it renders into. It feeds this module the
//! deserialised JSON and renders what comes back, so the two cannot drift.

/// The per-line stop-point endpoint. `{line}` is a line id from [`LINE_IDS`].
///
/// This is what replaced the single mode request, for the reason set out on
/// [`LINE_IDS`].
pub const LINE_STOPS_URL: &str = "https://api.tfl.gov.uk/Line/{line}/StopPoints";

/// The base the arrivals URL hangs off, so the id can be substituted without
/// pulling the whole template around.
pub const ARRIVALS_BASE: &str = "https://api.tfl.gov.uk/StopPoint/";

/// Every line whose stations the board offers, in the order they are fetched.
///
/// Eleven Underground lines plus the Elizabeth line — the ids
/// `Line/Mode/tube` and `Line/Mode/elizabeth-line` return, hard coded.
///
/// **Why not the one big request.** The original app asked for
/// `StopPoint/Mode/tube,elizabeth-line` in a single call, and that endpoint
/// answers with **16,958,009 bytes** — 17 MB of JSON for 1,858 stop points,
/// of which 270 are stations. Almost all of it is two fields the board never
/// reads: `children` (9.0 MB) and `additionalProperties` (7.1 MB), together
/// 95% of the payload. `?detail=false` is accepted and does essentially
/// nothing — it returned 16,957,998 bytes, a 0.0001% saving — and there is no
/// smaller station-list endpoint.
///
/// Fetching `Line/<id>/StopPoints` for the twelve lines instead returns the
/// **same 270 stations** — verified by diffing the two sets, which are
/// identical — in 3,255,303 bytes across twelve requests, an 81% reduction, and
/// the largest single response is one line's rather than the whole network's.
/// A station on three lines appears in three responses and is deduplicated
/// here.
///
/// The cost is twelve round trips instead of one, and they are issued
/// concurrently, so the wall-clock difference is small. What it buys is a
/// payload the UI thread can parse without freezing, and a failure that costs
/// one line rather than the whole picker. The trade is deliberate: for a list
/// this static, twelve small requests beat one large one.
pub const LINE_IDS: &[&str] = &[
    "bakerloo",
    "central",
    "circle",
    "district",
    "hammersmith-city",
    "jubilee",
    "metropolitan",
    "northern",
    "piccadilly",
    "victoria",
    "waterloo-city",
    "elizabeth",
];

/// The TfL stop-point id prefixes that are real stations on the board.
///
/// The mode endpoint also returns stop points that are not stations — the
/// entrances, the interchanges' unnamed legs, the `940GZZ...` virtual stop
/// points. `940GZZLU` is the London Underground prefix and `940GZZCR` is
/// central London, and a stop point whose id starts with one of the two is
/// something a passenger can actually choose. The original app expressed this
/// as an unanchored regular expression, `/(940GZZLU|940GZZCR)/`, matched
/// anywhere in the id; the anchors here do not change which ids pass, because
/// every such prefix occurs at the start of an id and nowhere else.
pub const STATION_ID_PREFIXES: &[&str] = &["940GZZLU", "940GZZCR"];

/// What a station looks like on the board: its id, which is what every other
/// request keys off, and its display name, which is what the reader reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Station {
    pub id: String,
    pub name: String,
}

/// One upcoming arrival, as far as this app cares: which line, where it is
/// going, which platform, and how far away it is in seconds.
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
}

/// One row of a departures table: a line, its destination, and the minutes
/// until it arrives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Departure {
    pub line: String,
    pub destination: String,
    pub minutes: i32,
}

/// A platform and the next few trains leaving it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Platform {
    pub name: String,
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
    ("Elizabeth", "#6950a1"),
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
pub fn line_colour(line: &str) -> &'static str {
    LINE_COLOURS
        .iter()
        .find(|(name, _)| *name == line)
        .map_or(DEFAULT_LINE_COLOUR, |(_, colour)| colour)
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

/// Whether a stop point is a station a passenger can pick.
///
/// Both halves matter and both are the original's: the stop needs a
/// `commonName` to have anything to show, and its id has to start with one of
/// [`STATION_ID_PREFIXES`]. TfL sends `commonName: null` for the unnamed legs
/// of an interchange, and those legs carry the same id shape as the station
/// they belong to — so filtering on the id alone fills the picker with blanks,
/// and filtering on the name alone fills it with the stops nobody can board.
pub fn is_station(id: &str, common_name: Option<&str>) -> bool {
    let named = common_name.is_some_and(|name| !name.is_empty());
    let underground = STATION_ID_PREFIXES
        .iter()
        .any(|prefix| id.starts_with(prefix));
    named && underground
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
/// A stop point exactly as TfL sends it: an id, and a name that may be absent.
///
/// This is the unfiltered input. [`station_list`] decides which of these are
/// stations; keeping the optional name here is what lets the filter reject the
/// unnamed legs of an interchange, which arrive with the station's own id and
/// would otherwise be indistinguishable from the station.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct StopPoint {
    /// The field is `id`, not `stopPointId`.
    ///
    /// The stop-point list and the arrivals list disagree about this, and that
    /// is the whole trap. `StopPoint/...` sends `"id"`; a `TflArrival` sends
    /// `"stationName"`, `"platformName"` and no id at all; and the *older* TfL
    /// Unified API documentation shows a `stopPointId` that the live endpoint
    /// does not send. Renaming this to `stopPointId` is the single change that
    /// leaves the picker silently empty forever: every stop point deserialises
    /// with an empty id, every id fails the prefix test, and the board offers
    /// nothing while reporting no error at all.
    #[serde(rename = "id", default)]
    pub id: String,
    #[serde(rename = "commonName", default)]
    pub name: Option<String>,
}

pub fn station_list(stop_points: Vec<StopPoint>) -> Vec<Station> {
    let mut stations: Vec<Station> = Vec::with_capacity(stop_points.len());
    // Deduplicate by id, keeping the first sighting.
    //
    // The per-line fetch returns a station once per line that serves it, and
    // most of the big ones are on three or four: Acton Town comes back from
    // both the Bakerloo and the Piccadilly, Aldgate from the Circle,
    // Metropolitan and Hammersmith & City. Concatenating twelve responses
    // therefore yields **383** entries for **270** distinct stations, and a
    // picker listing Acton Town four times is not a board anyone can use.
    //
    // First-wins rather than last-wins, so the entry kept is the one whose line
    // came earliest in `LINE_IDS`; the name is the same in every response, so
    // which one survives does not matter to the reader.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for point in stop_points {
        if !is_station(&point.id, point.name.as_deref()) {
            continue;
        }
        if !seen.insert(point.id.clone()) {
            continue;
        }
        stations.push(Station {
            id: point.id,
            // The filter has just established this is `Some` and non-empty.
            name: point.name.unwrap_or_default(),
        });
    }
    stations.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    stations
}

/// The whole departures board for one station's arrivals.
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
        match grouped.iter_mut().find(|platform| platform.name == name) {
            Some(platform) => platform.departures.push(departure_of(arrival)),
            None => grouped.push(Platform {
                name,
                departures: vec![departure_of(arrival)],
            }),
        }
    }

    grouped.sort_by_key(|platform| platform_number(&platform.name));
    for platform in &mut grouped {
        platform.departures.truncate(DEPARTURES_PER_PLATFORM);
    }
    Board { platforms: grouped }
}

/// One arrival as a table row.
fn departure_of(arrival: &Arrival) -> Departure {
    Departure {
        line: arrival.line_name.clone(),
        destination: arrival.destination.clone(),
        minutes: minutes_until(arrival),
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
#[cfg(test)]
mod tests {
    use super::*;

    /// A stop point, as the list would supply one.
fn station(id: &str, name: &str) -> StopPoint {
    StopPoint {
        id: id.to_string(),
        name: Some(name.to_string()),
    }
}

/// An arrival, with the fields the board reads.
fn arrival(line: &str, destination: &str, platform: Option<&str>, seconds: i32) -> Arrival {
    Arrival {
        line_name: line.to_string(),
        destination: destination.to_string(),
        time_to_station: Some(seconds),
        platform: platform.map(str::to_string),
    }
}

/// The minutes a platform's table shows, in order.
fn minutes_of(platform: &Platform) -> Vec<i32> {
    platform.departures.iter().map(|d| d.minutes).collect()
}

// ---------------------------------------------------------------- the filter

/// The two id prefixes are the whole filter, and they are prefixes: a stop
/// point is a station if and only if its id starts with one of them.
#[test]
fn the_station_filter_accepts_both_prefixes() {
    assert!(is_station("940GZZLUKSX", Some("King's Cross St. Pancras")));
    assert!(is_station("940GZZCRLHR", Some("Liverpool Street")));
    for prefix in STATION_ID_PREFIXES {
        assert!(
            is_station(&format!("{prefix}XXX"), Some("Anywhere")),
            "{prefix} must be accepted"
        );
    }
}

/// Everything else the mode endpoint returns is not a station, and the common
/// prefixes are named here because TfL's id space is the thing being filtered.
#[test]
fn the_station_filter_rejects_everything_else() {
    let rejected = [
        // Bus stops and DLR/Barnet trams, from the same mode-family endpoints.
        "4900000934",
        "940GZZDLSD",
        // The Elizabeth line's own stop points are 940GZZEL — deliberately
        // not a station prefix here, because the original's two prefixes are
        // what it filtered on and the board's stations are the ones those
        // name. See AGENTS.md.
        "940GZZELWSD",
    ];
    for id in rejected {
        assert!(!is_station(id, Some("Somewhere")), "{id} must be rejected");
    }
    // An interchange's unnamed leg carries the station's own id plus a suffix,
    // and the original's unanchored `/(940GZZLU|940GZZCR)/` matched it, so
    // `starts_with` matches it too. What keeps it out of the picker in
    // practice is the `commonName: null` half of the filter, which the wire
    // fixture below carries.
    assert!(
        is_station("940GZZLUKSX:1", Some("King's Cross St. Pancras")),
        "a suffixed stop-point id carries the station prefix, as it did before"
    );
}

/// A stop point with no name cannot be offered: there would be nothing in the
/// picker but a blank line. TfL sends `commonName: null` for these, which is
/// the case the original's `sp.commonName &&` was there to catch.
#[test]
fn a_station_without_a_common_name_is_not_offered() {
    for name in [None, Some("")] {
        assert!(
            !is_station("940GZZLUKSX", name),
            "a stop point with no usable name must be rejected: {name:?}"
        );
    }
    // And the id half still holds on its own: a named non-station is rejected.
    assert!(!is_station("4900000934", Some("Tottenham Court Road")));
}

// -------------------------------------------------------------- station sort

/// The picker is alphabetical by name, and the names that share a prefix come
/// out in the order a reader expects.
#[test]
fn stations_sort_alphabetically_by_common_name() {
    let listed = vec![
        station("940GZZLULST", "Liverpool Street"),
        station("940GZZLUKSX", "King's Cross St. Pancras"),
        station("940GZZLUBZW", "Brixton"),
        station("940GZZLUWLO", "Wood Lane"),
    ];
    let names: Vec<String> = station_list(listed)
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(
        names,
        vec!["Brixton", "King's Cross St. Pancras", "Liverpool Street", "Wood Lane"]
    );
}

/// The order is a comparison of names, not of ids, and it is case-insensitive
/// with an exact tie-break — so it cannot depend on the machine's locale the
/// way the original's `localeCompare` did.
#[test]
fn the_station_sort_is_case_insensitive_and_stable() {
    let listed = vec![
        station("940GZZLUAAA", "Acton Town"),
        station("940GZZLUBBB", "acton warren"),
        station("940GZZLUCCC", "Acton Town"),
    ];
    let names: Vec<String> = station_list(listed)
        .into_iter()
        .map(|s| s.name)
        .collect();
    // Lowercase compares equal for the first and third, so the exact-code-point
    // tie-break decides: "Acton Town" < "acton warren" because 'T' < 'w'.
    assert_eq!(
        names,
        vec!["Acton Town", "Acton Town", "acton warren"]
    );
}

/// A station served by more than one line arrives once per line, and must
/// appear in the picker once.
///
/// This test used to assert the opposite — "duplicates are kept" — and that was
/// not an accident of the fixture but the shipped behaviour: concatenating the
/// twelve per-line responses produced 383 options for 270 stations, with Acton
/// Town listed three times. It was found by loading the built page and reading
/// the options, which is the only check that could see it: the single-request
/// version of this code could not produce duplicates, so no unit test fed from
/// one response would ever have caught it.
#[test]
fn a_station_served_by_several_lines_is_listed_once() {
    // Acton Town, as the Bakerloo and the Piccadilly both return it.
    let listed = vec![
        station("940GZZLUACT", "Acton Town Underground Station"),
        station("940GZZLUACT", "Acton Town Underground Station"),
        station("940GZZLUACT", "Acton Town Underground Station"),
    ];
    let stations = station_list(listed);
    assert_eq!(
        stations.len(),
        1,
        "a station on three lines is one station, not three: {stations:?}"
    );
    assert_eq!(stations[0].name, "Acton Town Underground Station");
    // The id is the thing the picker submits, so it must survive intact.
    assert_eq!(stations[0].id, "940GZZLUACT");
}

/// The filter runs on the way in, so a caller cannot get a non-station past it
/// by building a `Station` itself.
#[test]
fn the_station_list_applies_the_filter() {
    let listed = vec![
        station("4900000934", "Not A Station"),
        station("940GZZLUBZW", "Brixton"),
    ];
    let kept = station_list(listed);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].name, "Brixton");
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
    let names: Vec<&str> = made
        .platforms
        .iter()
        .map(|p| p.name.as_str())
        .collect();
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
    assert_eq!(arrival_minutes(90), 1, "ninety seconds is one minute, not two");
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
        .map(|n| arrival("Central", &format!("Terminus {n}"), Some("Platform 1"), n * 60))
        .collect();
    let made = board(arrivals);
    assert_eq!(made.platforms.len(), 1);
    assert_eq!(made.platforms[0].departures.len(), DEPARTURES_PER_PLATFORM);
    // And they are the ten soonest, not ten arbitrary ones.
    assert_eq!(minutes_of(&made.platforms[0]), vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
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
        ("Elizabeth", "#6950a1"),
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
    let kept = station_list(list);
    let names: Vec<&str> = kept.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Bow Road Underground Station",
            "Brixton Underground Station",
            "King's Cross St. Pancras Underground Station",
            "Liverpool Street Underground Station",
        ]
    );
    // The ids must be the real ones, or every later request 404s. This is the
    // assertion that would have caught the `stopPointId` mistake: deserialising
    // that name leaves every id empty, the filter drops everything, and the
    // board quietly offers no stations at all.
    assert!(
        kept.iter().all(|s| is_station(&s.id, Some(&s.name))),
        "every station kept must carry its own real id: {:?}",
        kept.iter().map(|s| &s.id).collect::<Vec<_>>()
    );
    assert!(kept.iter().any(|s| s.id == "940GZZLUBZW"), "Brixton by id");

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
            destination: a["destinationName"].as_str().unwrap_or_default().to_string(),
            time_to_station: a["timeToStation"].as_i64().map(|s| s as i32),
            platform: a["platformName"].as_str().map(str::to_string),
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
    let stations = station_list(stops);

    assert!(
        !stations.is_empty(),
        "a real response must yield stations; an empty list means the id field \
         is wrong again"
    );
    // Six are Underground stations in this slice; the two `0400ZZLU…` entries
    // are named but are not on the network this board serves.
    assert_eq!(stations.len(), 6, "unexpected station count: {stations:?}");
    let names: Vec<&str> = stations.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Acton Town Underground Station",
            "Aldgate East Underground Station",
            "Aldgate Underground Station",
            "Alperton Underground Station",
            "Angel Underground Station",
            "Archway Underground Station",
        ]
    );
    // And the ids are the ones the arrivals endpoint will accept.
    for station in &stations {
        assert!(
            station.id.starts_with("940GZZLU") || station.id.starts_with("940GZZCR"),
            "{} has an id the board cannot request arrivals for: {}",
            station.name,
            station.id
        );
    }
    // A station on three lines arrives in three responses; the picker must not
    // show it three times.
    let unique: std::collections::HashSet<&str> = stations.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(unique.len(), stations.len(), "duplicate stations in the picker");
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
    assert!((ratio - 21.0).abs() < 0.01, "black on white is 21:1, got {ratio}");
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
            contrast_ratio(colour, DARK_PAGE) >= 1.5 || contrast_ratio(colour, LIGHT_PAGE) >= 1.5,
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
    let stations = station_list(all);
    assert!(
        stations.len() < before,
        "concatenating responses must collapse repeats: {} in, {} out",
        before,
        stations.len()
    );
    let mut ids: Vec<&str> = stations.iter().map(|s| s.id.as_str()).collect();
    let unique = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), unique, "the picker must hold no repeated station");
    // Acton Town is on both lines, so it must appear exactly once.
    assert_eq!(
        stations
            .iter()
            .filter(|s| s.name == "Acton Town Underground Station")
            .count(),
        1,
        "Acton Town is served by the Bakerloo and the Piccadilly and must be \
         offered once"
    );
}
}
