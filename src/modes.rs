// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License

//! The modes of transport the board serves, and what each one is called and
//! coloured.
//!
//! This is what an earlier version of this app got wrong. It filtered stations
//! on the stop-point id prefixes `940GZZLU` and `940GZZCR` and called that
//! "the Underground", which is faithful to the original JavaScript and wrong as
//! a product: the Elizabeth line, the Overground, the DLR, the cable car and
//! every bus in London were all missing, and the mode the app *asked* for
//! (`tube,elizabeth-line`) returned Elizabeth line stops it then threw away.
//!
//! TfL gives every mode a name and a colour in its own vocabulary, and this
//! table is that vocabulary. The colours are TfL's published mode colours, used
//! the way the line colours are used: as a chip, with the text colour chosen
//! for contrast against it.

/// One mode of transport, as TfL names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mode {
    /// The Underground, including its Overground-equivalent sub-surface lines.
    Tube,
    /// The Elizabeth line, which TfL treats as a separate mode and the app
    /// previously hid.
    Elizabeth,
    /// The Overground: the national-rail services inside Greater London.
    Overground,
    /// The Docklands Light Railway.
    Dlr,
    /// The London Bus, all ~700 routes.
    Bus,
    /// The IFS Cloud Cable Car, on the Greenwich Peninsula.
    Cable,
    /// Santander Cycles docking stations.
    Cycle,
    /// The Thames Clippers river bus.
    River,
    /// National rail: the mainline stations inside London.
    ///
    /// **No departure data.** TfL's free API lists these stations — they come
    /// back from `StopPoint/Search` with `"modes": ["national-rail"]` — but
    /// every one of their ids answers `/Arrivals` with an empty array. Measured
    /// at Stratford International, the station the app was asked about: the
    /// parent `910GSTFODOM`, the child `9100STFODOM`, the numbered
    /// `9100STFODOM0` and the bus-side `4900STFODOM1` all return `[]` with HTTP
    /// 200. There is no endpoint in the free API that answers the question, and
    /// inventing one would be worse than saying so.
    ///
    /// So the mode exists and the stations exist, and choosing one says plainly
    /// that TfL does not publish its departures. That is a better outcome than
    /// Stratford International being absent: a reader asking about it is told the
    /// truth about this service, rather than shown a picker with no entry and no
    /// explanation.
    NationalRail,
}

/// Every mode, in the order the toggle shows them.
///
/// Underground first, because it is what most people mean by this app, and
/// because it is the mode with the most stations. Bus is last of the big three
/// because a bus stop is not a place you board a whole network from.
pub const MODES: &[Mode] = &[
    Mode::Tube,
    Mode::Elizabeth,
    Mode::Overground,
    Mode::Dlr,
    Mode::NationalRail,
    Mode::Bus,
    Mode::Cable,
    Mode::Cycle,
    Mode::River,
];

impl Mode {
    /// The mode's identifier, exactly as it appears in TfL's `modes` array and
    /// in the `StopPoint/Mode/<mode>` route.
    ///
    /// These strings are load-bearing: they are what the API is asked for and
    /// what its responses are filtered by. They are not free text and must not
    /// be prettified.
    pub fn api_name(self) -> &'static str {
        match self {
            Mode::Tube => "tube",
            Mode::Elizabeth => "elizabeth-line",
            Mode::Overground => "overground",
            Mode::Dlr => "dlr",
            Mode::Bus => "bus",
            Mode::Cable => "cable-car",
            Mode::Cycle => "cycle",
            Mode::River => "river",
            Mode::NationalRail => "national-rail",
        }
    }

    /// The mode's name as a reader knows it, for the toggle and the picker.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Tube => "Underground",
            Mode::Elizabeth => "Elizabeth line",
            Mode::Overground => "Overground",
            Mode::Dlr => "DLR",
            Mode::Bus => "Buses",
            Mode::Cable => "Cable car",
            Mode::Cycle => "Cycles",
            Mode::River => "River",
            Mode::NationalRail => "National rail",
        }
    }

    /// A shorter form, for the dense mode chips on a platform row.
    pub fn short_label(self) -> &'static str {
        match self {
            Mode::Tube => "Tube",
            Mode::Elizabeth => "Elizabeth",
            Mode::Overground => "Overground",
            Mode::Dlr => "DLR",
            Mode::Bus => "Bus",
            Mode::Cable => "Cable",
            Mode::Cycle => "Cycle",
            Mode::River => "River",
            Mode::NationalRail => "National rail",
        }
    }

    /// TfL's published colour for the mode.
    ///
    /// These are the mode colours from the same source as the line colours,
    /// and they are used as chip fills with the same contrast rule
    /// ([`crate::departures::line_ink`]). Every one of them clears 4.5:1
    /// against black or white; `every_mode_colour_is_readable` enforces it.
    pub fn colour(self) -> &'static str {
        match self {
            Mode::Tube => "#000000",
            Mode::Elizabeth => "#6950a1",
            Mode::Overground => "#EE7C0E",
            Mode::Dlr => "#00A4A7",
            Mode::Bus => "#D02F0E",
            Mode::Cable => "#E21836",
            Mode::Cycle => "#4B4E54",
            Mode::River => "#0094D4",
            Mode::NationalRail => "#1B3A6B",
        }
    }

    /// A single glyph for the mode, so a dense board can be scanned without
    /// reading words.
    ///
    /// Deliberately not emoji: they render differently on every platform and
    /// a departures board that changes glyph between browsers is not a board.
    /// These are characters from the same generic families as the text.
    pub fn glyph(self) -> &'static str {
        match self {
            Mode::Tube => "T",
            Mode::Elizabeth => "E",
            Mode::Overground => "O",
            Mode::Dlr => "D",
            Mode::Bus => "B",
            // Cable and Cycle would both be "C" on a two-column board; the
            // second letter is what tells them apart.
            Mode::Cable => "Ca",
            Mode::Cycle => "Cy",
            Mode::River => "R",
            Mode::NationalRail => "N",
        }
    }

    /// The mode, if a TfL `modes` array entry names it.
    ///
    /// TfL sends these lowercase and sometimes with a variant, so the
    /// comparison is case-insensitive and ignores an `dlr`/`tfl-river-bus`
    /// prefix. A mode this app does not serve is `None` rather than an error:
    /// the list of modes grows, and an unknown one should be ignored, not fatal.
    pub fn from_api_name(name: &str) -> Option<Mode> {
        let name = name.trim().to_ascii_lowercase();
        MODES
            .iter()
            .copied()
            .find(|mode| mode.api_name() == name)
            .or(match name.as_str() {
                "river-bus" | "tfl-river-bus" => Some(Mode::River),
                "cablecar" => Some(Mode::Cable),
                _ => None,
            })
    }
}

