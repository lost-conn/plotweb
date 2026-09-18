//! Typing a note's time — the five states, in one line of text.
//!
//! The design settled on five ways a note can sit in time: **exact**, **approximate**,
//! **open-ended**, **relative** to another note, and **undated**
//! (`design/04-notes-wireframes.html`, "Fuzzy + relative time"). They are entered
//! through one field in the facet strip rather than a form with a mode switch, because
//! they compose — "roughly the dry season, after the parley" is one thought — and the
//! same text is what the tree gutter and the strip display, so what the author reads is
//! what they would type to get it:
//!
//! | typed                            | stored                                        |
//! |----------------------------------|-----------------------------------------------|
//! | `yr 1206, dry, day 12`           | exact instant, known to the Day               |
//! | `1181 – 1211`                    | exact span (`to`, `..`, `-`, `—` also work)   |
//! | `~1206` · `c. 1206` · `about …`  | approximate                                   |
//! | `1198 –` · `1198 onward`         | open-ended: a start, and no known end         |
//! | `after The Siege` · `after @Siege` | relative (`before`, `during` too)           |
//! | *(empty)*                        | undated                                       |
//!
//! Dates are read through the book's [`Calendar`]; nothing here knows what a year is.
//!
//! **Nothing here relates a note to its event parent.** Whether a child event may fall
//! outside its parent's span is card 6's open question, so a span is stored exactly as
//! typed and never clamped, stretched or warned about.
//!
//! Pure functions over the note list, host-tested like [`super::sigils`] and
//! [`super::notes_filter`].

use plotweb_common::{fold_token, Calendar, Note, RelativeTime, TimeRelation, TimePoint, TimeSpan};

/// A note's whole place in time: what [`parse_entry`] produces and the facet strip
/// writes, both fields at once — the text in the field *is* the note's time, so
/// committing it replaces both.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimeEntry {
    pub span: Option<TimeSpan>,
    pub relative: Option<RelativeTime>,
}

/// The design's five states, as the strip names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeState {
    Exact,
    Approximate,
    OpenEnded,
    Relative,
    Undated,
}

impl TimeState {
    pub fn label(self) -> &'static str {
        match self {
            TimeState::Exact => "Exact",
            TimeState::Approximate => "Approximate",
            TimeState::OpenEnded => "Open-ended",
            TimeState::Relative => "Relative",
            TimeState::Undated => "Undated",
        }
    }
}

/// Which of the five a note is in. A span wins over a relation for naming — "roughly
/// 1206, after the parley" is approximate first — and approximate wins over open-ended,
/// because "roughly from 1198, still going" is above all not a measured date.
pub fn state_of(span: Option<&TimeSpan>, relative: Option<&RelativeTime>) -> TimeState {
    match (span, relative) {
        (Some(s), _) if s.approximate => TimeState::Approximate,
        (Some(s), _) if s.open_ended => TimeState::OpenEnded,
        (Some(_), _) => TimeState::Exact,
        (None, Some(_)) => TimeState::Relative,
        (None, None) => TimeState::Undated,
    }
}

fn relation_word(relation: TimeRelation) -> &'static str {
    match relation {
        TimeRelation::After => "after",
        TimeRelation::Before => "before",
        TimeRelation::During => "during",
    }
}

/// A note's time as the author would type it — `""` when undated. The strip's field is
/// prefilled with this, and [`parse_entry`] reads it back to the same value.
pub fn format_entry(
    span: Option<&TimeSpan>,
    relative: Option<&RelativeTime>,
    calendar: &Calendar,
    notes: &[Note],
) -> String {
    let mut out = String::new();
    if let Some(span) = span {
        if span.approximate {
            out.push('~');
        }
        out.push_str(&calendar.format_point(&span.start));
        match (&span.end, span.open_ended) {
            (Some(end), _) => {
                out.push_str(" \u{2013} ");
                out.push_str(&calendar.format_point(end));
            }
            (None, true) => out.push_str(" \u{2013}"),
            (None, false) => {}
        }
    }
    if let Some(rel) = relative {
        if !out.is_empty() {
            out.push(' ');
        }
        let target = notes
            .iter()
            .find(|n| n.id == rel.note_id)
            .map(|n| n.title.clone())
            .unwrap_or_else(|| "another note".to_string());
        out.push_str(relation_word(rel.relation));
        out.push(' ');
        out.push_str(&target);
    }
    out
}

