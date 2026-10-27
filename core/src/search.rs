//! Search over your own documents, and answers that cite the page.
//!
//! Retrieval is BM25 over passages of the OCR text, plus a "fact card" per deadline and field
//! (kind, title, date, written out in words) so that "când expiră garanția la mașina de spălat?"
//! finds the warranty's end even though the certificate never says "expiră". Words are folded
//! (no diacritics), lightly stemmed for Romanian (mașina, mașinii, mașinile → masin), and a few
//! everyday words are expanded to the ones documents use (buletin → carte de identitate).
//!
//! An answer from the language model must cite the passages it used; a date it states must be in
//! one of them. Otherwise the answer is marked unverified and the best passage is shown instead.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::dates;
use crate::llm::LanguageModel;
use crate::model::Document;
use crate::text::{fold, words};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct Hit {
    pub document_id: String,
    pub title: String,
    pub page: u32,
    /// The passage (or the fact) that matched.
    pub text: String,
    pub score: f64,
    /// A deadline or field rather than raw page text.
    pub fact: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct Citation {
    pub document_id: String,
    pub title: String,
    pub page: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct Answer {
    pub text: String,
    pub citations: Vec<Citation>,
    /// The answer is backed by what it cites (every date it states is in a cited passage).
    pub verified: bool,
    /// "model" or "search" (no model: the best passage is the answer).
    pub method: String,
}

struct Passage {
    doc: usize,
    page: u32,
    text: String,
    fact: bool,
    terms: HashMap<String, f64>,
    len: f64,
}

pub struct Index {
    docs: Vec<(String, String)>, // (id, title)
    today: Option<chrono::NaiveDate>,
    passages: Vec<Passage>,
    df: HashMap<String, usize>,
    avg_len: f64,
}

/// Light Romanian (and English) stemming: strips the common inflections of nouns and adjectives.
pub fn stem(word: &str) -> String {
    const SUFFIXES: &[&str] = &[
        "urilor", "elor", "ilor", "ului", "iile", "ilei", "elei", "urile", "ele", "ile", "uri",
        "ul", "ii", "le", "lor", "ea", "ei", "a", "e", "i", "u", "s",
    ];
    if word.chars().any(|c| c.is_ascii_digit()) {
        return word.to_string();
    }
    for s in SUFFIXES {
        if let Some(stem) = word.strip_suffix(s) {
            if stem.chars().count() >= 4 {
                return stem.to_string();
            }
        }
    }
    word.to_string()
}

/// Everyday words and the words documents use for the same thing.
const EXPANSIONS: &[(&str, &[&str])] = &[
    (
        "expira",
        &[
            "valabil",
            "valabilitate",
            "pana",
            "expirarii",
            "termen",
            "sfarsit",
        ],
    ),
    ("expiry", &["valabil", "expira", "expirarii"]),
    ("buletin", &["carte", "identitate"]),
    ("talon", &["certificat", "inmatriculare"]),
    ("itp", &["inspectie", "tehnica", "periodica"]),
    ("chirie", &["inchiriere", "locatiune", "chiria"]),
    ("chiria", &["inchiriere", "locatiune"]),
    ("asigurare", &["polita", "rca", "casco"]),
    ("masina", &["auto", "vehicul", "autoturism"]),
    ("curent", &["energie", "electrica"]),
    ("plata", &["scadenta", "total", "factura"]),
    ("garantie", &["garantia", "termen"]),
    ("permis", &["conducere"]),
    ("pasaport", &["passport"]),
    ("cumparat", &["vanzator", "magazin"]),
    ("platesc", &["suma", "total", "plata"]),
];

/// Romanian names of the months, so that "factura din octombrie" finds "Data facturii: 03.10.2026".
const MONTHS_RO: [&str; 12] = [
    "ianuarie",
    "februarie",
    "martie",
    "aprilie",
    "mai",
    "iunie",
    "iulie",
    "august",
    "septembrie",
    "octombrie",
    "noiembrie",
    "decembrie",
];

/// Words for the dates in a text ("octombrie 2026"), indexed with it but not shown.
fn date_words(text: &str) -> String {
    use chrono::Datelike;
    dates::find(text)
        .iter()
        .map(|f| format!("{} {}", MONTHS_RO[f.date.month0() as usize], f.date.year()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The words people use for a field.
fn field_words(key: &str) -> &'static str {
    match key {
        "holder" => "titular, asigurat, proprietar, al cui este",
        "plate" => "număr de înmatriculare, mașina",
        "vin" => "serie de șasiu, VIN, număr de identificare",
        "vehicle" => "mașina, autoturism, marca și modelul",
        "insurer" => "asigurător, firma de asigurări, la ce firmă",
        "policy_number" => "număr poliță, seria poliței",
        "document_number" => "număr document, seria și numărul",
        "product" => "produs, ce am cumpărat",
        "seller" => "vânzător, magazin, furnizor, proprietar, de unde am cumpărat",
        "amount" => "sumă de plată, total, cât costă, cât plătesc, chiria",
        "address" => "adresa, unde este",
        _ => "",
    }
}

/// Little words that say nothing about what is searched.
const STOP: &[&str] = &[
    "de", "la", "si", "in", "cu", "pe", "pentru", "ce", "cand", "care", "cat", "este", "e", "sunt",
    "mea", "meu", "mele", "mei", "a", "al", "ale", "un", "o", "the", "of", "my", "when", "does",
    "is", "what", "to", "for", "nr", "din", "se", "sa", "ma", "imi", "am", "are",
];

fn terms(text: &str) -> Vec<String> {
    let f = fold(text);
    words(&f)
        .map(|(_, w)| w)
        .filter(|w| !STOP.contains(w))
        .map(stem)
        .collect()
}

fn fact_texts(d: &Document, today: Option<chrono::NaiveDate>) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    // What the document is about, so that a deadline is found by it: "the Enel bill", "the RCA
    // of B-123-ABC", "the fridge's warranty".
    let about: Vec<&str> = d
        .fields
        .iter()
        .filter(|f| {
            matches!(
                f.key.as_str(),
                "plate" | "vehicle" | "product" | "seller" | "insurer" | "holder" | "address"
            )
        })
        .map(|f| f.value.as_str())
        .collect();
    let about = about.join(", ");
    for dl in &d.deadlines {
        let date = dates::parse_one(&dl.date);
        let spelled = date
            .map(|x| x.format("%d.%m.%Y").to_string())
            .unwrap_or_default();
        // A policy that ended last year still answers "until when is the RCA valid?": say so.
        let status = match (date, today) {
            (Some(x), Some(t)) if x < t => " (expirat, nu mai e valabil)",
            (Some(_), Some(_)) => " (în vigoare)",
            _ => "",
        };
        out.push((
            dl.page,
            format!(
                "{} — {}: {} ({}){status}. {}. {}. {}. {about}",
                d.title,
                dl.kind.describe(),
                dl.date,
                spelled,
                spoken(dl.kind),
                d.kind.describe(),
                dl.quote
            ),
        ));
    }
    for f in &d.fields {
        out.push((
            f.page,
            format!(
                "{} — {} ({}): {}. {}",
                d.title,
                f.key.replace('_', " "),
                field_words(&f.key),
                f.value,
                f.quote
            ),
        ));
    }
    out
}

/// How people ask about a deadline, in Romanian.
fn spoken(k: crate::model::DeadlineKind) -> &'static str {
    use crate::model::DeadlineKind::*;
    match k {
        ItpExpiry => "ITP-ul expiră, inspecția tehnică periodică e valabilă până la",
        RcaExpiry => "RCA-ul expiră, asigurarea auto obligatorie e valabilă până la",
        CascoExpiry => "CASCO expiră, asigurarea e valabilă până la",
        PassportExpiry => "pașaportul expiră, e valabil până la",
        IdCardExpiry => "buletinul, cartea de identitate expiră, e valabilă până la",
        LicenceExpiry => "permisul de conducere expiră, e valabil până la",
        WarrantyEnd => "garanția expiră, se termină, e valabilă până la",
        ContractEnd => "contractul expiră, se termină, încetează",
        InvoiceDue => "factura e scadentă, trebuie plătită până la, termen de plată",
        Other => "termen, expiră",
    }
}

/// Pages cut into passages of about 60 words, overlapping by 20.
fn passages_of(text: &str) -> Vec<String> {
    let ws: Vec<&str> = text.split_whitespace().collect();
    if ws.len() <= 80 {
        return vec![ws.join(" ")];
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < ws.len() {
        out.push(ws[i..(i + 60).min(ws.len())].join(" "));
        if i + 60 >= ws.len() {
            break;
        }
        i += 40;
    }
    out
}

impl Index {
    pub fn build(docs: &[Document]) -> Index {
        Self::build_at(docs, None)
    }

    /// With `today`, deadline facts say whether they have passed.
    pub fn build_at(docs: &[Document], today: Option<chrono::NaiveDate>) -> Index {
        let mut passages = Vec::new();
        for (i, d) in docs.iter().enumerate() {
            for p in &d.pages {
                for text in passages_of(&p.text) {
                    passages.push((i, p.number, format!("{}\n{}", d.title, text), false));
                }
            }
            for (page, text) in fact_texts(d, today) {
                passages.push((i, page, text, true));
            }
        }
        let mut df: HashMap<String, usize> = HashMap::new();
        let mut built = Vec::with_capacity(passages.len());
        for (doc, page, text, fact) in passages {
            let ts = terms(&format!("{text} {}", date_words(&text)));
            let mut tf: HashMap<String, f64> = HashMap::new();
            for t in &ts {
                *tf.entry(t.clone()).or_default() += 1.0;
            }
            for t in tf.keys() {
                *df.entry(t.clone()).or_default() += 1;
            }
            let shown = text
                .split_once('\n')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or(text.clone());
            built.push(Passage {
                doc,
                page,
                text: shown,
                fact,
                len: ts.len().max(1) as f64,
                terms: tf,
            });
        }
        let avg_len = built.iter().map(|p| p.len).sum::<f64>() / built.len().max(1) as f64;
        Index {
            docs: docs
                .iter()
                .map(|d| (d.id.clone(), d.title.clone()))
                .collect(),
            today,
            passages: built,
            df,
            avg_len,
        }
    }

    /// The best passages for `query`, at most one per (document, page) and kind.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let mut q: Vec<(String, f64)> = terms(query).into_iter().map(|t| (t, 1.0)).collect();
        let folded = fold(query);
        for (w, more) in EXPANSIONS {
            if words(&folded).any(|(_, x)| stem(x) == stem(w)) {
                for m in *more {
                    q.push((stem(m), 0.4));
                }
            }
        }
        let asks_when = [
            "cand", "when", "expira", "data", "pana", "scaden", "termen", "valabil",
        ]
        .iter()
        .any(|w| folded.contains(w));
        let n = self.passages.len() as f64;
        let (k1, b) = (1.2, 0.75);
        let mut scored: Vec<(f64, usize)> = self
            .passages
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut s = 0.0;
                for (t, w) in &q {
                    let Some(tf) = p.terms.get(t) else { continue };
                    let df = *self.df.get(t).unwrap_or(&0) as f64;
                    let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                    s += w * idf * tf * (k1 + 1.0)
                        / (tf + k1 * (1.0 - b + b * p.len / self.avg_len));
                }
                // A question about a date prefers the deadline facts.
                if p.fact && asks_when && s > 0.0 {
                    s *= 1.3;
                }
                (s, i)
            })
            .filter(|(s, _)| *s > 0.0)
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then(a.1.cmp(&b.1)));
        let mut seen = Vec::new();
        let mut out = Vec::new();
        for (s, i) in scored {
            let p = &self.passages[i];
            let key = (p.doc, p.page, p.fact);
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            let (id, title) = &self.docs[p.doc];
            out.push(Hit {
                document_id: id.clone(),
                title: title.clone(),
                page: p.page,
                text: p.text.clone(),
                score: s,
                fact: p.fact,
            });
            if out.len() == limit {
                break;
            }
        }
        out
    }
}

