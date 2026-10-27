//! Dates and durations in Romanian (and English) documents, as OCR reads them.
//!
//! Documents write dates as 14.03.2027, 14/03/2027, 14-03-27, 2027-03-14 or "14 martie 2027", and
//! OCR adds its own noise: an O for a zero, an l for a one, a space around the dot. Durations
//! ("garanție 24 de luni", "2 ani") are found too, for deadlines that are computed (a purchase date
//! plus the warranty).

use std::sync::LazyLock;

use chrono::{Datelike, Months, NaiveDate};
use regex::Regex;

use crate::text::Folded;

/// A date found in a text, with its byte span in the original text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub date: NaiveDate,
    pub start: usize,
    pub end: usize,
}

const MONTHS: &[(&str, u32)] = &[
    ("ianuarie", 1),
    ("ian", 1),
    ("january", 1),
    ("jan", 1),
    ("februarie", 2),
    ("feb", 2),
    ("febr", 2),
    ("february", 2),
    ("martie", 3),
    ("mart", 3),
    ("mar", 3),
    ("march", 3),
    ("aprilie", 4),
    ("apr", 4),
    ("april", 4),
    ("mai", 5),
    ("may", 5),
    ("iunie", 6),
    ("iun", 6),
    ("june", 6),
    ("jun", 6),
    ("iulie", 7),
    ("iul", 7),
    ("july", 7),
    ("jul", 7),
    ("august", 8),
    ("aug", 8),
    ("septembrie", 9),
    ("sept", 9),
    ("sep", 9),
    ("september", 9),
    ("octombrie", 10),
    ("oct", 10),
    ("october", 10),
    ("noiembrie", 11),
    ("noi", 11),
    ("nov", 11),
    ("november", 11),
    ("decembrie", 12),
    ("dec", 12),
    ("december", 12),
];

fn month(name: &str) -> Option<u32> {
    MONTHS.iter().find(|(n, _)| *n == name).map(|(_, m)| *m)
}

// On folded text (lowercase, no diacritics) after the digit fixes below.
static NUMERIC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(\d{1,2}) ?[./-] ?(\d{1,2}) ?[./-] ?(\d{4}|\d{2})\b").unwrap());
static ISO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(\d{4})-(\d{2})-(\d{2})\b").unwrap());
static WORDS: LazyLock<Regex> = LazyLock::new(|| {
    let names: Vec<&str> = MONTHS.iter().map(|(n, _)| *n).collect();
    Regex::new(&format!(
        r"\b(\d{{1,2}})\.? ?({})\.? ?,? ?(\d{{4}})\b",
        names.join("|")
    ))
    .unwrap()
});
static DURATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(\d{1,3}) ?(?:\([^)]*\) )?(?:de )?(luni|luna|months?|ani|an|anul|years?|zile|days?)\b",
    )
    .unwrap()
});

/// OCR reads digits as letters inside numbers: "1O.O3.2O27", "l4.03.2027". Within a run of
/// characters that has digits, letters that look like digits become those digits. The text keeps
/// its length, so offsets stay valid.
fn fix_digits(folded: &str) -> String {
    let bytes = folded.as_bytes();
    let mut out = bytes.to_vec();
    let mut i = 0;
    while i < bytes.len() {
        // A token: a run without spaces.
        let start = i;
        while i < bytes.len() && bytes[i] != b' ' {
            i += 1;
        }
        let token = &bytes[start..i];
        let digits = token.iter().filter(|b| b.is_ascii_digit()).count();
        let separators = token
            .iter()
            .filter(|b| matches!(b, b'.' | b'/' | b'-'))
            .count();
        // Only date-like tokens: mostly digits with date separators.
        if digits >= 3 && separators >= 1 {
            for (j, b) in token.iter().enumerate() {
                out[start + j] = match b {
                    b'o' => b'0',
                    b'l' | b'i' | b'|' => b'1',
                    b's' => b'5',
                    b'b' => b'8',
                    _ => *b,
                };
            }
        }
        i += 1;
    }
    String::from_utf8(out).expect("ASCII replacements keep UTF-8 valid")
}

/// Expands a two-digit year: documents are about the present and the near future.
fn year(y: &str) -> i32 {
    let n: i32 = y.parse().unwrap_or(0);
    if y.len() == 2 { 2000 + n } else { n }
}

