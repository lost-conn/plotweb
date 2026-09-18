//! The calendar screen's form, as plain strings — and the rules that turn it into a
//! [`Calendar`].
//!
//! Every field is a text field, deliberately. "Defined as" is typed the way the
//! wireframe writes it (`1/4 Year`, `1/16 Day`), and "shown in timeline when" the same
//! (`< 40 Years`), so the form reads as the table in `design/04-notes-wireframes.html`
//! ("The calendar, sketched") and needs no pickers — which on rinch means no list firing
//! `onclick` on pointerdown under a finger that meant to scroll.
//!
//! Pure and host-tested, like [`super::time_entry`].

use plotweb_common::{Calendar, CalendarUnit, ShownBelow, TimePoint, TICKS_PER_BASE_UNIT};

/// One unit's row, exactly as the author has typed it so far.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UnitDraft {
    pub name: String,
    /// `base` for the first row; `1/4 Year` for the rest.
    pub defined_as: String,
    /// The unit's format: `yr {n}`, `, {name}`.
    pub written_as: String,
    /// Comma-separated value names; empty for numbers.
    pub names: String,
    /// The number the first value is counted as.
    pub counts_from: String,
    /// `always`, or `< 40 Years`.
    pub shown_when: String,
}

fn plural(name: &str, count: u32) -> String {
    if count == 1 { name.to_string() } else { format!("{name}s") }
}

/// The rows for an existing calendar — the form's starting point.
pub fn drafts_from(calendar: &Calendar) -> Vec<UnitDraft> {
    calendar
        .units
        .iter()
        .enumerate()
        .map(|(i, unit)| UnitDraft {
            name: unit.name.clone(),
            defined_as: if i == 0 {
                "base".to_string()
            } else {
                let of = calendar.units.get(unit.of).map(|u| u.name.as_str()).unwrap_or("");
                format!("1/{} {}", unit.per, of)
            },
            written_as: unit.format.clone(),
            names: unit.names.join(", "),
            counts_from: unit.first.to_string(),
            shown_when: match unit.shown_below {
                None => "always".to_string(),
                Some(rule) => {
                    let of = calendar.units.get(rule.unit).map(|u| u.name.as_str()).unwrap_or("");
                    format!("< {} {}", rule.count, plural(of, rule.count))
                }
            },
        })
        .collect()
}

/// A blank row for "Add a unit", defined against the unit above it.
pub fn new_row(above: Option<&UnitDraft>) -> UnitDraft {
    UnitDraft {
        name: String::new(),
        defined_as: above
            .map(|a| format!("1/10 {}", a.name.trim()))
            .unwrap_or_else(|| "base".to_string()),
        written_as: "{n}".to_string(),
        names: String::new(),
        counts_from: "1".to_string(),
        shown_when: "always".to_string(),
    }
}

/// Find a unit among `names` by what the author typed: case-insensitive, with a plural
/// `s` tolerated ("Years", "days").
fn find_unit(typed: &str, names: &[String]) -> Option<usize> {
    let typed = typed.trim().to_lowercase();
    if typed.is_empty() {
        return None;
    }
    names.iter().position(|n| {
        let n = n.trim().to_lowercase();
        n == typed || format!("{n}s") == typed || (typed.ends_with('s') && n == typed[..typed.len() - 1])
    })
}

/// Words in "Defined as" / "shown when" that carry nothing.
const FILLER: [&str; 11] = [
    "of", "a", "an", "the", "span", "under", "below", "less", "than", "shorter", "when",
];

/// Split into numbers and a trailing name, dropping filler and punctuation.
fn number_then_name(text: &str) -> (Vec<u64>, String) {
    let mut numbers = Vec::new();
    let mut name = Vec::new();
    for word in text
        .split(|c: char| c.is_whitespace() || c == '/' || c == '<')
        .filter(|w| !w.is_empty())
    {
        if let Ok(n) = word.parse::<u64>() {
            if name.is_empty() {
                numbers.push(n);
                continue;
            }
        }
        if FILLER.contains(&word.to_lowercase().as_str()) {
            continue;
        }
        name.push(word);
    }
    (numbers, name.join(" "))
}

