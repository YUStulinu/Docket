//! From OCR text to what matters in a document: its kind, a title, key values and deadlines.
//!
//! The language model reads the pages and answers in a JSON schema, quoting for every value the
//! text it read it from. Nothing it says is taken on trust:
//!
//! - every quote must be found on the page it names (allowing for OCR errors), or on another page,
//!   which then becomes the page cited; a quote that is nowhere is a hallucination, and its value
//!   is dropped;
//! - every deadline's date must be in its quote. A computed deadline (a purchase date plus a
//!   24-month warranty) must have both the start date and the duration in the document, and the
//!   date is recomputed here: the model's arithmetic is not trusted either;
//! - kinds and deadline types come from closed lists (the schema's enums).
//!
//! Rules (keywords near dates) do the same job without a model. They are the fallback when there
//! is no model, fill in a deadline the model missed, and are the baseline the evaluation compares
//! the model against.

use std::sync::LazyLock;
use std::time::Instant;

use chrono::NaiveDate;
use regex::Regex;
use serde::Deserialize;
use serde_json::json;

use crate::dates;
use crate::fuzzy;
use crate::llm::LanguageModel;
use crate::model::*;
use crate::text::fold;

/// Most of a document fits; the rest is cut (the model's context is 4096 tokens on a phone).
const MAX_CHARS: usize = 6000;
/// Edits allowed per quote character when looking for a quote on a page.
const QUOTE_TOLERANCE: f64 = 0.2;

#[derive(Debug, Clone, PartialEq)]
pub struct Extracted {
    pub kind: DocKind,
    pub title: String,
    pub fields: Vec<Field>,
    pub deadlines: Vec<Deadline>,
    pub extraction: Extraction,
}

// --- the model -----------------------------------------------------------------------------------

pub const FIELD_KEYS: &[(&str, &str)] = &[
    ("holder", "the person or company the document belongs to"),
    (
        "plate",
        "vehicle registration number (număr de înmatriculare): county letters, digits, three letters",
    ),
    ("vin", "vehicle identification number (17 characters)"),
    ("vehicle", "make and model of the vehicle"),
    ("insurer", "insurance company"),
    ("policy_number", "policy series and number"),
    (
        "document_number",
        "the number of the passport, ID card, licence or invoice",
    ),
    ("product", "the product bought (for a warranty or invoice)"),
    ("seller", "the shop, supplier or landlord"),
    ("amount", "the amount to pay, with the currency"),
    (
        "address",
        "the address of the rented property or of the holder",
    ),
];

/// The fields worth asking about for a kind of document (all of them when the kind is unknown):
/// a shorter answer is a faster one on a phone.
pub fn fields_for(kind: DocKind) -> Vec<&'static str> {
    let keys: &[&str] = match kind {
        DocKind::VehicleRegistration => &["holder", "plate", "vin", "vehicle"],
        DocKind::Rca | DocKind::Casco => &[
            "holder",
            "plate",
            "vin",
            "vehicle",
            "insurer",
            "policy_number",
        ],
        DocKind::Itp => &["plate", "vin", "vehicle"],
        DocKind::Passport | DocKind::IdCard | DocKind::DrivingLicence => {
            &["holder", "document_number"]
        }
        DocKind::Warranty => &["product", "seller", "holder"],
        DocKind::RentContract => &["seller", "holder", "address", "amount"],
        DocKind::Invoice => &["seller", "holder", "amount", "address", "document_number"],
        DocKind::Other => return FIELD_KEYS.iter().map(|(k, _)| *k).collect(),
    };
    keys.to_vec()
}

