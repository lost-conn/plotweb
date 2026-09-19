//! A book's calendar: how a stored tick reads, and how a typed date becomes one.
//!
//! [`crate::note_time`] fixed the *storage* shape — one `i64` tick plus a precision depth
//! — so that ordering never needs a calendar. This module is the other half: a per-book
//! **base unit plus an ordered stack of divisors**, each with a label and a rule for when
//! the timeline shows it.
//!
//! ```text
//!   The Accord            Defined as       Written as     Shown in timeline when
//!   Year        (0)       base             "yr {n}"       always
//!   Season      (1)       1/4 Year         ", {name}"     span < 40 Years
//!   Day         (2)       1/320 Year       ", day {n}"    span < 2 Years
//!   Bell        (3)       1/16 Day         ", bell {n}"   span < 20 Days
//! ```
//!
//! [`TimePoint::precision`] is an index into that list: precision `2` means "known to
//! the Day" in the Accord, and "known to the Day" in the default calendar too — the
//! calendar is what *names* the depth, per book.
//!
//! # Every unit is a uniform fraction of the base unit
//!
//! A unit is `1/per` of an earlier unit (`of`), so every unit is an exact rational
//! fraction of the base: a Gregorian-shaped Day is `1/365` Year, an Accord Bell is
//! `1/(16·320)` Year. There is **no leap year, no unequal month, no special case of
//! any kind** — `design/05-notes-build-plan.md` deferred real Gregorian on purpose, and
//! this representation cannot grow it by accident.
//!
//! Units need not nest: a Gregorian-shaped Month is `1/12` Year and a Day is `1/365`
//! Year, and 365 is not a multiple of 12. So when a date is *written*, each part is
//! counted inside the part before it by one uniform rule — **a unit belongs to the
//! coarser unit its start falls in**. For the default calendar that rule alone produces
//! months of 30 and 31 days (and a February of 30), with nothing that knows what a
//! month is. For the Accord, where everything divides, it is ordinary nesting.
//!
//! # Ticks
//!
//! A unit's start is `floor(n · TICKS / per_base)`. Where the division is exact — every
//! unit of the default calendar, down to the second — that is exact. Where it is not (an
//! Accord Bell is 6159.375 ticks) the start rounds down to a whole tick, which is
//! harmless: [`Calendar::index_at`] inverts it exactly, so a typed date reads back as
//! typed, and ordering is still plain integer comparison. The only constraint is that
//! the finest unit is at least one tick long, which [`Calendar::validate`] enforces.

use serde::{Deserialize, Serialize};

use crate::note_time::{TICKS_PER_BASE_UNIT, TimePoint};

const TICKS: i128 = TICKS_PER_BASE_UNIT as i128;

/// The most units a calendar may have. Precision is a `u8`, and nothing a novel needs
/// comes close; the cap exists so a malformed calendar cannot ask for absurd depth.
pub const MAX_UNITS: usize = 8;

/// A book's calendar. `Book::calendar` is `None` for every book that never opened the
/// calendar screen, which reads as [`Calendar::default`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calendar {
    /// What the author calls it ("The Accord"). Display only.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Coarsest first. `units[0]` is the base unit; every later unit is `1/per` of an
    /// earlier one, and each is strictly finer than the one before it.
    pub units: Vec<CalendarUnit>,
}

/// One level of the divisor stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarUnit {
    /// "Year", "Season", "Bell". Also what a precision depth is called in the UI.
    pub name: String,
    /// How many of this unit make one [`CalendarUnit::of`]. Ignored for the base unit.
    #[serde(default = "one")]
    pub per: u32,
    /// Index of the earlier unit this one divides. Ignored for the base unit.
    #[serde(default)]
    pub of: usize,
    /// A name for each value, in order ("wet", "dry", …). Empty means values are
    /// written as numbers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    /// How one value is written: `{n}` is its number, `{name}` its name (or its number
    /// when it has none). A piece starting with punctuation — `", day {n}"` — is joined
    /// to the previous piece without a space.
    #[serde(default = "default_format")]
    pub format: String,
    /// The number the first value is counted as: `1` for "day 1", `0` for "0h". For the
    /// base unit, the number the epoch's unit is shown as.
    #[serde(default)]
    pub first: i64,
    /// When the timeline shows this unit: only while the visible span is shorter than
    /// this. `None` is always. Stored and edited here; card 5's timeline reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown_below: Option<ShownBelow>,
}