/// Read the form into a calendar, or say — per row, in the author's terms — what is
/// wrong with it. The calendar's own [`Calendar::validate`] has the last word.
pub fn calendar_from(name: &str, rows: &[UnitDraft]) -> Result<Calendar, String> {
    if rows.is_empty() {
        return Err("A calendar needs at least a base unit.".into());
    }
    let names: Vec<String> = rows.iter().map(|r| r.name.trim().to_string()).collect();
    let mut units = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let unit_name = row.name.trim();
        let label = if unit_name.is_empty() {
            format!("Unit {}", i + 1)
        } else {
            format!("“{unit_name}”")
        };

        let (per, of) = if i == 0 {
            let d = row.defined_as.trim().to_lowercase();
            if !(d.is_empty() || d == "base") {
                return Err(format!("{label} is the base unit — the one everything divides."));
            }
            (1, 0)
        } else {
            let (numbers, of_name) = number_then_name(&row.defined_as);
            let per = match numbers.as_slice() {
                [1, n] | [n] => *n,
                _ => {
                    return Err(format!(
                        "{label}: write “Defined as” like “1/4 {}”.",
                        names[0]
                    ));
                }
            };
            let of = find_unit(&of_name, &names[..i]).ok_or_else(|| {
                format!("{label} divides “{of_name}”, which is not a unit above it.")
            })?;
            let per = u32::try_from(per).map_err(|_| format!("{label} divides too finely."))?;
            (per, of)
        };

        let value_names: Vec<String> = row
            .names
            .split([',', '·'])
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();

        let written = row.written_as.trim();
        let format = if written.is_empty() {
            if value_names.is_empty() { "{n}" } else { "{name}" }.to_string()
        } else {
            row.written_as.trim_end().to_string()
        };

        let first = match row.counts_from.trim() {
            "" => i64::from(i != 0),
            n => n
                .parse::<i64>()
                .map_err(|_| format!("{label}: “counts from” should be a number."))?,
        };

        let shown = row.shown_when.trim();
        let shown_below = if shown.is_empty() || shown.eq_ignore_ascii_case("always") {
            None
        } else {
            let (numbers, of_name) = number_then_name(shown);
            let count = match numbers.as_slice() {
                [n] => u32::try_from(*n).map_err(|_| format!("{label}: that span is too long."))?,
                _ => {
                    return Err(format!(
                        "{label}: write “shown when” as “always” or like “< 40 {}s”.",
                        names[0]
                    ));
                }
            };
            let unit = find_unit(&of_name, &names).ok_or_else(|| {
                format!("{label} is shown by “{of_name}”, which is not a unit here.")
            })?;
            Some(ShownBelow { count, unit })
        };

        units.push(CalendarUnit {
            name: unit_name.to_string(),
            per,
            of,
            names: value_names,
            format,
            first,
            shown_below,
        });
    }
    let calendar = Calendar {
        name: name.trim().to_string(),
        units,
    };
    calendar.validate()?;
    Ok(calendar)
}

/// A date written in this calendar, to its finest unit — the form's live preview, so an
/// author sees "yr 1206, dry, day 12, bell 9" rather than having to imagine it.
pub fn example(calendar: &Calendar) -> String {
    // Some way into year 1206: far enough that every unit shows a value past its first.
    let tick = 1206 * TICKS_PER_BASE_UNIT + TICKS_PER_BASE_UNIT * 37 / 100 + 1;
    calendar.format_point(&TimePoint {
        tick,
        precision: calendar.deepest(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accord_rows() -> Vec<UnitDraft> {
        let row = |name: &str, defined: &str, written: &str, names: &str, from: &str, shown: &str| UnitDraft {
            name: name.into(),
            defined_as: defined.into(),
            written_as: written.into(),
            names: names.into(),
            counts_from: from.into(),
            shown_when: shown.into(),
        };
        vec![
            row("Year", "base", "yr {n}", "", "0", "always"),
            row("Season", "1/4 Year", ", {name}", "wet, dry, high, low", "1", "< 40 Years"),
            row("Day", "1 / 320 year", ", day {n}", "", "1", "span < 2 years"),
            row("Bell", "1/16 of a Day", ", bell {n}", "", "1", "< 20 days"),
        ]
    }

    #[test]
    fn the_accord_can_be_typed_in_as_the_wireframe_writes_it() {
        let cal = calendar_from("The Accord", &accord_rows()).expect("valid");
        assert_eq!(cal.units[1].per, 4);
        assert_eq!(cal.units[3].of, 2, "a bell divides the day");
        assert_eq!(cal.units[1].shown_below, Some(ShownBelow { count: 40, unit: 0 }));
        assert_eq!(cal.units[3].shown_below, Some(ShownBelow { count: 20, unit: 2 }));
        let p = cal.parse_point("yr 1206, dry, day 12, bell 9").unwrap();
        assert_eq!(cal.format_point(&p), "yr 1206, dry, day 12, bell 9");
        assert_eq!(example(&cal), "yr 1206, dry, day 39, bell 7");
    }

    #[test]
    fn the_form_round_trips_a_calendar() {
        for cal in [Calendar::default(), calendar_from("The Accord", &accord_rows()).unwrap()] {
            let back = calendar_from(&cal.name, &drafts_from(&cal)).expect("valid");
            assert_eq!(back, cal);
        }
    }

    #[test]
    fn the_default_calendar_reads_as_gregorian_shaped() {
        let rows = drafts_from(&Calendar::default());
        let defined: Vec<&str> = rows.iter().map(|r| r.defined_as.as_str()).collect();
        assert_eq!(defined, vec!["base", "1/12 Year", "1/365 Year", "1/24 Day"]);
        assert_eq!(example(&Calendar::default()), "1206 May 14 1h");
    }

    #[test]
    fn a_new_row_divides_the_unit_above_it() {
        let mut rows = accord_rows();
        let mut added = new_row(rows.last());
        added.name = "Moment".into();
        assert_eq!(added.defined_as, "1/10 Bell");
        rows.push(added);
        assert!(calendar_from("", &rows).is_ok());
    }

    #[test]
    fn mistakes_name_the_row_they_are_in() {
        let mut rows = accord_rows();
        rows[2].defined_as = "1/320 Month".into();
        let err = calendar_from("", &rows).unwrap_err();
        assert!(err.contains("“Day”") && err.contains("Month"), "{err}");

        let mut rows = accord_rows();
        rows[1].defined_as = "quarterly".into();
        assert!(calendar_from("", &rows).unwrap_err().contains("“Season”"));

        let mut rows = accord_rows();
        rows[0].defined_as = "1/2 Age".into();
        assert!(calendar_from("", &rows).is_err(), "the first row is the base");

        let mut rows = accord_rows();
        rows[3].counts_from = "one".into();
        assert!(calendar_from("", &rows).is_err());

        let mut rows = accord_rows();
        rows[3].defined_as = "1/100000 Day".into();
        assert!(calendar_from("", &rows).is_err(), "finer than a tick — validate() catches it");
    }
}
