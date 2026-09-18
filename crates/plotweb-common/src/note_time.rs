//! When a note happens — the storage shape only.
//!
//! The *calendar* is card 4 of the notes revamp: a per-book base unit plus an ordered
//! stack of divisors, each with a label and a display rule. Nothing here knows what a
//! book's units are called or how to parse "3 Frostmonth"; this module only fixes how a
//! moment is **stored**, so the tree, the timeline and the sort order can be built
//! before any calendar exists.
//!
//! # One number, plus how precisely it is known
//!
//! A [`TimePoint`] is a single signed integer and a precision level. That shape is the
//! whole point:
//!
//! - **Sorting is integer comparison.** Ordering events — the operation the timeline
//!   performs on every note, on every render — never consults the book's calendar.
//! - **An invented calendar is not a special case.** "Year 4, third season, ninth bell"
//!   and "1817-03-04" are the same `i64` seen through different divisor stacks. There is
//!   no branch anywhere for a made-up calendar, because there is nothing to branch on.
//! - **No leap years, ever.** Units are uniform by construction: the divisor stack is a
//!   stack of *divisors*, so a day is exactly 1/365 of a year and a month exactly 1/12.
//!   Real Gregorian is deliberately deferred (see `design/05-notes-build-plan.md`); this
//!   representation cannot accidentally grow it.
//!
//! The number is fixed point rather than a float: equality and ordering must be exact,
//! and a float would make two notes typed as the same moment sort unpredictably.
//!
//! # The tick
//!
//! One base unit is [`TICKS_PER_BASE_UNIT`] ticks. That constant is 31_536_000 —
//! 365 × 24 × 60 × 60 — chosen so that
//!
//! - every divisor the default Gregorian-shaped calendar uses (1/12, 1/365, 1/24, 1/60,
//!   1/60) divides it exactly, so a month, a day, an hour and a minute are whole
//!   numbers of ticks rather than rounded ones, and
//! - in that default calendar a tick is exactly **one second**, which makes stored
//!   values legible in a debugger and in a JSON dump.
//!
//! It is also divisible by 2, 3, 4, 5, 6, 8, 9, 10, 100, 73 and much else, so an
//! invented calendar with a stack like "8 seasons, 40 days, 10 bells" lands on whole
//! ticks too. An `i64` of ticks spans roughly ±292 billion base units, which is more
//! history than any book needs.
//!
//! Note that the tick is *not* the precision: a value is stored in ticks whatever it is
//! known to. "Some time in year 4" is the tick at the start of year 4 with
//! [`TimePoint::precision`] `0`; "the ninth bell of the third day" is a far larger tick
//! with a deeper precision. Precision is what the renderer consults to decide how much
//! of the number to believe and how to word it.

use serde::{Deserialize, Serialize};

/// Ticks in one base calendar unit. See the module docs for why this number.
pub const TICKS_PER_BASE_UNIT: i64 = 31_536_000;

/// A moment on a book's timeline: a count of ticks from the calendar's epoch, and how
/// far down the divisor stack that count is meaningful.
///
/// `Ord` is derived and compares `tick` first, so a `Vec<TimePoint>` sorts
/// chronologically with no calendar in scope. Two points at the same tick order by
/// precision, which keeps the sort total (and puts the vaguer one first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TimePoint {
    /// Signed offset from the book calendar's epoch, in ticks
    /// ([`TICKS_PER_BASE_UNIT`] to a base unit). Negative is before the epoch.
    pub tick: i64,
    /// How deep in the book's divisor stack this value is actually known:
    /// `0` = the base unit alone (the year, in the default calendar), `1` = one divisor
    /// down (the month), `2` = the day, and so on. Card 4 names the levels per book;
    /// nothing here needs to.
    ///
    /// Anything finer than the stated precision is padding, not data: "year 4" is
    /// stored as the first tick of year 4 with precision `0`, and a renderer must not
    /// present that tick as a date.
    #[serde(default)]
    pub precision: u8,
}