/// One line the strip shows under the field while typing: which state this is, and how
/// precisely it is known, in the book's own words ("known to the Season").
pub fn describe(entry: &TimeEntry, calendar: &Calendar) -> String {
    let state = state_of(entry.span.as_ref(), entry.relative.as_ref());
    match &entry.span {
        Some(span) => {
            let finest = span.end.map_or(span.start.precision, |e| e.precision.max(span.start.precision));
            let mut out = format!(
                "{} · known to the {}",
                state.label(),
                calendar.precision_name(finest)
            );
            if span.approximate && span.open_ended {
                out.push_str(" · open-ended");
            }
            if entry.relative.is_some() {
                out.push_str(" · and relative");
            }
            out
        }
        None => state.label().to_string(),
    }
}

/// Read the facet strip's field. `self_id` is the note being dated — a note cannot be
/// placed relative to itself.
pub fn parse_entry(
    text: &str,
    calendar: &Calendar,
    notes: &[Note],
    self_id: &str,
) -> Result<TimeEntry, String> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("undated") {
        return Ok(TimeEntry::default());
    }

    let (span_text, relative) = match split_relation(text) {
        Some((before, relation, target)) => {
            (before, Some(resolve_target(relation, target, notes, self_id)?))
        }
        None => (text, None),
    };
    let span = match span_text.trim() {
        "" => None,
        s => Some(parse_span(s, calendar)?),
    };
    Ok(TimeEntry { span, relative })
}