/// The prompt and schema for a document. `hint` is the kind the rules see (they are rarely wrong
/// about the kind); it narrows the fields and deadline kinds asked for. The system prompt does not
/// depend on the document, so the phone's model keeps it computed between documents.
pub fn prompt(pages: &[Page], hint: DocKind) -> (String, String, String) {
    let kinds: Vec<String> = DocKind::ALL
        .iter()
        .map(|k| format!("- {}: {}", k.name(), k.describe()))
        .collect();
    let deadlines: Vec<String> = DeadlineKind::ALL
        .iter()
        .map(|k| format!("- {}: {}", k.name(), k.describe()))
        .collect();
    let fields: Vec<String> = FIELD_KEYS
        .iter()
        .map(|(k, d)| format!("- {k}: {d}"))
        .collect();
    let system = format!(
        "You read the OCR text of a Romanian personal document and extract what its owner needs to remember.\n\
         The text comes from OCR: expect misread letters, broken lines and stray characters.\n\n\
         Document kinds:\n{}\n\nDeadline kinds:\n{}\n\nField keys:\n{}\n\n\
         Rules:\n\
         - quote: copy, character for character, the short piece of text (one line, at most 100 characters) that \
         holds the value. Never invent text that is not there.\n\
         - page: the page number the quote is on.\n\
         - fields: give every key; when the document does not state it, an empty value and quote, and page 0.\n\
         - Dates as YYYY-MM-DD. Romanian dates are day.month.year (14.03.2027 is 2027-03-14).\n\
         - A deadline that is a start date plus a period (a warranty of 24 months from the purchase): give the \
         start_date and the months, and the date they add up to.\n\
         - For a contract, give notice_days if the text states a notice period (preaviz).\n\
         - Only the deadlines the document states or implies; an issue date is not a deadline.\n\
         - title: short, in Romanian, for a list of documents: the kind of document and what it is about (the product, \
         the vehicle and its plate, the supplier, the person), taken from this document only.\n\
         - Every value comes from the text above; there are no examples to copy.",
        kinds.join("\n"),
        deadlines.join("\n"),
        fields.join("\n")
    );
    let mut user = String::new();
    let mut used = 0;
    for p in pages {
        let room = MAX_CHARS.saturating_sub(used);
        if room == 0 {
            break;
        }
        let text: String = p.text.chars().take(room).collect();
        used += text.chars().count();
        user.push_str(&format!("=== Page {} ===\n{}\n\n", p.number, text.trim()));
    }
    user.push_str("Extract the document's kind, title, fields and deadlines.");
    let keys = fields_for(hint);
    let deadline_kinds: Vec<&str> = match DeadlineKind::typical_for(hint) {
        Some(t) => vec![t.name(), DeadlineKind::Other.name()],
        None => DeadlineKind::ALL.iter().map(|k| k.name()).collect(),
    };
    let schema = json!({
        "type": "object",
        "properties": {
            "kind": {"type": "string", "enum": DocKind::ALL.iter().map(|k| k.name()).collect::<Vec<_>>()},
            "title": {"type": "string", "maxLength": 80},
            // An object with one property per key, not an array: each key exactly once, so a
            // small model can neither loop on the same field until it runs out of tokens nor stop
            // after the first few (it does both with an array, or with optional keys).
            "fields": {
                "type": "object",
                "properties": keys.iter().map(|k| (k.to_string(), json!({
                    "type": "object",
                    "properties": {
                        "value": {"type": "string", "maxLength": 100},
                        "page": {"type": "integer"},
                        "quote": {"type": "string", "maxLength": 120}
                    },
                    "required": ["value", "page", "quote"]
                }))).collect::<serde_json::Map<String, serde_json::Value>>(),
                "required": keys
            },
            "deadlines": {
                "type": "array", "maxItems": 4,
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": {"type": "string", "enum": deadline_kinds},
                        "date": {"type": "string"},
                        "page": {"type": "integer"},
                        "quote": {"type": "string", "maxLength": 120},
                        "start_date": {"type": "string"},
                        "months": {"type": "integer"},
                        "notice_days": {"type": "integer"}
                    },
                    "required": ["kind", "date", "page", "quote"]
                }
            }
        },
        "required": ["kind", "title", "fields", "deadlines"]
    });
    (system, user, schema.to_string())
}

#[derive(Deserialize)]
struct Answer {
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    fields: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    deadlines: Vec<AnswerDeadline>,
}

#[derive(Deserialize)]
struct AnswerField {
    #[serde(default)]
    key: String,
    value: String,
    #[serde(default)]
    page: u32,
    #[serde(default)]
    quote: String,
}

#[derive(Deserialize)]
struct AnswerDeadline {
    kind: String,
    date: String,
    #[serde(default)]
    page: u32,
    #[serde(default)]
    quote: String,
    start_date: Option<String>,
    months: Option<u32>,
    notice_days: Option<u32>,
}

/// Finds a quote, preferring the page the model named. Returns (page, the text as it is on the
/// page).
fn locate(pages: &[Page], page: u32, quote: &str) -> Option<(u32, String)> {
    if fold(quote).chars().count() < 4 {
        return None;
    }
    let mut order: Vec<&Page> = pages.iter().filter(|p| p.number == page).collect();
    order.extend(pages.iter().filter(|p| p.number != page));
    for p in order {
        if let Some((s, e, _)) = fuzzy::locate(&p.text, quote, QUOTE_TOLERANCE) {
            return Some((p.number, whole_line(&p.text, s, e)));
        }
    }
    None
}

/// The line(s) of `text` around the span: a citation shows its context, and the checks need it
/// (a bare "21.01.2023" does not say whether it is an issue date or an expiry).
fn whole_line(text: &str, s: usize, e: usize) -> String {
    let start = text[..s].rfind('\n').map_or(0, |i| i + 1);
    let end = text[e..].find('\n').map_or(text.len(), |i| e + i);
    let line = text[start..end].trim();
    if line.chars().count() > 200 {
        text[s..e].trim().to_string()
    } else {
        line.to_string()
    }
}