impl TimePoint {
    /// A point at the start of base unit `n`, known only to the base unit.
    pub fn base_unit(n: i64) -> Self {
        Self {
            tick: n.saturating_mul(TICKS_PER_BASE_UNIT),
            precision: 0,
        }
    }
}

/// When a note happens.
///
/// An instant is a span with no `end`. A duration carries both ends. `approximate` and
/// `open_ended` are the fuzzy cases the design calls for, and they are flags rather than
/// separate variants because they compose: "roughly from the spring, and still going".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSpan {
    /// When it starts. Carries its own precision — see [`TimePoint`].
    pub start: TimePoint,
    /// When it ends, if it does. `None` means an instant (or, with `open_ended`, a
    /// beginning whose end is not known).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<TimePoint>,
    /// "Around then" — the author is placing this, not dating it. Drawn softly; it must
    /// not read as a measured date.
    #[serde(default)]
    pub approximate: bool,
    /// Runs past whichever end is missing — an exile with no return, a siege still under
    /// way at the last page. Distinct from `end: None`, which is an instant.
    #[serde(default)]
    pub open_ended: bool,
}

impl TimeSpan {
    /// An instant at `start`.
    pub fn at(start: TimePoint) -> Self {
        Self {
            start,
            end: None,
            approximate: false,
            open_ended: false,
        }
    }

    /// The tick a timeline sorts this span by.
    pub fn sort_key(&self) -> i64 {
        self.start.tick
    }
}

/// How one note sits in time relative to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeRelation {
    /// Happens after the other note.
    After,
    /// Happens before the other note.
    Before,
    /// Happens somewhere inside the other note's span.
    ///
    /// **Not** containment in the sense [`crate::Note::event_parent`] means it. This is
    /// the author saying "some time during the siege, I don't know when"; the siege does
    /// not thereby own this note on the ribbon.
    During,
}

/// "After the parley" — a note placed against another note rather than against the
/// calendar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelativeTime {
    pub relation: TimeRelation,
    /// The note this one is placed against.
    pub note_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_divisor_lands_on_a_whole_tick() {
        // A month, a day, an hour, a minute and a second in the Gregorian-shaped
        // default stack. A remainder here would mean a stored date that cannot be typed
        // back in exactly, which is the bug this constant exists to prevent.
        for divisor in [12, 365, 365 * 24, 365 * 24 * 60, 365 * 24 * 60 * 60] {
            assert_eq!(
                TICKS_PER_BASE_UNIT % divisor,
                0,
                "{divisor} does not divide the base unit evenly"
            );
        }
    }

    #[test]
    fn points_sort_by_tick_without_a_calendar() {
        let mut points = vec![
            TimePoint {
                tick: 900,
                precision: 3,
            },
            TimePoint::base_unit(-2),
            TimePoint {
                tick: 900,
                precision: 0,
            },
            TimePoint::base_unit(1),
        ];
        points.sort();
        assert_eq!(
            points,
            vec![
                TimePoint::base_unit(-2),
                TimePoint {
                    tick: 900,
                    precision: 0
                },
                TimePoint {
                    tick: 900,
                    precision: 3
                },
                TimePoint::base_unit(1),
            ]
        );
    }

    #[test]
    fn a_span_with_only_a_start_round_trips_without_the_absent_fields() {
        let span = TimeSpan::at(TimePoint::base_unit(4));
        let json = serde_json::to_string(&span).expect("serialize");
        assert_eq!(
            json, r#"{"start":{"tick":126144000,"precision":0},"approximate":false,"open_ended":false}"#
        );
        assert_eq!(
            serde_json::from_str::<TimeSpan>(&json).expect("deserialize"),
            span
        );
    }

    #[test]
    fn a_point_stored_before_precision_existed_still_reads() {
        let point: TimePoint = serde_json::from_str(r#"{"tick":42}"#).expect("deserialize");
        assert_eq!(point.precision, 0);
    }
}
