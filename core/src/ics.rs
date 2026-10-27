//! Calendar export (iCalendar, RFC 5545), for Google Calendar, Outlook or the phone's calendar.
//!
//! Each deadline is an all-day event with an alarm for every reminder of its policy (at 09:00 on
//! the reminder's day, as a trigger relative to the event, so it works in any time zone). Reminders
//! that are an action of their own (booking the ITP, the last day for a notice) are events too: a
//! calendar that drops imported alarms still shows them.
//!
//! UIDs are stable, so importing again updates the events instead of duplicating them.

use chrono::{DateTime, NaiveDate, Utc};

use crate::dates;
use crate::model::Document;
use crate::reminders::{self, Reminder};

/// Escapes a TEXT value (RFC 5545 §3.3.11).
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Folds a content line at 75 octets, never inside a UTF-8 character (§3.1).
fn fold_line(line: &str, out: &mut String) {
    let mut width = 0;
    for c in line.chars() {
        let n = c.len_utf8();
        if width + n > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += n;
    }
    out.push_str("\r\n");
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y%m%d").to_string()
}

/// Days from `from` to `to`.
fn days(from: NaiveDate, to: NaiveDate) -> i64 {
    (to - from).num_days()
}

/// An alarm at 09:00 on `on`, relative to the start (midnight) of an all-day event on `start`.
fn trigger(start: NaiveDate, on: NaiveDate) -> String {
    let d = days(on, start); // how many days before the event
    if d > 0 {
        // 09:00 that day = (d - 1) days and 15 hours before midnight of the event.
        format!("-P{}DT15H", d - 1)
    } else {
        format!("PT{}H", 9 + 24 * (-d))
    }
}