/// "span < 40 Years": `count` of the unit at index `unit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShownBelow {
    pub count: u32,
    pub unit: usize,
}

fn one() -> u32 {
    1
}

fn default_format() -> String {
    "{n}".to_string()
}

impl CalendarUnit {
    fn new(name: &str, per: u32, of: usize, format: &str, first: i64) -> Self {
        Self {
            name: name.to_string(),
            per,
            of,
            names: Vec::new(),
            format: format.to_string(),
            first,
            shown_below: None,
        }
    }
}

impl Default for Calendar {
    /// Gregorian-shaped: Year base, Month 1/12, Day 1/365 Year, Hour 1/24 Day. A
    /// contemporary book gets this without ever seeing the calendar screen.
    fn default() -> Self {
        let mut month = CalendarUnit::new("Month", 12, 0, "{name}", 1);
        month.names = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        month.shown_below = Some(ShownBelow { count: 10, unit: 0 });
        let mut day = CalendarUnit::new("Day", 365, 0, "{n}", 1);
        day.shown_below = Some(ShownBelow { count: 1, unit: 0 });
        let mut hour = CalendarUnit::new("Hour", 24, 2, "{n}h", 0);
        hour.shown_below = Some(ShownBelow { count: 3, unit: 2 });
        Self {
            name: String::new(),
            units: vec![CalendarUnit::new("Year", 1, 0, "{n}", 0), month, day, hour],
        }
    }
}

fn floor_div(a: i128, b: i128) -> i128 {
    a.div_euclid(b)
}

fn ceil_div(a: i128, b: i128) -> i128 {
    -((-a).div_euclid(b))
}

/// Punctuation that attaches to the piece before it rather than after a space.
fn attaches(piece: &str) -> bool {
    piece
        .chars()
        .next()
        .is_some_and(|c| matches!(c, ',' | ';' | ':' | '·' | '/' | '.' | ')'))
}

impl Calendar {
    /// The calendar a book should be read through: its own if it has a usable one,
    /// otherwise the default. A stored calendar this build rejects must not make every
    /// date in the book unreadable, so it degrades rather than failing.
    pub fn effective(stored: Option<&Calendar>) -> Calendar {
        match stored {
            Some(c) if c.validate().is_ok() => c.clone(),
            _ => Calendar::default(),
        }
    }