/// Checks the model's answer against the pages; see the module comment.
pub fn check(
    pages: &[Page],
    raw: &str,
) -> Result<(DocKind, String, Vec<Field>, Vec<Deadline>, Vec<String>), String> {
    let a: Answer = serde_json::from_str(raw)
        .map_err(|e| format!("the answer is not the requested JSON: {e}"))?;
    let kind = DocKind::from_name(&a.kind).unwrap_or(DocKind::Other);
    let mut rejected = Vec::new();
    let mut fields = Vec::new();
    for (key, v) in a.fields {
        let Ok(mut f) = serde_json::from_value::<AnswerField>(v) else {
            continue;
        };
        f.key = key;
        if f.value.trim().is_empty() {
            continue;
        }
        let Some((page, quote)) = locate(pages, f.page, &f.quote) else {
            rejected.push(format!(
                "{} = {:?}: the quote {:?} is not in the document",
                f.key, f.value, f.quote
            ));
            continue;
        };
        // The value itself must be in the document too (a quote can be real and the value made up).
        if locate(pages, page, &f.value).is_none()
            && fuzzy::locate(&quote, &f.value, 0.34).is_none()
        {
            rejected.push(format!(
                "{} = {:?}: the value is not in the document",
                f.key, f.value
            ));
            continue;
        }
        if fields.iter().any(|x: &Field| x.key == f.key) {
            continue;
        }
        fields.push(Field {
            key: f.key,
            value: f.value.trim().to_string(),
            page,
            quote,
            source: Source::Model,
        });
    }
    let mut deadlines: Vec<Deadline> = Vec::new();
    for d in a.deadlines {
        let dk = DeadlineKind::from_name(&d.kind).unwrap_or(DeadlineKind::Other);
        let Some((page, quote)) = locate(pages, d.page, &d.quote) else {
            rejected.push(format!(
                "{} {}: the quote {:?} is not in the document",
                d.kind, d.date, d.quote
            ));
            continue;
        };
        let in_quote: Vec<NaiveDate> = dates::find(&quote).into_iter().map(|f| f.date).collect();
        let mut date = dates::parse_one(&d.date);
        // A computed deadline: the start and the period must be in the document; the sum is ours.
        if let (Some(start), Some(months)) =
            (d.start_date.as_deref().and_then(dates::parse_one), d.months)
        {
            let all_text: String = pages
                .iter()
                .map(|p| p.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let start_found = dates::find(&all_text).iter().any(|f| f.date == start);
            let months_found = dates::durations_months(&all_text).contains(&months);
            if start_found && months_found && months > 0 && months <= 240 {
                date = Some(computed_end(dk, start, months));
            } else if date.is_none_or(|x| !in_quote.contains(&x)) {
                rejected.push(format!(
                    "{} {}: {} + {} months is not what the document says",
                    d.kind,
                    d.date,
                    d.start_date.unwrap_or_default(),
                    months
                ));
                continue;
            }
        } else if let Some(x) = date {
            if !in_quote.contains(&x) {
                // The model may have misread the date: the quote's date nearest to it (within a
                // year: a digit or two wrong) is what the document says.
                let nearest = in_quote
                    .iter()
                    .min_by_key(|q| (**q - x).num_days().abs())
                    .copied();
                if let Some(n) = nearest.filter(|n| (*n - x).num_days().abs() <= 366) {
                    date = Some(n);
                } else {
                    rejected.push(format!(
                        "{} {}: the date is not in its quote {:?}",
                        d.kind, d.date, quote
                    ));
                    continue;
                }
            }
        }
        let Some(mut date) = date else {
            rejected.push(format!("{} {:?}: not a date", d.kind, d.date));
            continue;
        };
        // "de la 15.03.2026 până la 14.03.2027": a deadline is the end of a range, never its start.
        if in_quote.len() >= 2 && d.months.is_none() {
            if let Some(latest) = in_quote.iter().max().copied().filter(|l| *l > date) {
                rejected.push(format!(
                    "{} {}: the start of the range {:?}; its end is {}",
                    d.kind, d.date, quote, latest
                ));
                date = latest;
            }
        }
        let date_s = date.format("%Y-%m-%d").to_string();
        if deadlines.iter().any(|x| x.kind == dk && x.date == date_s) {
            continue;
        }
        let notice_days = d
            .notice_days
            .filter(|n| *n > 0 && *n <= 365 && quote_or_text_has_number(pages, *n));
        deadlines.push(Deadline {
            id: String::new(),
            kind: dk,
            date: date_s,
            label: String::new(),
            page,
            quote,
            source: Source::Model,
            notice_days,
            done: false,
        });
    }
    Ok((
        kind,
        a.title.trim().to_string(),
        fields,
        deadlines,
        rejected,
    ))
}

fn quote_or_text_has_number(pages: &[Page], n: u32) -> bool {
    let re = Regex::new(&format!(r"\b{n}\b")).unwrap();
    pages.iter().any(|p| re.is_match(&p.text))
}

/// The last day of a period that starts on `start`: a contract for 12 months from 1 Nov 2026 ends
/// on 31 Oct 2027; a 24-month warranty from 14 Mar 2025 runs until 14 Mar 2027 (Romanian
/// certificates count the warranty to the same day).
pub fn computed_end(kind: DeadlineKind, start: NaiveDate, months: u32) -> NaiveDate {
    let end = dates::add_months(start, months);
    if kind == DeadlineKind::ContractEnd {
        end.pred_opt().unwrap()
    } else {
        end
    }
}

// --- rules ---------------------------------------------------------------------------------------

/// (kind, folded phrase, weight)
const KIND_WORDS: &[(DocKind, &str, u32)] = &[
    (
        DocKind::VehicleRegistration,
        "certificat de inmatriculare",
        6,
    ),
    (DocKind::VehicleRegistration, "inmatricularii", 2),
    (DocKind::VehicleRegistration, "numar de identificare", 2),
    (DocKind::VehicleRegistration, "categoria vehiculului", 2),
    (DocKind::Rca, "raspundere civila", 4),
    (DocKind::Rca, "asigurare obligatorie", 3),
    (DocKind::Rca, "polita rca", 5),
    (DocKind::Rca, "rca", 2),
    (DocKind::Casco, "casco", 5),
    (DocKind::Itp, "inspectie tehnica periodica", 3),
    (DocKind::Itp, "raport de inspectie", 4),
    (DocKind::Passport, "pasaport", 5),
    (DocKind::Passport, "passport", 4),
    (DocKind::Passport, "p<rou", 5),
    (DocKind::IdCard, "carte de identitate", 6),
    (DocKind::IdCard, "identity card", 4),
    (DocKind::IdCard, "idrou", 5),
    (DocKind::DrivingLicence, "permis de conducere", 6),
    (DocKind::DrivingLicence, "driving licence", 4),
    (DocKind::Warranty, "certificat de garantie", 6),
    (DocKind::Warranty, "termen de garantie", 4),
    (DocKind::Warranty, "garantie", 2),
    (DocKind::RentContract, "contract de inchiriere", 6),
    (DocKind::RentContract, "locatiune", 3),
    (DocKind::RentContract, "locatar", 2),
    (DocKind::RentContract, "chiria", 2),
    (DocKind::Invoice, "factura", 3),
    (DocKind::Invoice, "total de plata", 3),
    (DocKind::Invoice, "scadenta", 3),
];

pub fn classify(text: &str) -> DocKind {
    let t = fold(text);
    let mut best = (DocKind::Other, 0u32);
    for kind in DocKind::ALL {
        let score: u32 = KIND_WORDS
            .iter()
            .filter(|(k, w, _)| *k == kind && t.contains(w))
            .map(|(_, _, s)| s)
            .sum();
        if score > best.1 {
            best = (kind, score);
        }
    }
    best.0
}

const EXPIRY_WORDS: &[&str] = &[
    "valabil",
    "expir",
    "pana la",
    "valid until",
    "date of expiry",
    "data expirarii",
    "valabilitate",
    "pana in",
];
const DUE_WORDS: &[&str] = &[
    "scaden",
    "termen de plata",
    "data limita",
    "due date",
    "plata pana la",
];
const PURCHASE_WORDS: &[&str] = &[
    "data vanzarii",
    "data cumpararii",
    "data facturii",
    "data livrarii",
    "data achizitiei",
    "cumparat la",
    "data:",
];
const START_WORDS: &[&str] = &[
    "incepand cu",
    "incepand de la",
    "de la data de",
    "cu incepere",
];

static PLATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([A-Z]{1,2})[ -]?(\d{2,3})[ -]?([A-Z]{3})\b").unwrap());
static VIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-HJ-NPR-Z0-9]{17}\b").unwrap());
static NOTICE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"preaviz[^\d]{0,30}(\d{1,3}) ?(?:\([^)]*\) )?(?:de )?zile").unwrap()
});