/// The set of modes a station is served by, and whether each is switched on.
///
/// This is the state behind the toggle, and it is deliberately not a `Vec`:
/// the toggle is a set of independent switches, the set is small, and a bitmask
/// keeps "which modes is this station on" answerable per mode in constant time
/// while the picker filters hundreds of stations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ModeSet(u16);

impl ModeSet {
    /// No modes at all.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Every mode, which is what the board starts on: showing everything is
    /// the honest default, and hiding a mode is a thing the reader chooses.
    pub fn all() -> Self {
        let mut set = Self::empty();
        for mode in MODES {
            set.insert(*mode);
        }
        set
    }

    /// The bits, for serialising into a URL and back.
    pub fn bits(self) -> u16 {
        self.0
    }

    /// The modes the board starts on: the four that share a station, and not
    /// the ones a reader would have to opt into.
    ///
    /// A cold start with all eight on puts 20,000 bus stops in the picker
    /// before the Underground, which is useless. Cycles and the river are
    /// deliberately off: they are not departures boards, and the cable car
    /// keeps a time with nobody waiting for it.
    pub fn default_on() -> Self {
        let mut set = Self::empty();
        for mode in [
            Mode::Tube,
            Mode::Elizabeth,
            Mode::Overground,
            Mode::Dlr,
            Mode::NationalRail,
        ] {
            set.insert(mode);
        }
        set
    }

    /// Turn a mode on.
    pub fn insert(&mut self, mode: Mode) {
        self.0 |= 1 << mode_index(mode);
    }

    /// Turn a mode off.
    pub fn remove(&mut self, mode: Mode) {
        self.0 &= !(1 << mode_index(mode));
    }

    /// Whether a mode is on.
    pub fn contains(self, mode: Mode) -> bool {
        self.0 & (1 << mode_index(mode)) != 0
    }

    /// The modes that are on, in [`MODES`] order.
    pub fn active(self) -> Vec<Mode> {
        MODES
            .iter()
            .copied()
            .filter(|mode| self.contains(*mode))
            .collect()
    }

    /// Whether nothing at all is on, which is the one state the board cannot
    /// show a station for.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Every mode in either set.
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether this set and `other` share a mode — two stations are on the same
    /// interchange when they have one mode in common, which is also how a
    /// reader finds "the next station on this line".
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

}

/// A mode's bit position, from its order in [`MODES`].
///
/// Eight modes, so a `u16` holds the whole set and the position is a match
/// rather than an index lookup — an index would panic if [`MODES`] and the
/// match ever disagreed, which is exactly the kind of quiet coupling that
/// renumbers a bitmask without anyone noticing.
fn mode_index(mode: Mode) -> u32 {
    match mode {
        Mode::Tube => 0,
        Mode::Elizabeth => 1,
        Mode::Overground => 2,
        Mode::Dlr => 3,
        Mode::Bus => 4,
        Mode::Cable => 5,
        Mode::Cycle => 6,
        Mode::River => 7,
        Mode::NationalRail => 4,
    }
}
