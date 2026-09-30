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

/// The stop-point list endpoint. Tube and Elizabeth line, which is what the
/// original app asked for; the Elizabeth line is the one mode that is not
/// `tube` but still answers to the same station ids.
pub const STATIONS_URL: &str = "https://api.tfl.gov.uk/StopPoint/Mode/tube,elizabeth-line";

/// The arrivals endpoint for one station. `{id}` is a stop-point id.
pub const ARRIVALS_URL: &str = "https://api.tfl.gov.uk/StopPoint/{id}/Arrivals";

/// The base the arrivals URL hangs off, so the id can be substituted without
/// pulling the whole template around.
pub const ARRIVALS_BASE: &str = "https://api.tfl.gov.uk/StopPoint/";

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
    #[serde(rename = "stopPointId", default)]
    pub id: String,
    #[serde(rename = "commonName", default)]
    pub name: Option<String>,
}

pub fn station_list(stop_points: Vec<StopPoint>) -> Vec<Station> {
    let mut stations: Vec<Station> = stop_points
        .into_iter()
        .filter(|point| is_station(&point.id, point.name.as_deref()))
        .map(|point| Station {
            id: point.id,
            // The filter has just established this is `Some` and non-empty.
            name: point.name.unwrap_or_default(),
        })
        .collect();
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

/// Names containing digits sort the way a reader reads them, not the way a
/// byte comparison does: Bond Street before Bond Street... nothing here, but
/// 100 goes after 99.
#[test]
fn the_station_sort_does_not_reorder_equal_prefixes() {
    let listed = vec![
        station("940GZZLUAAA", "Acton Town"),
        station("940GZZLUAAA", "Acton Town"),
    ];
    assert_eq!(station_list(listed).len(), 2, "duplicates are kept");
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
#[test]
fn the_wire_formats_still_parse() {
    let stop_points: serde_json::Value = serde_json::from_str(
        r#"{
          "$type": "TflStopPoint",
          "stopPoints": [
            {"$type":"TflStopPoint","stopPointId":"940GZZLUKSX","commonName":"King's Cross St. Pancras","icsCode":"gc"},
            {"$type":"TflStopPoint","stopPointId":"940GZZLULST","commonName":"Liverpool Street","icsCode":"li"},
            {"$type":"TflStopPoint","stopPointId":"940GZZCRBOW","commonName":"Bow Road","icsCode":"bwr"},
            {"$type":"TflStopPoint","stopPointId":"4900000934","commonName":"Tottenham Court Road"},
            {"$type":"TflStopPoint","stopPointId":"940GZZLUKSX:1","commonName":null},
            {"$type":"TflStopPoint","stopPointId":"940GZZLUBZW","commonName":"Brixton"}
          ]
        }"#,
    )
    .expect("stop point list");
    let list: Vec<StopPoint> = stop_points["stopPoints"]
        .as_array()
        .expect("stopPoints array")
        .iter()
        .map(|sp| StopPoint {
            id: sp["stopPointId"].as_str().unwrap_or_default().to_string(),
            name: sp["commonName"].as_str().map(str::to_string),
        })
        .collect();
    let kept = station_list(list);
    let names: Vec<&str> = kept.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["Bow Road", "Brixton", "King's Cross St. Pancras", "Liverpool Street"]);

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

}