/// County codes on Romanian plates.
const COUNTIES: &[&str] = &[
    "AB", "AR", "AG", "B", "BC", "BH", "BN", "BT", "BV", "BR", "BZ", "CS", "CL", "CJ", "CT", "CV",
    "DB", "DJ", "GL", "GR", "GJ", "HR", "HD", "IL", "IS", "IF", "MM", "MH", "MS", "NT", "OT", "PH",
    "SM", "SJ", "SB", "SV", "TR", "TM", "TL", "VS", "VL", "VN",
];

fn lines(pages: &[Page]) -> impl Iterator<Item = (u32, &str)> {
    pages
        .iter()
        .flat_map(|p| p.text.lines().map(move |l| (p.number, l)))
}

fn rule_deadline(kind: DeadlineKind, page: u32, quote: &str, date: NaiveDate) -> Deadline {
    Deadline {
        id: String::new(),
        kind,
        date: date.format("%Y-%m-%d").to_string(),
        label: String::new(),
        page,
        quote: quote.trim().to_string(),
        source: Source::Rules,
        notice_days: None,
        done: false,
    }
}

/// The latest date on a line that has one of `words`.
fn latest_near(pages: &[Page], words: &[&str]) -> Option<(u32, String, NaiveDate)> {
    let mut best: Option<(u32, String, NaiveDate)> = None;
    for (page, line) in lines(pages) {
        let f = fold(line);
        if !words.iter().any(|w| f.contains(w)) {
            continue;
        }
        for d in dates::find(line) {
            if best.as_ref().is_none_or(|b| d.date > b.2) {
                best = Some((page, line.to_string(), d.date));
            }
        }
    }
    best
}

fn first_near(pages: &[Page], words: &[&str]) -> Option<(u32, String, NaiveDate)> {
    for (page, line) in lines(pages) {
        let f = fold(line);
        if words.iter().any(|w| f.contains(w)) {
            if let Some(d) = dates::find(line).first() {
                return Some((page, line.to_string(), d.date));
            }
        }
    }
    None
}