/// Every date in `text`, in order.
pub fn find(text: &str) -> Vec<Found> {
    let folded = Folded::new(text);
    let fixed = fix_digits(&folded.text);
    let mut found: Vec<(usize, usize, NaiveDate)> = Vec::new();
    for c in ISO.captures_iter(&fixed) {
        let (y, m, d) = (
            c[1].parse().unwrap(),
            c[2].parse().unwrap(),
            c[3].parse().unwrap(),
        );
        if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
            let m0 = c.get(0).unwrap();
            found.push((m0.start(), m0.end(), date));
        }
    }
    for c in NUMERIC.captures_iter(&fixed) {
        let m0 = c.get(0).unwrap();
        if found
            .iter()
            .any(|(s, e, _)| m0.start() < *e && *s < m0.end())
        {
            continue;
        }
        // Day first: the Romanian (and European) order.
        let (d, m, y) = (c[1].parse().unwrap(), c[2].parse().unwrap(), year(&c[3]));
        if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
            found.push((m0.start(), m0.end(), date));
        }
    }
    for c in WORDS.captures_iter(&fixed) {
        let m0 = c.get(0).unwrap();
        if found
            .iter()
            .any(|(s, e, _)| m0.start() < *e && *s < m0.end())
        {
            continue;
        }
        let (d, y) = (c[1].parse().unwrap(), year(&c[3]));
        if let Some(date) = month(&c[2]).and_then(|m| NaiveDate::from_ymd_opt(y, m, d)) {
            found.push((m0.start(), m0.end(), date));
        }
    }
    found.sort_by_key(|f| f.0);
    found
        .into_iter()
        .map(|(s, e, date)| {
            let (start, end) = folded.original_range(s, e);
            Found { date, start, end }
        })
        .collect()
}

/// Parses a text that should be exactly one date (as a model or a user writes it).
pub fn parse_one(s: &str) -> Option<NaiveDate> {
    let f = find(s);
    if f.len() == 1 { Some(f[0].date) } else { None }
}

/// Durations in months: "24 de luni" = 24, "2 ani" = 24, "36 months" = 36. Days are not months
/// and are left out.
pub fn durations_months(text: &str) -> Vec<u32> {
    let folded = Folded::new(text);
    DURATION
        .captures_iter(&folded.text)
        .filter_map(|c| {
            let n: u32 = c[1].parse().ok()?;
            match &c[2] {
                "luni" | "luna" | "month" | "months" => Some(n),
                "ani" | "an" | "anul" | "year" | "years" => Some(n * 12),
                _ => None,
            }
        })
        .collect()
}

/// `date` plus `months`, clamped to the end of the month (31 Jan + 1 month = 28/29 Feb).
pub fn add_months(date: NaiveDate, months: u32) -> NaiveDate {
    date.checked_add_months(Months::new(months)).unwrap_or(date)
}

/// Orthodox Easter Sunday (Gregorian calendar), by the Meeus Julian algorithm.
pub fn orthodox_easter(year: i32) -> NaiveDate {
    let a = year % 4;
    let b = year % 7;
    let c = year % 19;
    let d = (19 * c + 15) % 30;
    let e = (2 * a + 4 * b - d + 34) % 7;
    let month = (d + e + 114) / 31;
    let day = (d + e + 114) % 31 + 1;
    let julian = NaiveDate::from_ymd_opt(year, month as u32, day as u32).unwrap();
    // The Julian calendar is 13 days behind for 1900–2099.
    julian + chrono::Duration::days(13)
}

/// Romania's public holidays (Codul muncii, art. 139), when offices and ITP stations are closed.
pub fn is_public_holiday(d: NaiveDate) -> bool {
    let fixed = [
        (1, 1),
        (1, 2),
        (1, 6),
        (1, 7),
        (1, 24),
        (5, 1),
        (6, 1),
        (8, 15),
        (11, 30),
        (12, 1),
        (12, 25),
        (12, 26),
    ];
    if fixed.contains(&(d.month(), d.day())) {
        return true;
    }
    let easter = orthodox_easter(d.year());
    let offsets = [-2, 0, 1, 49, 50]; // Good Friday, Easter, Easter Monday, Pentecost, Whit Monday
    offsets
        .iter()
        .any(|o| easter + chrono::Duration::days(*o) == d)
}