    /// Every rule the arithmetic below relies on. A calendar that passes can format and
    /// parse any tick without panicking or overflowing.
    pub fn validate(&self) -> Result<(), String> {
        if self.units.is_empty() {
            return Err("A calendar needs at least a base unit.".into());
        }
        if self.units.len() > MAX_UNITS {
            return Err(format!("A calendar can have at most {MAX_UNITS} units."));
        }
        let mut per_base: Vec<i128> = Vec::with_capacity(self.units.len());
        for (i, unit) in self.units.iter().enumerate() {
            let name = unit.name.trim();
            if name.is_empty() {
                return Err(format!("Unit {} needs a name.", i + 1));
            }
            if self.units[..i]
                .iter()
                .any(|u| u.name.trim().eq_ignore_ascii_case(name))
            {
                return Err(format!("Two units are called “{name}”."));
            }
            if !unit.format.contains("{n}") && !unit.format.contains("{name}") {
                return Err(format!(
                    "“{name}” is written as “{}”, which has no {{n}} or {{name}} in it.",
                    unit.format
                ));
            }
            if unit.names.iter().any(|n| n.trim().is_empty()) {
                return Err(format!("“{name}” has an empty value name."));
            }
            if i == 0 {
                per_base.push(1);
                continue;
            }
            if unit.of >= i {
                return Err(format!("“{name}” must divide a unit listed above it."));
            }
            if unit.per < 2 {
                return Err(format!("“{name}” must be at least 1/2 of what it divides."));
            }
            let d = per_base[unit.of]
                .checked_mul(unit.per as i128)
                .filter(|d| *d <= TICKS)
                .ok_or_else(|| format!("“{name}” is finer than this calendar can store."))?;
            if d <= per_base[i - 1] {
                return Err(format!(
                    "“{name}” must be smaller than “{}”, the unit above it.",
                    self.units[i - 1].name
                ));
            }
            per_base.push(d);
        }
        for unit in &self.units {
            if let Some(rule) = unit.shown_below {
                if rule.unit >= self.units.len() {
                    return Err(format!("“{}” is shown by a unit that does not exist.", unit.name));
                }
                if rule.count == 0 {
                    return Err(format!("“{}” is shown below a span of zero.", unit.name));
                }
            }
        }
        Ok(())
    }

    /// Index of the deepest unit.
    pub fn deepest(&self) -> u8 {
        self.units.len().saturating_sub(1) as u8
    }

    /// The level a precision reads at: its own, or the deepest this calendar has.
    fn level(&self, precision: u8) -> usize {
        (precision as usize).min(self.units.len().saturating_sub(1))
    }

    /// What a precision depth is called in this book — "Day", "Bell".
    pub fn precision_name(&self, precision: u8) -> &str {
        self.units
            .get(self.level(precision))
            .map(|u| u.name.as_str())
            .unwrap_or("")
    }

    /// How many of unit `level` make one base unit — exact.
    fn per_base(&self, level: usize) -> i128 {
        if level == 0 {
            return 1;
        }
        let unit = &self.units[level];
        // `of < level` on a validated calendar; clamp so an unvalidated one cannot recurse
        // forever.
        let of = unit.of.min(level - 1);
        (unit.per.max(1) as i128) * self.per_base(of)
    }

    /// The tick unit `n` (counted from the epoch) of unit `level` starts on.
    pub fn start_tick(&self, level: usize, n: i64) -> i64 {
        let t = floor_div(n as i128 * TICKS, self.per_base(level));
        t.clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }

    /// Which unit `level` (counted from the epoch) holds `tick`: the last one starting
    /// at or before it. Exact inverse of [`Calendar::start_tick`].
    pub fn index_at(&self, level: usize, tick: i64) -> i64 {
        let n = ceil_div((tick as i128 + 1) * self.per_base(level), TICKS) - 1;
        n.clamp(i64::MIN as i128, i64::MAX as i128) as i64
    }

    /// How long one of unit `level` is, in ticks — exact as a rational, so a float.
    /// What the timeline measures a visible span against for each unit's "shown in
    /// timeline when" rule.
    pub fn unit_ticks(&self, level: usize) -> f64 {
        let level = level.min(self.units.len().saturating_sub(1));
        TICKS as f64 / self.per_base(level) as f64
    }

    /// Whether the timeline shows unit `level` while `span_ticks` of time are on screen:
    /// the unit's own [`CalendarUnit::shown_below`] rule, or always when it has none.
    pub fn shown_at(&self, level: usize, span_ticks: f64) -> bool {
        let Some(unit) = self.units.get(level) else {
            return false;
        };
        match unit.shown_below {
            None => true,
            Some(rule) => span_ticks < rule.count as f64 * self.unit_ticks(rule.unit),
        }
    }