/// The calendar for these documents, with reminders from `today` (their dates are only used to
/// leave out alarms already past).
pub fn calendar(docs: &[Document], today: NaiveDate, now: DateTime<Utc>, locale: &str) -> String {
    let ro = locale.starts_with("ro");
    let all = reminders::plan(docs, today, locale);
    let stamp = now.format("%Y%m%dT%H%M%SZ").to_string();
    let mut lines: Vec<String> = vec![
        "BEGIN:VCALENDAR".into(),
        "VERSION:2.0".into(),
        "PRODID:-//Docket//Docket 0.1//EN".into(),
        "CALSCALE:GREGORIAN".into(),
        "METHOD:PUBLISH".into(),
        "X-WR-CALNAME:Docket".into(),
    ];
    for doc in docs {
        for d in doc.deadlines.iter().filter(|d| !d.done) {
            let Some(date) = dates::parse_one(&d.date) else {
                continue;
            };
            let label = if d.label.is_empty() {
                &doc.title
            } else {
                &d.label
            };
            let mine: Vec<&Reminder> = all
                .iter()
                .filter(|r| r.document_id == doc.id && r.deadline_id == d.id)
                .collect();
            let summary = if ro {
                format!("{label}: termen")
            } else {
                format!("{label}: deadline")
            };
            let description = if ro {
                format!(
                    "{}\nDin documentul „{}”, pagina {}: „{}”",
                    d.kind.describe(),
                    doc.title,
                    d.page,
                    d.quote
                )
            } else {
                format!(
                    "{}\nFrom “{}”, page {}: “{}”",
                    d.kind.describe(),
                    doc.title,
                    d.page,
                    d.quote
                )
            };
            lines.push("BEGIN:VEVENT".into());
            lines.push(format!("UID:{}-{}@docket", doc.id, d.id));
            lines.push(format!("DTSTAMP:{stamp}"));
            lines.push(format!("DTSTART;VALUE=DATE:{}", ymd(date)));
            lines.push(format!(
                "DTEND;VALUE=DATE:{}",
                ymd(date.succ_opt().unwrap())
            ));
            lines.push(format!("SUMMARY:{}", escape(&summary)));
            lines.push(format!("DESCRIPTION:{}", escape(&description)));
            lines.push(format!("CATEGORIES:{}", escape(d.kind.name())));
            lines.push("TRANSP:TRANSPARENT".into());
            for r in &mine {
                let Some(on) = dates::parse_one(&r.date) else {
                    continue;
                };
                lines.push("BEGIN:VALARM".into());
                lines.push("ACTION:DISPLAY".into());
                lines.push(format!(
                    "DESCRIPTION:{}",
                    escape(&format!("{} — {}", r.title, r.body))
                ));
                lines.push(format!("TRIGGER:{}", trigger(date, on)));
                lines.push("END:VALARM".into());
            }
            lines.push("END:VEVENT".into());
            for r in mine.iter().filter(|r| r.own_event) {
                let Some(on) = dates::parse_one(&r.date) else {
                    continue;
                };
                lines.push("BEGIN:VEVENT".into());
                lines.push(format!("UID:{}@docket", r.id.replace('/', "-")));
                lines.push(format!("DTSTAMP:{stamp}"));
                lines.push(format!("DTSTART;VALUE=DATE:{}", ymd(on)));
                lines.push(format!("DTEND;VALUE=DATE:{}", ymd(on.succ_opt().unwrap())));
                lines.push(format!("SUMMARY:{}", escape(&r.title)));
                lines.push(format!("DESCRIPTION:{}", escape(&r.body)));
                lines.push("TRANSP:TRANSPARENT".into());
                lines.push("BEGIN:VALARM".into());
                lines.push("ACTION:DISPLAY".into());
                lines.push(format!("DESCRIPTION:{}", escape(&r.title)));
                lines.push("TRIGGER:PT9H".into());
                lines.push("END:VALARM".into());
                lines.push("END:VEVENT".into());
            }
        }
    }
    lines.push("END:VCALENDAR".into());
    let mut out = String::new();
    for l in &lines {
        fold_line(l, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn rca() -> Document {
        Document {
            id: "a1".into(),
            kind: DocKind::Rca,
            title: "RCA Dacia, Logan; B-123-ABC".into(),
            added: "2026-10-01".into(),
            pages: vec![],
            fields: vec![],
            deadlines: vec![Deadline {
                id: "rca_expiry".into(),
                kind: DeadlineKind::RcaExpiry,
                date: "2027-03-14".into(),
                label: "RCA B-123-ABC".into(),
                page: 1,
                quote: "până la 14.03.2027".into(),
                source: Source::Model,
                notice_days: None,
                done: false,
            }],
            extraction: None,
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-27T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    /// Unfolds and splits into content lines.
    fn unfold(ics: &str) -> Vec<String> {
        ics.replace("\r\n ", "")
            .split("\r\n")
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect()
    }

    #[test]
    fn structure_and_alarms() {
        let ics = calendar(
            &[rca()],
            NaiveDate::from_ymd_opt(2026, 10, 27).unwrap(),
            now(),
            "en",
        );
        assert!(ics.starts_with("BEGIN:VCALENDAR\r\n") && ics.ends_with("END:VCALENDAR\r\n"));
        let lines = unfold(&ics);
        assert!(lines.contains(&"UID:a1-rca_expiry@docket".to_string()));
        assert!(lines.contains(&"DTSTART;VALUE=DATE:20270314".to_string()));
        assert!(lines.contains(&"DTEND;VALUE=DATE:20270315".to_string()));
        // RCA: 30, 7 (moved to the working day before) and 1 day before, each at 09:00.
        let triggers: Vec<&String> = lines.iter().filter(|l| l.starts_with("TRIGGER:")).collect();
        // 30 days before: 12 Feb; 7 days before: Sunday 7 March -> Friday 5 March (9 days); 1 day.
        assert_eq!(
            triggers,
            ["TRIGGER:-P29DT15H", "TRIGGER:-P8DT15H", "TRIGGER:-P0DT15H"]
        );
        assert!(lines.iter().any(|l| l == "SUMMARY:RCA B-123-ABC: deadline"));
        // Escaped commas and semicolons in the description.
        assert!(lines.iter().any(
            |l| l.starts_with("DESCRIPTION:") && l.contains("RCA Dacia\\, Logan\\; B-123-ABC")
        ));
        assert_eq!(
            lines.iter().filter(|l| *l == "BEGIN:VEVENT").count(),
            lines.iter().filter(|l| *l == "END:VEVENT").count()
        );
    }

    #[test]
    fn lines_are_folded_at_75_octets_on_character_boundaries() {
        let mut d = rca();
        d.deadlines[0].quote = "ăîșțâ".repeat(40);
        let ics = calendar(
            &[d],
            NaiveDate::from_ymd_opt(2026, 10, 27).unwrap(),
            now(),
            "ro",
        );
        for l in ics.split("\r\n") {
            assert!(l.len() <= 75, "{} octets: {l}", l.len());
        }
        assert!(unfold(&ics).iter().any(|l| l.contains(&"ăîșțâ".repeat(40))));
    }

    #[test]
    fn own_events_and_triggers() {
        assert_eq!(
            trigger(
                NaiveDate::from_ymd_opt(2027, 3, 14).unwrap(),
                NaiveDate::from_ymd_opt(2027, 3, 14).unwrap()
            ),
            "PT9H"
        );
        let mut d = rca();
        d.deadlines[0].kind = DeadlineKind::ItpExpiry;
        d.deadlines[0].id = "itp_expiry".into();
        let lines = unfold(&calendar(
            &[d],
            NaiveDate::from_ymd_opt(2026, 10, 27).unwrap(),
            now(),
            "en",
        ));
        // The booking reminder (14 days before, Sunday 28 Feb -> Friday 26 Feb) is its own event.
        assert!(lines.contains(&"UID:a1-itp_expiry-14@docket".to_string()));
        assert!(lines.contains(&"DTSTART;VALUE=DATE:20270226".to_string()));
    }
}