/// Split off `after|before|during <target>` at the first such word.
fn split_relation(text: &str) -> Option<(&str, TimeRelation, &str)> {
    let lower = text.to_lowercase();
    // Byte offsets line up: ASCII keywords, and lowercasing is checked to be
    // length-preserving before any offset is trusted.
    if lower.len() != text.len() {
        return split_relation_slow(text);
    }
    let mut at = 0;
    while at < lower.len() {
        for (word, relation) in [
            ("after", TimeRelation::After),
            ("before", TimeRelation::Before),
            ("during", TimeRelation::During),
        ] {
            if lower[at..].starts_with(word) {
                let starts_word = at == 0 || lower[..at].ends_with(char::is_whitespace);
                let rest = &lower[at + word.len()..];
                let ends_word = rest.is_empty() || rest.starts_with(char::is_whitespace);
                if starts_word && ends_word {
                    return Some((&text[..at], relation, text[at + word.len()..].trim()));
                }
            }
        }
        at += lower[at..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

/// The same split for text whose lowercase form changes length (rare scripts): word by
/// word, reassembling the halves.
fn split_relation_slow(text: &str) -> Option<(&str, TimeRelation, &str)> {
    let mut offset = 0;
    for word in text.split_inclusive(char::is_whitespace) {
        let relation = match word.trim().to_lowercase().as_str() {
            "after" => Some(TimeRelation::After),
            "before" => Some(TimeRelation::Before),
            "during" => Some(TimeRelation::During),
            _ => None,
        };
        if let Some(relation) = relation {
            return Some((&text[..offset], relation, text[offset + word.len()..].trim()));
        }
        offset += word.len();
    }
    None
}

/// Find the note a relation names: by title, or by the `@`/`$` token the sigils write,
/// compared the way sigils compare them ([`fold_token`]). A unique prefix will do.
fn resolve_target(
    relation: TimeRelation,
    target: &str,
    notes: &[Note],
    self_id: &str,
) -> Result<RelativeTime, String> {
    let word = relation_word(relation);
    let key = fold_token(target.trim_start_matches(['@', '$']));
    if key.is_empty() {
        return Err(format!("{word} what? Name a note."));
    }
    let candidates: Vec<&Note> = notes.iter().filter(|n| n.id != self_id).collect();
    let exact: Vec<&&Note> = candidates.iter().filter(|n| fold_token(&n.title) == key).collect();
    let found = match exact.as_slice() {
        [one] => Some(**one),
        [first, ..] => Some(**first),
        [] => {
            let prefixed: Vec<&&Note> = candidates
                .iter()
                .filter(|n| fold_token(&n.title).starts_with(&key))
                .collect();
            match prefixed.as_slice() {
                [one] => Some(**one),
                [] => None,
                many => {
                    return Err(format!(
                        "“{}” could be {} notes — type more of the title.",
                        target,
                        many.len()
                    ));
                }
            }
        }
    };
    match found {
        Some(note) => Ok(RelativeTime {
            relation,
            note_id: note.id.clone(),
        }),
        None if notes.iter().any(|n| n.id == self_id && fold_token(&n.title) == key) => {
            Err("A note can’t be placed relative to itself.".into())
        }
        None => Err(format!("No note is called “{target}”.")),
    }
}

/// Words that mark a date as the author's placement rather than a measurement.
const APPROXIMATE: [&str; 6] = ["circa", "about", "around", "roughly", "c", "ca"];
/// Words that end an open span.
const ONWARD: [&str; 3] = ["onward", "onwards", "on"];

fn parse_span(text: &str, calendar: &Calendar) -> Result<TimeSpan, String> {
    let mut rest = text.trim();
    let mut approximate = false;
    loop {
        if let Some(r) = rest.strip_prefix(['~', '≈']) {
            approximate = true;
            rest = r.trim_start();
            continue;
        }
        let first: String = rest
            .chars()
            .take_while(|c| c.is_alphabetic())
            .collect::<String>()
            .to_lowercase();
        let after = &rest[rest
            .char_indices()
            .nth(first.chars().count())
            .map_or(rest.len(), |(i, _)| i)..];
        if APPROXIMATE.contains(&first.as_str()) && !first.is_empty() {
            approximate = true;
            rest = after.trim_start_matches('.').trim_start();
            continue;
        }
        if first == "from" {
            rest = after.trim_start();
            continue;
        }
        break;
    }

    let (start_text, end_text) = match split_range(rest) {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };

    // "1198 onward" is the same as "1198 –".
    let mut start_text = start_text.trim();
    let mut open = matches!(end_text, Some(e) if e.trim().is_empty());
    if end_text.is_none()
        && let Some((head, last)) = start_text.rsplit_once(char::is_whitespace)
        && ONWARD.contains(&last.to_lowercase().as_str())
    {
        start_text = head.trim();
        open = true;
    }

    let start = calendar.parse_point(start_text)?;
    let end = match end_text.map(str::trim) {
        Some(e) if !e.is_empty() => {
            let end = calendar.parse_point(e)?;
            if unit_end(calendar, &end) <= start.tick {
                return Err("That span ends before it starts.".into());
            }
            Some(end)
        }
        _ => None,
    };
    Ok(TimeSpan {
        start,
        end,
        approximate,
        open_ended: open && end.is_none(),
    })
}

/// The first tick after the unit a point names — so "1206 Mar – 1206" (through the end
/// of 1206) is not read as ending before it starts.
fn unit_end(calendar: &Calendar, point: &TimePoint) -> i64 {
    let level = (point.precision as usize).min(calendar.units.len().saturating_sub(1));
    calendar.start_tick(level, calendar.index_at(level, point.tick).saturating_add(1))
}

/// Split at the first range separator: `–`, `—`, `..`, a spaced ` to `, or a `-` that is
/// not a minus sign (a minus starts a number with nothing alphanumeric before it).
fn split_range(text: &str) -> Option<(&str, &str)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        match c {
            '\u{2013}' | '\u{2014}' => return Some((&text[..i], &text[i + c.len_utf8()..])),
            '.' if text[i..].starts_with("..") => {
                let after = text[i..].trim_start_matches('.');
                return Some((&text[..i], after));
            }
            '-' => {
                let prev = k.checked_sub(1).map(|p| chars[p].1);
                let next = chars.get(k + 1).map(|&(_, n)| n);
                // The calendar's own rule (`calendar::tokenize`): a minus starts a number
                // with nothing alphanumeric directly before it.
                let is_minus = next.is_some_and(|n| n.is_ascii_digit())
                    && !prev.is_some_and(char::is_alphanumeric);
                if !is_minus {
                    return Some((&text[..i], &text[i + 1..]));
                }
            }
            _ => {}
        }
    }
    // A spaced word "to", case-insensitively.
    let lower = text.to_lowercase();
    if lower.len() == text.len()
        && let Some(i) = lower.find(" to ")
    {
        return Some((&text[..i], &text[i + 4..]));
    }
    if lower.len() == text.len() && lower.ends_with(" to") {
        return Some((&text[..text.len() - 3], ""));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotweb_common::{CalendarUnit, ShownBelow, TICKS_PER_BASE_UNIT};

    fn accord() -> Calendar {
        let unit = |name: &str, per, of, format: &str, first| CalendarUnit {
            name: name.into(),
            per,
            of,
            names: vec![],
            format: format.into(),
            first,
            shown_below: None,
        };
        let mut season = unit("Season", 4, 0, ", {name}", 1);
        season.names = ["wet", "dry", "high", "low"].iter().map(|s| s.to_string()).collect();
        season.shown_below = Some(ShownBelow { count: 40, unit: 0 });
        Calendar {
            name: "The Accord".into(),
            units: vec![
                unit("Year", 1, 0, "yr {n}", 0),
                season,
                unit("Day", 320, 0, ", day {n}", 1),
                unit("Bell", 16, 2, ", bell {n}", 1),
            ],
        }
    }

    fn note(id: &str, title: &str) -> Note {
        Note {
            id: id.into(),
            book_id: "b".into(),
            title: title.into(),
            content: String::new(),
            color: None,
            created_at: String::new(),
            updated_at: String::new(),
            span: None,
            relative: None,
            is_entity: false,
            event_parent: None,
            links: Default::default(),
        }
    }

    fn cast() -> Vec<Note> {
        vec![
            note("siege", "The Siege of Vaun"),
            note("parley", "The Parley"),
            note("self", "The Breach"),
            note("sq1", "Siege engines"),
        ]
    }

    fn parse(text: &str) -> Result<TimeEntry, String> {
        parse_entry(text, &accord(), &cast(), "self")
    }

    /// Whatever is typed must read back as itself once formatted, and formatting that
    /// must parse to the same value — the strip prefills the field with the formatted
    /// text, so a mismatch would silently re-date a note on an unchanged Enter.
    fn round_trips(text: &str) -> TimeEntry {
        let entry = parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        let shown = format_entry(entry.span.as_ref(), entry.relative.as_ref(), &accord(), &cast());
        assert_eq!(parse(&shown), Ok(entry.clone()), "{text} → {shown}");
        entry
    }

    #[test]
    fn exact() {
        let e = round_trips("yr 1206, dry, day 12, bell 9");
        let span = e.span.unwrap();
        assert_eq!(span.start.precision, 3);
        assert!(!span.approximate && !span.open_ended && span.end.is_none());
        assert_eq!(state_of(Some(&span), None), TimeState::Exact);

        let e = round_trips("yr 1181 – yr 1211");
        assert_eq!(e.span.as_ref().unwrap().end.unwrap().tick, 1211 * TICKS_PER_BASE_UNIT);
        for spelled in ["1181 - 1211", "1181-1211", "1181 to 1211", "1181..1211", "1181 — 1211"] {
            assert_eq!(parse(spelled), Ok(e.clone()), "{spelled}");
        }
    }

    #[test]
    fn approximate() {
        let e = round_trips("~yr 1206, low");
        assert!(e.span.as_ref().unwrap().approximate);
        assert_eq!(e.span.as_ref().unwrap().start.precision, 1);
        for spelled in ["c. 1206 low", "circa 1206 low", "about 1206 low", "~ 1206, low"] {
            assert_eq!(parse(spelled), Ok(e.clone()), "{spelled}");
        }
        assert_eq!(state_of(e.span.as_ref(), None), TimeState::Approximate);
    }

    #[test]
    fn open_ended() {
        let e = round_trips("yr 1198 –");
        let span = e.span.unwrap();
        assert!(span.open_ended && span.end.is_none());
        assert_eq!(parse("1198 onward").unwrap().span, Some(span.clone()));
        assert_eq!(parse("from 1198 -").unwrap().span, Some(span.clone()));
        assert_eq!(state_of(Some(&span), None), TimeState::OpenEnded);
    }

    #[test]
    fn relative() {
        let e = round_trips("after The Siege of Vaun");
        assert_eq!(e.span, None);
        assert_eq!(
            e.relative,
            Some(RelativeTime { relation: TimeRelation::After, note_id: "siege".into() })
        );
        // The sigil token, a case-folded title, and a unique prefix all find it.
        assert_eq!(parse("after @The-Siege-of-Vaun"), Ok(e.clone()));
        assert_eq!(parse("AFTER the siege of vaun"), Ok(e.clone()));
        assert_eq!(parse("during the parl").unwrap().relative.unwrap().note_id, "parley");
        assert_eq!(state_of(None, e.relative.as_ref()), TimeState::Relative);

        // Composes with a span.
        let both = round_trips("~1206 before The Parley");
        assert!(both.span.unwrap().approximate);
        assert_eq!(both.relative.unwrap().relation, TimeRelation::Before);
    }

    #[test]
    fn undated() {
        assert_eq!(round_trips(""), TimeEntry::default());
        assert_eq!(parse("   "), Ok(TimeEntry::default()));
        assert_eq!(state_of(None, None), TimeState::Undated);
        assert_eq!(format_entry(None, None, &accord(), &cast()), "");
    }

    #[test]
    fn mistakes_are_refused_with_a_reason_rather_than_guessed_at() {
        assert!(parse("after").is_err());
        assert!(parse("after nobody at all").is_err());
        assert!(parse("after The Breach").is_err(), "not relative to itself");
        assert!(parse("after the").is_err(), "ambiguous: the siege, or the parley");
        assert!(parse("yr 1211 – yr 1181").is_err(), "ends before it starts");
        assert!(parse("yr 1206, spring").is_err());
    }

    #[test]
    fn a_span_ending_in_the_unit_it_started_in_is_not_backwards() {
        // "dry season 1206 through 1206" — the end names the whole of 1206.
        assert!(parse("1206 dry – 1206").is_ok());
    }

    #[test]
    fn negative_years_are_not_mistaken_for_a_range() {
        let e = parse("-40 – -30").unwrap();
        let span = e.span.unwrap();
        assert_eq!(span.start.tick, -40 * TICKS_PER_BASE_UNIT);
        assert_eq!(span.end.unwrap().tick, -30 * TICKS_PER_BASE_UNIT);
        assert_eq!(parse("-40").unwrap().span.unwrap().end, None);
    }

    #[test]
    fn the_default_calendar_needs_no_setup() {
        let cal = Calendar::default();
        let e = parse_entry("1817 Mar 4 – 1817 Apr", &cal, &[], "x").unwrap();
        assert_eq!(
            format_entry(e.span.as_ref(), None, &cal, &[]),
            "1817 Mar 4 \u{2013} 1817 Apr"
        );
        assert_eq!(describe(&e, &cal), "Exact · known to the Day");
    }

    #[test]
    fn describe_names_the_state_and_the_depth_in_the_books_words() {
        let cal = accord();
        assert_eq!(describe(&parse("~1206 low").unwrap(), &cal), "Approximate · known to the Season");
        assert_eq!(describe(&parse("after the parley").unwrap(), &cal), "Relative");
        assert_eq!(describe(&TimeEntry::default(), &cal), "Undated");
    }
}