    /// Only the finest part of a point, as written, without the punctuation that joins
    /// it to the part before — `"day 12"`, `"Mar"`, `"13h"`. A timeline tick label,
    /// where the coarser parts are already on the tick to its left.
    pub fn format_part(&self, point: &TimePoint) -> String {
        let parts = self.components(point);
        let Some(&value) = parts.last() else {
            return String::new();
        };
        let piece = self.render_part(parts.len() - 1, value);
        piece
            .trim()
            .trim_start_matches(|c: char| attaches(&c.to_string()) || c.is_whitespace())
            .to_string()
    }

    /// The first unit `level` whose start falls inside unit `parent_n` of `level - 1`.
    fn first_child(&self, level: usize, parent_n: i128) -> i128 {
        ceil_div(parent_n * self.per_base(level), self.per_base(level - 1))
    }

    /// How many of unit `level` start inside unit `parent_n` of `level - 1`.
    fn child_count(&self, level: usize, parent_n: i128) -> i128 {
        self.first_child(level, parent_n + 1) - self.first_child(level, parent_n)
    }

    /// The parts of a point, coarsest first, down to its precision: the base unit's
    /// absolute index, then each later part counted from zero inside the part before it.
    pub fn components(&self, point: &TimePoint) -> Vec<i64> {
        if self.units.is_empty() {
            return Vec::new();
        }
        let p = self.level(point.precision);
        let mut n = vec![0i128; p + 1];
        n[p] = self.index_at(p, point.tick) as i128;
        for l in (0..p).rev() {
            // The part holding the *start* of the finer part — "a unit belongs to the
            // coarser unit its start falls in".
            n[l] = floor_div(n[l + 1] * self.per_base(l), self.per_base(l + 1));
        }
        let mut out = Vec::with_capacity(p + 1);
        out.push(n[0] as i64);
        for l in 1..=p {
            out.push((n[l] - self.first_child(l, n[l - 1])) as i64);
        }
        out
    }

    /// One unit's part as it is written.
    fn render_part(&self, level: usize, value: i64) -> String {
        let unit = &self.units[level];
        let shown = value.saturating_add(unit.first);
        let name = usize::try_from(value)
            .ok()
            .and_then(|i| unit.names.get(i))
            .cloned()
            .unwrap_or_else(|| shown.to_string());
        unit.format
            .replace("{name}", &name)
            .replace("{n}", &shown.to_string())
    }

    /// A point as the author would write it, to its own precision and no further —
    /// "year 4" never reads as a day.
    pub fn format_point(&self, point: &TimePoint) -> String {
        let mut out = String::new();
        for (level, value) in self.components(point).into_iter().enumerate() {
            let piece = self.render_part(level, value);
            let piece = piece.trim();
            if !out.is_empty() && !attaches(piece) {
                out.push(' ');
            }
            out.push_str(piece);
        }
        out
    }

    /// Words that carry no value: unit names ("day 12"), their plurals, and the literal
    /// text of every format ("yr", "of", "h").
    fn filler_words(&self) -> Vec<String> {
        let mut words = Vec::new();
        for unit in &self.units {
            for w in words_of(&unit.name) {
                words.push(format!("{w}s"));
                words.push(w);
            }
            let literal = unit.format.replace("{name}", " ").replace("{n}", " ");
            words.extend(words_of(&literal));
        }
        words
    }