fn cite(h: &Hit) -> Citation {
    Citation {
        document_id: h.document_id.clone(),
        title: h.title.clone(),
        page: h.page,
        text: h.text.chars().take(300).collect(),
    }
}

/// Answers a question from the documents. With a model: an answer in words that cites its sources;
/// without one, the best passage.
pub fn ask(
    index: &Index,
    question: &str,
    model: Option<&dyn LanguageModel>,
    locale: &str,
) -> Answer {
    let hits = index.search(question, 6);
    let ro = locale.starts_with("ro");
    if hits.is_empty() {
        return Answer {
            text: if ro {
                "Nu am găsit nimic despre asta în documentele tale.".into()
            } else {
                "Nothing in your documents seems to be about that.".into()
            },
            citations: vec![],
            verified: false,
            method: "search".into(),
        };
    }
    let fallback = |method: &str| Answer {
        text: hits[0].text.clone(),
        citations: vec![cite(&hits[0])],
        verified: true,
        method: method.into(),
    };
    let Some(model) = model else {
        return fallback("search");
    };
    let mut sources = String::new();
    for (i, h) in hits.iter().enumerate() {
        sources.push_str(&format!(
            "[{}] {} (page {}): {}\n\n",
            i + 1,
            h.title,
            h.page,
            h.text
        ));
    }
    let today = index
        .today
        .map(|t| format!(" Today is {t}: when several documents answer, use the one still valid, not an expired one."))
        .unwrap_or_default();
    let system = format!(
        "You answer questions about a person's own documents (Romanian insurance policies, warranties, contracts, \
                  identity papers, bills), using only the numbered sources given. Answer in the language of the question, \
                  in one or two sentences, with exact dates written as in the source. List the numbers of the sources you used. \
                  If the sources do not hold the answer, set found to false and say so.{today}"
    );
    let user = format!("Sources:\n{sources}Question: {question}");
    let schema = json!({
        "type": "object",
        "properties": {
            "found": {"type": "boolean"},
            "answer": {"type": "string", "maxLength": 400},
            "sources": {"type": "array", "items": {"type": "integer"}, "maxItems": 3}
        },
        "required": ["found", "answer", "sources"]
    })
    .to_string();
    #[derive(Deserialize)]
    struct Reply {
        found: bool,
        answer: String,
        #[serde(default)]
        sources: Vec<usize>,
    }
    let reply = match model
        .complete(system, user, schema)
        .ok()
        .and_then(|raw| serde_json::from_str::<Reply>(&raw).ok())
    {
        Some(r) => r,
        None => return fallback("search"),
    };
    let cited: Vec<&Hit> = reply
        .sources
        .iter()
        .filter_map(|i| i.checked_sub(1).and_then(|i| hits.get(i)))
        .collect();
    if !reply.found {
        return Answer {
            text: reply.answer,
            citations: vec![],
            verified: false,
            method: "model".into(),
        };
    }
    // Every date the answer states must be in a cited source; if not, but in another source, that
    // one is cited instead.
    let stated: Vec<_> = dates::find(&reply.answer)
        .into_iter()
        .map(|f| f.date)
        .collect();
    let has = |h: &Hit, d| {
        dates::find(&h.text).iter().any(|f| f.date == d)
            || h.text.contains(&d.format("%Y-%m-%d").to_string())
    };
    let mut citations: Vec<&Hit> = if cited.is_empty() {
        vec![&hits[0]]
    } else {
        cited
    };
    let mut verified = true;
    for d in &stated {
        if !citations.iter().any(|h| has(h, *d)) {
            match hits.iter().find(|h| has(h, *d)) {
                Some(h) => citations.push(h),
                None => verified = false,
            }
        }
    }
    citations
        .dedup_by(|a, b| a.document_id == b.document_id && a.page == b.page && a.fact == b.fact);
    Answer {
        text: reply.answer,
        citations: citations.into_iter().map(cite).collect(),
        verified,
        method: "model".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn doc(
        id: &str,
        kind: DocKind,
        title: &str,
        text: &str,
        deadline: Option<(DeadlineKind, &str, &str)>,
    ) -> Document {
        Document {
            id: id.into(),
            kind,
            title: title.into(),
            added: "2026-10-01".into(),
            pages: vec![
                Page {
                    number: 1,
                    text: "Pagina de gardă".into(),
                    lines: vec![],
                    width: 0,
                    height: 0,
                    confidence: 90.0,
                },
                Page {
                    number: 2,
                    text: text.into(),
                    lines: vec![],
                    width: 0,
                    height: 0,
                    confidence: 90.0,
                },
            ],
            fields: vec![],
            deadlines: deadline
                .map(|(k, date, quote)| Deadline {
                    id: k.name().into(),
                    kind: k,
                    date: date.into(),
                    label: title.into(),
                    page: 2,
                    quote: quote.into(),
                    source: Source::Model,
                    notice_days: None,
                    done: false,
                })
                .into_iter()
                .collect(),
            extraction: None,
        }
    }

    fn library() -> Vec<Document> {
        vec![
            doc(
                "w1",
                DocKind::Warranty,
                "Garanție mașină de spălat Samsung WW90T",
                "CERTIFICAT DE GARANȚIE Produs: Mașină de spălat rufe Samsung WW90T Data vânzării: 14.03.2025 Termen de garanție: 24 de luni",
                Some((
                    DeadlineKind::WarrantyEnd,
                    "2027-03-14",
                    "Termen de garanție: 24 de luni",
                )),
            ),
            doc(
                "w2",
                DocKind::Warranty,
                "Garanție frigider Arctic",
                "CERTIFICAT DE GARANȚIE Produs: Frigider Arctic AK60 Data vânzării: 02.08.2024 Termen de garanție: 36 de luni",
                Some((
                    DeadlineKind::WarrantyEnd,
                    "2027-08-02",
                    "Termen de garanție: 36 de luni",
                )),
            ),
            doc(
                "r1",
                DocKind::Rca,
                "RCA Dacia Logan B-123-ABC",
                "POLIȚĂ RCA valabilă de la 15.03.2026 până la 14.03.2027 Asigurător Allianz-Țiriac",
                Some((DeadlineKind::RcaExpiry, "2027-03-14", "până la 14.03.2027")),
            ),
            doc(
                "c1",
                DocKind::RentContract,
                "Contract închiriere Str. Lalelelor 5",
                "CONTRACT DE ÎNCHIRIERE chiria lunară este de 2.500 lei, plătibilă până pe data de 5 a fiecărei luni",
                None,
            ),
        ]
    }

    #[test]
    fn stems() {
        assert_eq!(stem("masina"), "masin");
        assert_eq!(stem("masinii"), "masin");
        assert_eq!(stem("masinile"), "masin");
        assert_eq!(stem("garantiei"), "garanti");
        assert_eq!(stem("rca"), "rca");
        assert_eq!(stem("2027"), "2027");
    }

    #[test]
    fn finds_the_right_document_and_page() {
        let idx = Index::build(&library());
        let h = idx.search("când expiră garanția la mașina de spălat?", 3);
        assert_eq!((h[0].document_id.as_str(), h[0].page), ("w1", 2));
        assert!(
            h[0].fact,
            "the deadline fact comes first for a 'when' question"
        );
        let h = idx.search("cât e chiria?", 3);
        assert_eq!(h[0].document_id, "c1");
        let h = idx.search("asigurare masina", 3);
        assert_eq!(h[0].document_id, "r1");
        assert!(idx.search("zzzz qqqq", 3).is_empty());
    }

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
    fn answers_cite_and_dates_are_checked() {
        let idx = Index::build(&library());
        let q = "când expiră garanția la mașina de spălat?";
        let good = Fixed(
            r#"{"found":true,"answer":"Garanția expiră pe 14.03.2027.","sources":[1]}"#.into(),
        );
        let a = ask(&idx, q, Some(&good), "ro");
        assert!(a.verified);
        assert_eq!(
            (a.citations[0].document_id.as_str(), a.citations[0].page),
            ("w1", 2)
        );
        // A date that no source holds is flagged.
        let bad = Fixed(
            r#"{"found":true,"answer":"Garanția expiră pe 01.01.2030.","sources":[1]}"#.into(),
        );
        assert!(!ask(&idx, q, Some(&bad), "ro").verified);
        // A right date with the wrong source cited: the source that has it is added.
        let wrong =
            Fixed(r#"{"found":true,"answer":"Expiră pe 2027-08-02.","sources":[1]}"#.into());
        let a = ask(&idx, "frigider garantie", Some(&wrong), "ro");
        assert!(a.verified && a.citations.iter().any(|c| c.document_id == "w2"));
        // No model: the best passage.
        let a = ask(&idx, q, None, "ro");
        assert_eq!(
            (a.method.as_str(), a.citations[0].document_id.as_str()),
            ("search", "w1")
        );
    }
}