/// A working day: not a weekend, not a public holiday.
pub fn is_working_day(d: NaiveDate) -> bool {
    d.weekday().number_from_monday() <= 5 && !is_public_holiday(d)
}

/// The last working day on or before `d` (to do something before a deadline).
pub fn working_day_on_or_before(mut d: NaiveDate) -> NaiveDate {
    while !is_working_day(d) {
        d = d.pred_opt().unwrap();
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn dates(s: &str) -> Vec<NaiveDate> {
        find(s).into_iter().map(|f| f.date).collect()
    }

    #[test]
    fn formats() {
        assert_eq!(
            dates("Valabil de la 15.03.2026 până la 14.03.2027"),
            [d(2026, 3, 15), d(2027, 3, 14)]
        );
        assert_eq!(
            dates("data: 14/03/2027; emis 2026-03-15"),
            [d(2027, 3, 14), d(2026, 3, 15)]
        );
        assert_eq!(dates("expiră la 14-03-27"), [d(2027, 3, 14)]);
        assert_eq!(dates("Data expirării: 3 ianuarie 2031"), [d(2031, 1, 3)]);
        assert_eq!(
            dates("emis la 3 IAN. 2021, valabil până la 2 febr. 2031"),
            [d(2021, 1, 3), d(2031, 2, 2)]
        );
        assert_eq!(
            dates("14 Martie 2027 și 1 decembrie 2026"),
            [d(2027, 3, 14), d(2026, 12, 1)]
        );
        assert_eq!(dates("Date of expiry 12 MAY 2032"), [d(2032, 5, 12)]);
    }

    #[test]
    fn ocr_noise() {
        assert_eq!(dates("valabil pana la 1O.O3.2O27"), [d(2027, 3, 10)]);
        assert_eq!(dates("ITP valabil: l4.03.2027"), [d(2027, 3, 14)]);
        assert_eq!(dates("pana la 14 . 03 . 2027"), [d(2027, 3, 14)]);
        // Letters in words stay letters.
        assert!(dates("Solo Bold lilo").is_empty());
    }

    #[test]
    fn invalid_dates_and_numbers_are_not_dates() {
        assert!(dates("31.02.2027").is_empty());
        assert!(dates("tel. 0721.123.456").is_empty());
        assert!(dates("Total 1.234,56 lei").is_empty());
        assert!(dates("nr. 2026/123").is_empty());
    }

    #[test]
    fn spans_point_into_the_original() {
        let s = "Poliță valabilă până la 14 martie 2027 inclusiv";
        let f = find(s);
        assert_eq!(&s[f[0].start..f[0].end], "14 martie 2027");
    }

    #[test]
    fn parse_one_needs_exactly_one() {
        assert_eq!(parse_one("2027-03-14"), Some(d(2027, 3, 14)));
        assert_eq!(parse_one("14.03.2027 - 15.03.2027"), None);
        assert_eq!(parse_one("nimic"), None);
    }

    #[test]
    fn durations() {
        assert_eq!(durations_months("Termen de garanție: 24 de luni"), [24]);
        assert_eq!(durations_months("garanție 2 ani"), [24]);
        assert_eq!(durations_months("warranty 36 months; retur 14 zile"), [36]);
        assert_eq!(
            durations_months("garanție 24 (douăzeci și patru) luni"),
            [24]
        );
    }

    #[test]
    fn month_arithmetic_clamps() {
        assert_eq!(add_months(d(2026, 1, 31), 1), d(2026, 2, 28));
        assert_eq!(add_months(d(2026, 3, 14), 24), d(2028, 3, 14));
    }

    #[test]
    fn holidays() {
        // Orthodox Easter: 20 April 2025, 12 April 2026, 2 May 2027.
        assert_eq!(orthodox_easter(2025), d(2025, 4, 20));
        assert_eq!(orthodox_easter(2026), d(2026, 4, 12));
        assert_eq!(orthodox_easter(2027), d(2027, 5, 2));
        assert!(is_public_holiday(d(2026, 4, 13))); // Easter Monday
        assert!(is_public_holiday(d(2026, 12, 1)));
        assert!(is_public_holiday(d(2026, 4, 10))); // Good Friday
        assert!(!is_public_holiday(d(2026, 4, 14)));
        // Saturday 2 May 2026 -> Thursday 30 April (1 May is a holiday).
        assert_eq!(working_day_on_or_before(d(2026, 5, 2)), d(2026, 4, 30));
    }
}