    /// Read a typed point: its parts in order, coarsest first, each a number or one of
    /// its unit's names. How many parts were given is the precision.
    ///
    /// `"1817 Mar 4"`, `"1817 march 4"` and `"1817 3 4"` are the same Day; in the Accord,
    /// `"yr 1206, dry, day 12"`. A part out of range for the part before it — a Day 31 in
    /// a 30-day Month — is refused rather than rolled over, because a rolled-over date
    /// is one the author did not type.
    pub fn parse_point(&self, text: &str) -> Result<TimePoint, String> {
        if self.units.is_empty() {
            return Err("This book has no calendar.".into());
        }
        let tokens = tokenize(text);
        let fillers = self.filler_words();
        let mut values: Vec<i64> = Vec::new();
        let mut i = 0;
        while i < tokens.len() {
            let level = values.len();
            match &tokens[i] {
                Token::Num(v) => {
                    let Some(unit) = self.units.get(level) else {
                        return Err(self.too_deep());
                    };
                    values.push(v.saturating_sub(unit.first));
                    i += 1;
                }
                Token::Word(w) => {
                    if let Some(unit) = self.units.get(level)
                        && let Some((index, used)) = match_name(&unit.names, &tokens[i..])
                    {
                        values.push(index as i64);
                        i += used;
                        continue;
                    }
                    if fillers.iter().any(|f| f == w) {
                        i += 1;
                        continue;
                    }
                    return Err(match self.units.get(level) {
                        Some(unit) if !unit.names.is_empty() => {
                            format!("“{w}” isn’t a {}.", unit.name)
                        }
                        Some(unit) => format!("Expected a number for the {}, not “{w}”.", unit.name),
                        None => self.too_deep(),
                    });
                }
            }
        }
        if values.is_empty() {
            return Err(format!("A date needs at least a {}.", self.units[0].name));
        }

        // Walk down, counting each part inside the part before it.
        let mut n: i128 = values[0] as i128;
        for (level, &v) in values.iter().enumerate().skip(1) {
            let count = self.child_count(level, n);
            if v < 0 || v as i128 >= count {
                let unit = &self.units[level];
                return Err(format!(
                    "There is no {} {} there — that {} has {}.",
                    unit.name,
                    v.saturating_add(unit.first),
                    self.units[level - 1].name,
                    count
                ));
            }
            n = self.first_child(level, n) + v as i128;
        }
        let level = values.len() - 1;
        let n = i64::try_from(n).map_err(|_| "That date is out of range.".to_string())?;
        Ok(TimePoint {
            tick: self.start_tick(level, n),
            precision: level as u8,
        })
    }