pub fn rules(pages: &[Page]) -> (DocKind, String, Vec<Field>, Vec<Deadline>) {
    let all: String = pages
        .iter()
        .map(|p| p.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let kind = classify(&all);
    let mut fields = Vec::new();
    for (page, line) in lines(pages) {
        // OCR reads a capital I as "|" (|S 36 XDU).
        let up = line.replace('|', "I").to_uppercase();
        if !fields.iter().any(|f: &Field| f.key == "plate") {
            if let Some(c) = PLATE.captures_iter(&up).find(|c| COUNTIES.contains(&&c[1])) {
                let value = format!("{}-{}-{}", &c[1], &c[2], &c[3]);
                fields.push(Field {
                    key: "plate".into(),
                    value,
                    page,
                    quote: line.trim().into(),
                    source: Source::Rules,
                });
            }
        }
        if !fields.iter().any(|f: &Field| f.key == "vin") {
            if let Some(m) = VIN.find(&up).filter(|m| {
                m.as_str().chars().any(|c| c.is_ascii_digit())
                    && m.as_str().chars().any(|c| c.is_ascii_alphabetic())
            }) {
                fields.push(Field {
                    key: "vin".into(),
                    value: m.as_str().into(),
                    page,
                    quote: line.trim().into(),
                    source: Source::Rules,
                });
            }
        }
    }
    let mut deadlines = Vec::new();
    match DeadlineKind::typical_for(kind) {
        Some(DeadlineKind::InvoiceDue) => {
            if let Some((p, q, d)) = first_near(pages, DUE_WORDS) {
                deadlines.push(rule_deadline(DeadlineKind::InvoiceDue, p, &q, d));
            }
        }
        Some(DeadlineKind::WarrantyEnd) => {
            // An explicit end, or the purchase date plus the period.
            if let Some((p, q, d)) = latest_near(
                pages,
                &[
                    "garantie valabila pana",
                    "garantia expira",
                    "pana la data de",
                ],
            ) {
                deadlines.push(rule_deadline(DeadlineKind::WarrantyEnd, p, &q, d));
            } else {
                let months = lines(pages)
                    .filter(|(_, l)| fold(l).contains("garanti"))
                    .find_map(|(p, l)| {
                        dates::durations_months(l)
                            .first()
                            .map(|m| (p, l.to_string(), *m))
                    });
                let start = first_near(pages, PURCHASE_WORDS).or_else(|| {
                    // Otherwise the first date of the document (receipts start with it).
                    lines(pages).find_map(|(p, l)| {
                        dates::find(l).first().map(|d| (p, l.to_string(), d.date))
                    })
                });
                if let (Some((p, q, m)), Some((_, _, s))) = (months, start) {
                    deadlines.push(rule_deadline(
                        DeadlineKind::WarrantyEnd,
                        p,
                        &q,
                        computed_end(DeadlineKind::WarrantyEnd, s, m),
                    ));
                }
            }
        }
        Some(DeadlineKind::ContractEnd) => {
            let months = lines(pages)
                .filter(|(_, l)| {
                    let f = fold(l);
                    f.contains("perioad") || f.contains("durata")
                })
                .find_map(|(p, l)| {
                    dates::durations_months(l)
                        .first()
                        .map(|m| (p, l.to_string(), *m))
                });
            let start = first_near(pages, START_WORDS);
            let mut d = match (months, start) {
                (Some((p, q, m)), Some((_, _, s))) => Some(rule_deadline(
                    DeadlineKind::ContractEnd,
                    p,
                    &q,
                    computed_end(DeadlineKind::ContractEnd, s, m),
                )),
                _ => latest_near(pages, &["pana la", "inceteaza", "expira"])
                    .map(|(p, q, d)| rule_deadline(DeadlineKind::ContractEnd, p, &q, d)),
            };
            if let Some(d) = d.as_mut() {
                d.notice_days = NOTICE.captures(&fold(&all)).and_then(|c| c[1].parse().ok());
            }
            deadlines.extend(d);
        }
        Some(k) => {
            let words: Vec<&str> = EXPIRY_WORDS
                .iter()
                .copied()
                .chain(["urmatoarea inspectie", "4b."])
                .collect();
            if let Some((p, q, d)) = latest_near(pages, &words) {
                deadlines.push(rule_deadline(k, p, &q, d));
            } else if matches!(
                k,
                DeadlineKind::PassportExpiry
                    | DeadlineKind::IdCardExpiry
                    | DeadlineKind::LicenceExpiry
            ) {
                // Identity documents: the expiry is the latest date printed on them.
                let latest = lines(pages)
                    .flat_map(|(p, l)| dates::find(l).into_iter().map(move |d| (p, l, d.date)))
                    .max_by_key(|x| x.2);
                if let Some((p, q, d)) = latest {
                    deadlines.push(rule_deadline(k, p, q, d));
                }
            }
        }
        None => {}
    }
    let title = rule_title(kind, &fields, pages);
    (kind, title, fields, deadlines)
}

fn rule_title(kind: DocKind, fields: &[Field], pages: &[Page]) -> String {
    let plate = fields
        .iter()
        .find(|f| f.key == "plate")
        .map(|f| f.value.as_str());
    let base = match kind {
        DocKind::VehicleRegistration => "Vehicle registration",
        DocKind::Rca => "RCA",
        DocKind::Casco => "CASCO",
        DocKind::Itp => "ITP",
        DocKind::Passport => "Passport",
        DocKind::IdCard => "ID card",
        DocKind::DrivingLicence => "Driving licence",
        DocKind::Warranty => "Warranty",
        DocKind::RentContract => "Rent contract",
        DocKind::Invoice => "Invoice",
        DocKind::Other => {
            return pages
                .first()
                .and_then(|p| p.text.lines().find(|l| l.trim().len() > 3))
                .unwrap_or("Document")
                .trim()
                .chars()
                .take(60)
                .collect();
        }
    };
    match plate {
        Some(p) => format!("{base} {p}"),
        None => base.to_string(),
    }
}

// --- both ----------------------------------------------------------------------------------------

/// Extracts with the model if there is one, checking its answer, and with the rules for what it
/// leaves out. Without a model, or if it fails, the rules alone.
pub fn extract(pages: &[Page], model: Option<&dyn LanguageModel>) -> Extracted {
    let started = Instant::now();
    let (rkind, rtitle, rfields, rdeadlines) = rules(pages);
    let Some(model) = model else {
        return finish(
            rkind,
            rtitle,
            rfields,
            rdeadlines,
            Extraction {
                method: "rules".into(),
                seconds: started.elapsed().as_secs_f64(),
                rejected: vec![],
            },
        );
    };
    let (system, user, schema) = prompt(pages, rkind);
    let answer = model
        .complete(system, user, schema)
        .map_err(|e| e.to_string())
        .and_then(|raw| check(pages, &raw));
    match answer {
        Ok((kind, title, mut fields, mut deadlines, mut rejected)) => {
            review(kind, &mut deadlines, &rdeadlines, &mut rejected);
            // What the rules found and the model did not.
            for f in rfields {
                if !fields.iter().any(|x| x.key == f.key) {
                    fields.push(f);
                }
            }
            if let Some(typical) = DeadlineKind::typical_for(kind) {
                if !deadlines.iter().any(|d| d.kind == typical) {
                    deadlines.extend(rdeadlines.into_iter().filter(|d| d.kind == typical));
                }
            }
            let title = if title.is_empty() { rtitle } else { title };
            finish(
                kind,
                title,
                fields,
                deadlines,
                Extraction {
                    method: model.name(),
                    seconds: started.elapsed().as_secs_f64(),
                    rejected,
                },
            )
        }
        Err(e) => finish(
            rkind,
            rtitle,
            rfields,
            rdeadlines,
            Extraction {
                method: "rules".into(),
                seconds: started.elapsed().as_secs_f64(),
                rejected: vec![format!("the model failed: {e}")],
            },
        ),
    }
}

/// Words of the line that states when something starts or was issued, not when it ends.
const START_WORDS_ANY: &[&str] = &[
    "emis",
    "eliber",
    "issue",
    "emiterii",
    "4a.",
    "incepand",
    "vanzarii",
    "cumpararii",
    "facturii",
    "data itp",
    "inspectiei",
    "nasterii",
    "birth",
    "din data",
    "primei inmatriculari",
    "semnat",
];
/// Words of the line that states an end.
const END_WORDS: &[&str] = &[
    "valabil",
    "expir",
    "pana la",
    "pana in",
    "until",
    "expiry",
    "scaden",
    "termen de plata",
    "data limita",
    "4b.",
    "urmatoarea",
    "inceteaza",
    "garantie",
];

/// Second thoughts on the model's deadlines, with what the rules found:
/// - a document has only the deadlines its kind can have (no "invoice due" on a warranty);
/// - a deadline whose quote says when something started or was issued, and nothing about an end,
///   is a misread: the rules' deadline of that kind, if there is one, replaces it.
fn review(
    kind: DocKind,
    deadlines: &mut Vec<Deadline>,
    rules: &[Deadline],
    rejected: &mut Vec<String>,
) {
    let allowed = |k: DeadlineKind| match (kind, DeadlineKind::typical_for(kind)) {
        (DocKind::Other, _) => true,
        (_, Some(t)) => k == t,
        _ => false,
    };
    deadlines.retain(|d| {
        let ok = allowed(d.kind);
        if !ok {
            rejected.push(format!(
                "{} {}: not a deadline a {} has",
                d.kind.name(),
                d.date,
                kind.name()
            ));
        }
        ok
    });
    for d in deadlines.iter_mut() {
        let q = fold(&d.quote);
        let says_start = START_WORDS_ANY.iter().any(|w| q.contains(w));
        let says_end = END_WORDS.iter().any(|w| q.contains(w));
        if says_start && !says_end {
            if let Some(r) = rules.iter().find(|r| r.kind == d.kind) {
                rejected.push(format!(
                    "{} {}: its quote {:?} is a start or issue date; the rules read {}",
                    d.kind.name(),
                    d.date,
                    d.quote,
                    r.date
                ));
                *d = r.clone();
            }
        }
    }
    deadlines.dedup_by(|a, b| a.kind == b.kind && a.date == b.date);
}

fn finish(
    kind: DocKind,
    title: String,
    fields: Vec<Field>,
    mut deadlines: Vec<Deadline>,
    extraction: Extraction,
) -> Extracted {
    deadlines.sort_by(|a, b| a.date.cmp(&b.date));
    assign_ids(&mut deadlines);
    let plate = fields
        .iter()
        .find(|f| f.key == "plate")
        .map(|f| f.value.clone());
    for d in deadlines.iter_mut() {
        if d.label.is_empty() {
            d.label = label(d.kind, &title, plate.as_deref());
        }
    }
    Extracted {
        kind,
        title,
        fields,
        deadlines,
        extraction,
    }
}

pub fn assign_ids(deadlines: &mut [Deadline]) {
    let mut seen: Vec<DeadlineKind> = Vec::new();
    for d in deadlines.iter_mut() {
        let n = seen.iter().filter(|k| **k == d.kind).count();
        seen.push(d.kind);
        d.id = if n == 0 {
            d.kind.name().to_string()
        } else {
            format!("{}-{}", d.kind.name(), n + 1)
        };
    }
}

fn label(kind: DeadlineKind, title: &str, plate: Option<&str>) -> String {
    let what = match kind {
        DeadlineKind::ItpExpiry => "ITP",
        DeadlineKind::RcaExpiry => "RCA",
        DeadlineKind::CascoExpiry => "CASCO",
        DeadlineKind::PassportExpiry => "Passport",
        DeadlineKind::IdCardExpiry => "ID card",
        DeadlineKind::LicenceExpiry => "Driving licence",
        _ => return title.to_string(),
    };
    match plate {
        Some(p) => format!("{what} {p}"),
        None => what.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn page(n: u32, text: &str) -> Page {
        Page {
            number: n,
            text: text.into(),
            lines: vec![],
            width: 0,
            height: 0,
            confidence: 90.0,
        }
    }

    const RCA: &str = "ASIGURARE OBLIGATORIE DE RĂSPUNDERE CIVILĂ AUTO\nPOLIȚĂ RCA Seria RO/25/X25/AA Nr. 0012345678\n\
                       Asigurător: Allianz-Țiriac Asigurări S.A.\nVehicul: DACIA LOGAN, nr. înmatriculare B 123 ABC\n\
                       Perioada de valabilitate: de la 15.03.2026 până la 14.03.2027\nPrima de asigurare: 1.250,00 lei";

    struct Fixed(String);
    impl LanguageModel for Fixed {
        fn complete(&self, _s: String, _u: String, _j: String) -> Result<String, crate::Error> {
            Ok(self.0.clone())
        }
        fn name(&self) -> String {
            "fixed".into()
        }
    }

    #[test]
    fn rules_read_an_rca() {
        let (kind, title, fields, deadlines) = rules(&[page(1, RCA)]);
        assert_eq!(kind, DocKind::Rca);
        assert_eq!(title, "RCA B-123-ABC");
        assert_eq!(fields[0].value, "B-123-ABC");
        assert_eq!(deadlines.len(), 1);
        assert_eq!(
            (deadlines[0].kind, deadlines[0].date.as_str()),
            (DeadlineKind::RcaExpiry, "2027-03-14")
        );
        assert!(deadlines[0].quote.contains("până la 14.03.2027"));
    }

    #[test]
    fn rules_compute_a_warranty_and_a_contract() {
        let w = "CERTIFICAT DE GARANȚIE\nProdus: Mașină de spălat Samsung WW90T\nData vânzării: 14.03.2025\nTermen de garanție: 24 de luni";
        let (kind, _, _, d) = rules(&[page(1, w)]);
        assert_eq!(kind, DocKind::Warranty);
        assert_eq!(d[0].date, "2027-03-14");
        let c = "CONTRACT DE ÎNCHIRIERE\nArt. 3. Contractul se încheie pe o perioadă de 12 luni,\nîncepând cu 01.11.2026.\n\
                 Art. 7. Rezilierea se face cu un preaviz de 30 de zile.";
        let (kind, _, _, d) = rules(&[page(1, c)]);
        assert_eq!(kind, DocKind::RentContract);
        assert_eq!(
            (d[0].date.as_str(), d[0].notice_days),
            ("2027-10-31", Some(30))
        );
    }

    #[test]
    fn model_answers_are_checked() {
        let pages = [page(1, RCA)];
        let raw = json!({
            "kind": "rca", "title": "RCA Dacia Logan B-123-ABC",
            "fields": {
                "insurer": {"value": "Allianz-Țiriac Asigurări", "page": 1, "quote": "Asigurător: Allianz-Țiriac Asigurări S.A."},
                "holder": {"value": "Ion Popescu", "page": 1, "quote": "Asigurat: Ion Popescu"},
                "vin": {"value": "UU1XXXXXX12345678", "page": 1, "quote": "Vehicul: DACIA LOGAN, nr. înmatriculare B 123 ABC"}
            },
            "deadlines": [
                {"kind": "rca_expiry", "date": "2027-03-14", "page": 2, "quote": "de la 15.03.2026 pana la 14.03.2027"},
                {"kind": "itp_expiry", "date": "2027-06-01", "page": 1, "quote": "ITP valabil până la 01.06.2027"}
            ]
        })
        .to_string();
        let (kind, _, fields, deadlines, rejected) = check(&pages, &raw).unwrap();
        assert_eq!(kind, DocKind::Rca);
        // The made-up holder and VIN are dropped; the insurer stays.
        assert_eq!(
            fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(),
            ["insurer"]
        );
        // The RCA deadline is kept, and its page fixed to where the quote really is.
        assert_eq!(deadlines.len(), 1);
        assert_eq!(
            (deadlines[0].page, deadlines[0].date.as_str()),
            (1, "2027-03-14")
        );
        assert_eq!(rejected.len(), 3, "{rejected:?}");
    }

    #[test]
    fn model_dates_are_corrected_from_the_quote_and_computed_here() {
        let w = "CERTIFICAT DE GARANȚIE\nData vânzării: 14.03.2025\nTermen de garanție: 24 de luni";
        let pages = [page(1, w)];
        // Wrong arithmetic (2027-03-13) is replaced by the sum of what the document says.
        let raw = r#"{"kind":"warranty","title":"t","fields":{},"deadlines":[{"kind":"warranty_end","date":"2027-03-13","page":1,"quote":"Termen de garanție: 24 de luni","start_date":"2025-03-14","months":24}]}"#;
        let (_, _, _, d, _) = check(&pages, raw).unwrap();
        assert_eq!(d[0].date, "2027-03-14");
        // A period the document does not state is rejected.
        let raw = r#"{"kind":"warranty","title":"t","fields":{},"deadlines":[{"kind":"warranty_end","date":"2028-03-14","page":1,"quote":"Termen de garanție: 24 de luni","start_date":"2025-03-14","months":36}]}"#;
        let (_, _, _, d, rejected) = check(&pages, raw).unwrap();
        assert!(d.is_empty() && rejected.len() == 1);
        // A misread date with the quote holding one date: the quote wins.
        let pages = [page(1, RCA)];
        let raw = r#"{"kind":"rca","title":"t","fields":{},"deadlines":[{"kind":"rca_expiry","date":"2027-03-17","page":1,"quote":"până la 14.03.2027"}]}"#;
        let (_, _, _, d, _) = check(&pages, raw).unwrap();
        assert_eq!(d[0].date, "2027-03-14");
    }

    #[test]
    fn extract_falls_back_to_rules() {
        let pages = [page(1, RCA)];
        let bad = Fixed("not json".into());
        let e = extract(&pages, Some(&bad));
        assert_eq!(e.extraction.method, "rules");
        assert_eq!(e.deadlines[0].date, "2027-03-14");
        assert_eq!(e.deadlines[0].id, "rca_expiry");
        assert_eq!(e.deadlines[0].label, "RCA B-123-ABC");
        // A model that misses the deadline gets it from the rules.
        let quiet =
            Fixed(r#"{"kind":"rca","title":"RCA Dacia","fields":{},"deadlines":[]}"#.into());
        let e = extract(&pages, Some(&quiet));
        assert_eq!(
            (e.extraction.method.as_str(), e.title.as_str()),
            ("fixed", "RCA Dacia")
        );
        assert_eq!(e.deadlines[0].source, Source::Rules);
    }

    #[test]
    fn second_thoughts_on_the_model() {
        // The start of a range, an issue date taken for the expiry, a deadline the kind cannot have.
        let id = "ROMÂNIA CARTE DE IDENTITATE
Emisă de SPCLEP Iași la 12.09.2025
Valabilitate 12.09.25-12.09.2035";
        let raw = r#"{"kind":"id_card","title":"CI","fields":{},"deadlines":[
            {"kind":"id_card_expiry","date":"2025-09-12","page":1,"quote":"Valabilitate 12.09.25-12.09.2035"},
            {"kind":"invoice_due","date":"2035-09-12","page":1,"quote":"Valabilitate 12.09.25-12.09.2035"}]}"#;
        let e = extract(&[page(1, id)], Some(&Fixed(raw.into())));
        assert_eq!(e.deadlines.len(), 1);
        assert_eq!(
            (e.deadlines[0].kind, e.deadlines[0].date.as_str()),
            (DeadlineKind::IdCardExpiry, "2035-09-12")
        );
        let raw = r#"{"kind":"id_card","title":"CI","fields":{},"deadlines":[
            {"kind":"id_card_expiry","date":"2025-09-12","page":1,"quote":"Emisă de SPCLEP Iași la 12.09.2025"}]}"#;
        let e = extract(&[page(1, id)], Some(&Fixed(raw.into())));
        assert_eq!(
            (e.deadlines[0].date.as_str(), e.deadlines[0].source),
            ("2035-09-12", Source::Rules)
        );
        assert!(
            e.extraction
                .rejected
                .iter()
                .any(|r| r.contains("issue date"))
        );
    }

    #[test]
    fn plates_through_ocr_noise() {
        let (_, _, fields, _) = rules(&[page(
            1,
            "RCA
Număr de înmatriculare: |S 36 XDU",
        )]);
        assert_eq!(fields[0].value, "IS-36-XDU");
        // The first capitals-digits-capitals on the line need not be the plate.
        let (_, _, fields, _) = rules(&[page(1, "RCA ZZ 12 ABC, nr. B 123 XYZ")]);
        assert_eq!(fields[0].value, "B-123-XYZ");
    }

    #[test]
    fn classify_kinds() {
        assert_eq!(
            classify("ROMÂNIA CARTE DE IDENTITATE IDROU POPESCU<<ION"),
            DocKind::IdCard
        );
        assert_eq!(
            classify("PAŞAPORT PASSPORT P<ROUPOPESCU"),
            DocKind::Passport
        );
        assert_eq!(
            classify("FACTURĂ FISCALĂ ... Total de plată 245,30 lei, data scadenței 15.11.2026"),
            DocKind::Invoice
        );
        assert_eq!(
            classify("CERTIFICAT DE ÎNMATRICULARE ... ITP valabil"),
            DocKind::VehicleRegistration
        );
        assert_eq!(classify("o listă de cumpărături"), DocKind::Other);
    }
}