    fn too_deep(&self) -> String {
        format!(
            "Too many parts — this calendar stops at the {}.",
            self.units.last().map(|u| u.name.as_str()).unwrap_or("")
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(i64),
    Word(String),
}

/// Numbers and words; everything else separates them. A `-` is a minus sign only where
/// it starts a number that nothing precedes on its side — `-40`, `yr -40` — so `4-7`
/// never reads as two numbers with a sign.
fn tokenize(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev_is_gap = i == 0 || !chars[i - 1].is_alphanumeric();
        if c.is_ascii_digit()
            || (c == '-' && prev_is_gap && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            out.push(Token::Num(s.parse().unwrap_or(if s.starts_with('-') {
                i64::MIN
            } else {
                i64::MAX
            })));
        } else if c.is_alphabetic() {
            let start = i;
            while i < chars.len() && (chars[i].is_alphabetic() || chars[i] == '\'') {
                i += 1;
            }
            let s: String = chars[start..i].iter().collect();
            out.push(Token::Word(s.to_lowercase()));
        } else {
            i += 1;
        }
    }
    out
}

fn words_of(text: &str) -> Vec<String> {
    tokenize(text)
        .into_iter()
        .filter_map(|t| match t {
            Token::Word(w) => Some(w),
            Token::Num(_) => None,
        })
        .collect()
}

/// Match one of `names` at the head of `tokens`: every word of the name, case-folded,
/// where a word may be written in full or cut short to at least three letters ("sept",
/// "march" against "Sep", "Mar"). Only an unambiguous match counts.
fn match_name(names: &[String], tokens: &[Token]) -> Option<(usize, usize)> {
    let word_matches = |typed: &str, name: &str| {
        typed == name
            || (typed.chars().count() >= 3 && (name.starts_with(typed) || typed.starts_with(name)))
    };
    let mut hits = names.iter().enumerate().filter_map(|(index, name)| {
        let words = words_of(name);
        if words.is_empty() || words.len() > tokens.len() {
            return None;
        }
        let all = words.iter().zip(tokens).all(|(w, t)| match t {
            Token::Word(typed) => word_matches(typed, w),
            Token::Num(_) => false,
        });
        all.then_some((index, words.len()))
    });
    let first = hits.next()?;
    // An exact spelling wins over a prefix that also fits another name.
    let rest: Vec<_> = hits.collect();
    if rest.is_empty() {
        return Some(first);
    }
    let exact = |(index, used): &(usize, usize)| {
        let words = words_of(&names[*index]);
        words
            .iter()
            .zip(&tokens[..*used])
            .all(|(w, t)| matches!(t, Token::Word(typed) if typed == w))
    };
    std::iter::once(first).chain(rest).find(exact)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The design's invented calendar, exactly as the wireframe sketches it.
    pub(crate) fn accord() -> Calendar {
        let mut season = CalendarUnit::new("Season", 4, 0, ", {name}", 1);
        season.names = ["wet", "dry", "high", "low"].iter().map(|s| s.to_string()).collect();
        season.shown_below = Some(ShownBelow { count: 40, unit: 0 });
        let mut day = CalendarUnit::new("Day", 320, 0, ", day {n}", 1);
        day.shown_below = Some(ShownBelow { count: 2, unit: 0 });
        let mut bell = CalendarUnit::new("Bell", 16, 2, ", bell {n}", 1);
        bell.shown_below = Some(ShownBelow { count: 20, unit: 2 });
        Calendar {
            name: "The Accord".into(),
            units: vec![CalendarUnit::new("Year", 1, 0, "yr {n}", 0), season, day, bell],
        }
    }

    fn at(tick: i64, precision: u8) -> TimePoint {
        TimePoint { tick, precision }
    }

    #[test]
    fn both_calendars_validate() {
        assert_eq!(Calendar::default().validate(), Ok(()));
        assert_eq!(accord().validate(), Ok(()));
    }

    #[test]
    fn a_base_unit_point_reads_as_the_bare_year_in_the_default_calendar() {
        // Card 3's gutter printed base units; the default calendar must say the same.
        let cal = Calendar::default();
        assert_eq!(cal.format_point(&TimePoint::base_unit(1206)), "1206");
        assert_eq!(cal.format_point(&TimePoint::base_unit(-40)), "-40");
    }

    #[test]
    fn default_days_are_one_365th_and_whole_ticks() {
        let cal = Calendar::default();
        let p = cal.parse_point("1817 Mar 4").unwrap();
        assert_eq!(p.precision, 2);
        // Mar starts on day ceil(2·365/12) = 61 of the year; the 4th is day 64.
        assert_eq!(p.tick, 1817 * TICKS_PER_BASE_UNIT + 64 * 86_400);
        assert_eq!(cal.format_point(&p), "1817 Mar 4");
    }

    #[test]
    fn default_months_fall_out_as_thirty_or_thirty_one_days_with_no_special_case() {
        let cal = Calendar::default();
        let lengths: Vec<i128> = (0..12).map(|m| cal.child_count(2, 1817 * 12 + m)).collect();
        assert_eq!(lengths, vec![31, 30, 31, 30, 31, 30, 30, 31, 30, 31, 30, 30]);
        assert_eq!(lengths.iter().sum::<i128>(), 365, "a year is still exactly 365 days");
        // Including a February 30th, which is the price of deferring real Gregorian.
        assert!(cal.parse_point("2019 Feb 30").is_ok());
        assert!(cal.parse_point("2019 Feb 31").is_err());
    }

    #[test]
    fn every_default_day_and_hour_round_trips_through_text() {
        let cal = Calendar::default();
        for day in 0..365 {
            let p = at(2019 * TICKS_PER_BASE_UNIT + day * 86_400, 2);
            let text = cal.format_point(&p);
            assert_eq!(cal.parse_point(&text), Ok(p), "{text}");
        }
        let p = at(2019 * TICKS_PER_BASE_UNIT + 40 * 86_400 + 13 * 3_600, 3);
        assert_eq!(cal.format_point(&p), "2019 Feb 10 13h");
        assert_eq!(cal.parse_point("2019 Feb 10 13h"), Ok(p));
    }

    #[test]
    fn the_accord_reads_and_writes_seasons_days_and_bells() {
        let cal = accord();
        let p = cal.parse_point("yr 1206, dry, day 12, bell 9").unwrap();
        assert_eq!(p.precision, 3);
        assert_eq!(cal.format_point(&p), "yr 1206, dry, day 12, bell 9");
        // Parts can be typed loosely: names by prefix, units named or not, commas or not.
        assert_eq!(cal.parse_point("1206 dry 12 9"), Ok(p));
        assert_eq!(cal.parse_point("Year 1206 Dry Day 12 Bell 9"), Ok(p));
        // A season is a quarter of 320 days.
        assert!(cal.parse_point("yr 1206, dry, day 80").is_ok());
        assert!(cal.parse_point("yr 1206, dry, day 81").is_err());
        assert!(cal.parse_point("yr 1206, dry, day 12, bell 17").is_err());
        assert!(cal.parse_point("yr 1206, spring").is_err());
    }

    #[test]
    fn a_bell_that_is_not_a_whole_number_of_ticks_still_reads_back_exactly() {
        // 31_536_000 / (320 × 16) = 6159.375: the one place a start rounds. Every bell of
        // a day must still come back as the bell that was typed.
        let cal = accord();
        for bell in 1..=16 {
            let text = format!("yr 3, high, day 5, bell {bell}");
            let p = cal.parse_point(&text).unwrap();
            assert_eq!(cal.format_point(&p), text);
        }
    }

    #[test]
    fn precision_is_a_depth_the_calendar_names() {
        let cal = accord();
        assert_eq!(cal.parse_point("yr 1206").unwrap().precision, 0);
        assert_eq!(cal.parse_point("yr 1206 wet").unwrap().precision, 1);
        assert_eq!(cal.precision_name(1), "Season");
        assert_eq!(Calendar::default().precision_name(1), "Month");
        // A point deeper than the calendar reads at the calendar's deepest unit.
        assert_eq!(cal.precision_name(7), "Bell");
    }

    #[test]
    fn a_coarse_point_never_reads_as_a_finer_date() {
        let cal = accord();
        let p = cal.parse_point("yr 1206 low").unwrap();
        assert_eq!(cal.format_point(&p), "yr 1206, low");
    }

    #[test]
    fn accord_dates_sort_chronologically_as_plain_integers() {
        let cal = accord();
        let typed = [
            "yr 1206, low",
            "yr 1205",
            "yr 1206, dry, day 12, bell 9",
            "yr 1206, dry, day 12, bell 10",
            "yr 1206, dry, day 13",
            "yr -2",
            "yr 1206, wet",
        ];
        let mut points: Vec<(i64, &str)> = typed
            .iter()
            .map(|t| (cal.parse_point(t).unwrap().tick, *t))
            .collect();
        points.sort();
        let order: Vec<&str> = points.into_iter().map(|(_, t)| t).collect();
        assert_eq!(
            order,
            vec![
                "yr -2",
                "yr 1205",
                "yr 1206, wet",
                "yr 1206, dry, day 12, bell 9",
                "yr 1206, dry, day 12, bell 10",
                "yr 1206, dry, day 13",
                "yr 1206, low",
            ]
        );
    }

    #[test]
    fn negative_years_count_back_from_the_epoch() {
        let cal = Calendar::default();
        let p = cal.parse_point("-3 Dec 30").unwrap();
        assert!(p.tick < 0);
        assert_eq!(cal.format_point(&p), "-3 Dec 30");
    }

    #[test]
    fn junk_is_refused_with_a_reason() {
        let cal = Calendar::default();
        assert!(cal.parse_point("").is_err());
        assert!(cal.parse_point("sometime").is_err());
        assert!(cal.parse_point("1817 Mar 4 13h 5").is_err(), "no minutes in this calendar");
        assert!(cal.parse_point("1817 Mar 0").is_err(), "days count from 1");
    }

    #[test]
    fn validation_catches_every_rule_the_arithmetic_depends_on() {
        let mut c = accord();
        c.units[2].of = 3;
        assert!(c.validate().is_err(), "a unit must divide one above it");

        let mut c = accord();
        c.units[1].per = 1;
        assert!(c.validate().is_err(), "a divisor of one is not a finer unit");

        let mut c = accord();
        c.units[3].per = 100_000;
        assert!(c.validate().is_err(), "finer than a tick");

        let mut c = accord();
        c.units[2].per = 2; // a Day of half a Year, coarser than a Season
        assert!(c.validate().is_err(), "each unit finer than the one before");

        let mut c = accord();
        c.units[1].format = "season".into();
        assert!(c.validate().is_err(), "a format that writes no value");

        let mut c = accord();
        c.units[2].name = "season".into();
        assert!(c.validate().is_err(), "names must be distinct");

        assert!(Calendar { name: String::new(), units: Vec::new() }.validate().is_err());
    }

    #[test]
    fn an_unusable_stored_calendar_falls_back_to_the_default_rather_than_failing() {
        let mut broken = accord();
        broken.units[1].per = 0;
        assert_eq!(Calendar::effective(Some(&broken)), Calendar::default());
        assert_eq!(Calendar::effective(Some(&accord())), accord());
        assert_eq!(Calendar::effective(None), Calendar::default());
    }

    #[test]
    fn the_timeline_reads_each_units_shown_rule_against_the_visible_span() {
        let cal = accord();
        let year = TICKS_PER_BASE_UNIT as f64;
        assert_eq!(cal.unit_ticks(0), year);
        assert_eq!(cal.unit_ticks(2), year / 320.0);
        // Season: span < 40 Years. Day: < 2 Years. Bell: < 20 Days.
        assert!(cal.shown_at(0, 1e12), "the base unit has no rule: always shown");
        assert!(cal.shown_at(1, 39.0 * year) && !cal.shown_at(1, 40.0 * year));
        assert!(cal.shown_at(2, 1.5 * year) && !cal.shown_at(2, 2.0 * year));
        assert!(cal.shown_at(3, 19.0 * year / 320.0) && !cal.shown_at(3, 20.0 * year / 320.0));
        assert!(!cal.shown_at(9, 1.0), "a unit that does not exist is never shown");
    }

    #[test]
    fn a_part_reads_alone_without_its_joining_punctuation() {
        let cal = accord();
        let p = cal.parse_point("yr 1206, dry, day 12").unwrap();
        assert_eq!(cal.format_part(&p), "day 12");
        let p = cal.parse_point("yr 1206, dry").unwrap();
        assert_eq!(cal.format_part(&p), "dry");
        assert_eq!(cal.format_part(&TimePoint::base_unit(1206)), "yr 1206");
        let d = Calendar::default();
        assert_eq!(d.format_part(&d.parse_point("1817 Mar").unwrap()), "Mar");
    }

    #[test]
    fn a_calendar_round_trips_through_json_with_absent_fields_defaulted() {
        let json = serde_json::to_string(&accord()).unwrap();
        assert_eq!(serde_json::from_str::<Calendar>(&json).unwrap(), accord());
        let minimal: Calendar = serde_json::from_str(r#"{"units":[{"name":"Age"}]}"#).unwrap();
        assert_eq!(minimal.validate(), Ok(()));
        assert_eq!(minimal.format_point(&TimePoint::base_unit(3)), "3");
    }
}
