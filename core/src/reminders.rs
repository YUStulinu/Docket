//! When to remind, and of what.
//!
//! A deadline is not one reminder: each kind has its own lead times, set by what has to happen
//! before it. An ITP needs an appointment, and stations are often booked two weeks ahead. A
//! passport takes weeks to issue, and many countries want six months of validity left. A rent
//! contract has to be terminated a notice period before it ends.
//!
//! Reminders that ask you to do something (book, pay, give notice) are moved to the last working
//! day before, skipping weekends and Romania's public holidays. A reminder whose date has passed
//! while the deadline has not is collapsed into one reminder today.

use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::dates;
use crate::model::{DeadlineKind, Document};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum Urgency {
    /// Ahead of time: plan.
    Plan,
    /// Do it now.
    Act,
    /// The deadline is today or tomorrow.
    Last,
    /// The deadline has passed.
    Overdue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct Reminder {
    /// Stable: "<document>/<deadline>/<days before>", so the app can replace a scheduled
    /// notification instead of adding a second one.
    pub id: String,
    pub document_id: String,
    pub deadline_id: String,
    /// YYYY-MM-DD: the day to notify (at 09:00 local time).
    pub date: String,
    pub deadline_date: String,
    pub title: String,
    pub body: String,
    pub urgency: Urgency,
    /// Worth its own calendar event (booking the ITP, the last day for the notice), not only an
    /// alarm on the deadline.
    pub own_event: bool,
}

/// One reminder of a policy: so many days before, and whether it asks for an action.
#[derive(Debug, Clone, Copy)]
struct Step {
    days: i64,
    action: bool,
    own_event: bool,
    text: Text,
}

#[derive(Debug, Clone, Copy)]
enum Text {
    ItpPlan,
    ItpBook,
    Soon,
    Today,
    InsuranceCompare,
    InsuranceRenew,
    InsuranceLastDay,
    PassportSixMonths,
    PassportBook,
    IdBook,
    LicenceBook,
    WarrantyCheck,
    ContractDecide,
    ContractNoticeDay,
    ContractEnds,
    InvoicePay,
    InvoiceToday,
}

fn step(days: i64, action: bool, own_event: bool, text: Text) -> Step {
    Step {
        days,
        action,
        own_event,
        text,
    }
}

fn policy(kind: DeadlineKind, notice_days: Option<u32>) -> Vec<Step> {
    use Text::*;
    match kind {
        DeadlineKind::ItpExpiry => vec![
            step(30, false, false, ItpPlan),
            step(14, true, true, ItpBook),
            step(3, false, false, Soon),
            step(0, false, false, Today),
        ],
        DeadlineKind::RcaExpiry | DeadlineKind::CascoExpiry => {
            vec![
                step(30, false, false, InsuranceCompare),
                step(7, true, false, InsuranceRenew),
                step(1, false, false, InsuranceLastDay),
            ]
        }
        DeadlineKind::PassportExpiry => {
            vec![
                step(270, false, false, PassportSixMonths),
                step(90, true, true, PassportBook),
                step(30, false, false, Soon),
                step(0, false, false, Today),
            ]
        }
        DeadlineKind::IdCardExpiry => vec![
            step(60, true, true, IdBook),
            step(14, false, false, Soon),
            step(0, false, false, Today),
        ],
        DeadlineKind::LicenceExpiry => vec![
            step(60, true, true, LicenceBook),
            step(14, false, false, Soon),
            step(0, false, false, Today),
        ],
        DeadlineKind::WarrantyEnd => vec![
            step(30, false, false, WarrantyCheck),
            step(7, false, false, Soon),
        ],
        DeadlineKind::ContractEnd => match notice_days {
            Some(n) if n > 0 => vec![
                step(n as i64 + 14, false, false, ContractDecide),
                step(n as i64, true, true, ContractNoticeDay),
                step(0, false, false, ContractEnds),
            ],
            _ => vec![
                step(60, false, false, ContractDecide),
                step(30, false, false, Soon),
                step(0, false, false, ContractEnds),
            ],
        },
        DeadlineKind::InvoiceDue => vec![
            step(3, true, false, InvoicePay),
            step(0, true, false, InvoiceToday),
        ],
        DeadlineKind::Other => vec![step(7, false, false, Soon), step(0, false, false, Today)],
    }
}

fn fmt_date(d: NaiveDate, ro: bool) -> String {
    const EN: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    const RO: [&str; 12] = [
        "ian.", "feb.", "mar.", "apr.", "mai", "iun.", "iul.", "aug.", "sept.", "oct.", "nov.",
        "dec.",
    ];
    use chrono::Datelike;
    let m = d.month0() as usize;
    if ro {
        format!("{} {} {}", d.day(), RO[m], d.year())
    } else {
        format!("{} {} {}", d.day(), EN[m], d.year())
    }
}

/// (title, body) in English or Romanian.
fn words(
    t: Text,
    label: &str,
    date: &str,
    days: i64,
    notice: Option<u32>,
    ro: bool,
) -> (String, String) {
    use Text::*;
    let n = notice.unwrap_or(0);
    let (en, ro_) = match t {
        ItpPlan => (
            (format!("{label}: ITP expires on {date}"), "Plan the inspection: stations are often booked two weeks ahead.".to_string()),
            (format!("{label}: ITP expiră pe {date}"), "Planifică inspecția: stațiile sunt adesea ocupate cu două săptămâni înainte.".to_string()),
        ),
        ItpBook => (
            (format!("Book the ITP for {label}"), format!("It expires on {date}. Driving without a valid ITP is fined, and the RCA can be refused.")),
            (format!("Programează ITP-ul pentru {label}"), format!("Expiră pe {date}. Fără ITP valabil se dă amendă, iar RCA-ul poate fi refuzat.")),
        ),
        Soon => (
            (format!("{label}: {days} days left"), format!("The deadline is {date}.")),
            (format!("{label}: mai sunt {days} zile"), format!("Termenul este {date}.")),
        ),
        Today => (
            (format!("{label}: expires today"), format!("{date} is the last day.")),
            (format!("{label}: expiră azi"), format!("{date} este ultima zi.")),
        ),
        InsuranceCompare => (
            (format!("{label}: the policy ends on {date}"), "Compare offers now; a new policy can start the day after the old one ends.".to_string()),
            (format!("{label}: polița se termină pe {date}"), "Compară ofertele acum; noua poliță poate începe a doua zi după ce se termină cea veche.".to_string()),
        ),
        InsuranceRenew => (
            (format!("Renew {label}"), format!("It covers until {date}. Without RCA the car may not be driven, and the fine is steep.")),
            (format!("Reînnoiește {label}"), format!("Acoperă până pe {date}. Fără RCA mașina nu poate circula, iar amenda e mare.")),
        ),
        InsuranceLastDay => (
            (format!("{label} ends tomorrow"), format!("The last day covered is {date}.")),
            (format!("{label} se termină mâine"), format!("Ultima zi acoperită este {date}.")),
        ),
        PassportSixMonths => (
            (format!("{label}: expires on {date}"), "Many countries want six months of validity left at entry: renew before you book a trip.".to_string()),
            (format!("{label}: expiră pe {date}"), "Multe țări cer șase luni de valabilitate la intrare: reînnoiește-l înainte să rezervi o călătorie.".to_string()),
        ),
        PassportBook => (
            (format!("Book a passport appointment ({label})"), format!("It expires on {date}; issuing takes several weeks.")),
            (format!("Programează-te pentru pașaport ({label})"), format!("Expiră pe {date}; emiterea durează câteva săptămâni.")),
        ),
        IdBook => (
            (format!("Book the new ID card ({label})"), format!("It expires on {date}. Appointments at the population records office (SPCLEP) fill up.")),
            (format!("Programează-te pentru buletin ({label})"), format!("Expiră pe {date}. Programările la evidența populației (SPCLEP) se ocupă repede.")),
        ),
        LicenceBook => (
            (format!("Renew the driving licence ({label})"), format!("It expires on {date}. Book at the DRPCIV and get the medical check first.")),
            (format!("Reînnoiește permisul ({label})"), format!("Expiră pe {date}. Programează-te la DRPCIV și fă întâi fișa medicală.")),
        ),
        WarrantyCheck => (
            (format!("{label}: warranty ends on {date}"), "Check that everything works; report a defect while it is still covered.".to_string()),
            (format!("{label}: garanția se termină pe {date}"), "Verifică dacă totul funcționează; anunță un defect cât timp e încă în garanție.".to_string()),
        ),
        ContractDecide => (
            (format!("{label}: decide whether to renew"), if n > 0 { format!("The contract ends on {date}; notice ({n} days) is due two weeks from now.") } else { format!("The contract ends on {date}.") }),
            (format!("{label}: decide dacă prelungești"), if n > 0 { format!("Contractul se termină pe {date}; preavizul ({n} zile) trebuie dat peste două săptămâni.") } else { format!("Contractul se termină pe {date}.") }),
        ),
        ContractNoticeDay => (
            (format!("{label}: last day to give notice"), format!("The notice period is {n} days and the contract ends on {date}.")),
            (format!("{label}: ultima zi pentru preaviz"), format!("Preavizul este de {n} zile, iar contractul se termină pe {date}.")),
        ),
        ContractEnds => (
            (format!("{label}: the contract ends"), format!("{date} is the last day.")),
            (format!("{label}: contractul se termină"), format!("{date} este ultima zi.")),
        ),
        InvoicePay => (
            (format!("Pay {label}"), format!("Due on {date}.")),
            (format!("Plătește {label}"), format!("Scadența este pe {date}.")),
        ),
        InvoiceToday => (
            (format!("{label}: payment due"), format!("Due on {date}; a transfer made today may arrive late.")),
            (format!("{label}: plata e scadentă"), format!("Scadența este pe {date}; un transfer făcut azi poate ajunge cu întârziere.")),
        ),
    };
    if ro { ro_ } else { en }
}

/// The reminders for every deadline that is not done, from `today` on, sorted by date. Deadlines
/// already passed give one Overdue entry dated today (shown, not notified again and again).
pub fn plan(docs: &[Document], today: NaiveDate, locale: &str) -> Vec<Reminder> {
    let ro = locale.starts_with("ro");
    let mut out = Vec::new();
    for doc in docs {
        for d in doc.deadlines.iter().filter(|d| !d.done) {
            let Some(date) = dates::parse_one(&d.date) else {
                continue;
            };
            let label = if d.label.is_empty() {
                doc.title.as_str()
            } else {
                d.label.as_str()
            };
            let date_s = fmt_date(date, ro);
            let mk = |id_part: &str,
                      on: NaiveDate,
                      t: Text,
                      days: i64,
                      urgency: Urgency,
                      own_event: bool| {
                let (title, body) = words(t, label, &date_s, days, d.notice_days, ro);
                Reminder {
                    id: format!("{}/{}/{}", doc.id, d.id, id_part),
                    document_id: doc.id.clone(),
                    deadline_id: d.id.clone(),
                    date: on.format("%Y-%m-%d").to_string(),
                    deadline_date: d.date.clone(),
                    title,
                    body,
                    urgency,
                    own_event,
                }
            };
            if date < today {
                let (title, body) = if ro {
                    (
                        format!("{label}: termenul a trecut"),
                        format!("A expirat pe {date_s}."),
                    )
                } else {
                    (
                        format!("{label}: the deadline has passed"),
                        format!("It expired on {date_s}."),
                    )
                };
                out.push(Reminder {
                    id: format!("{}/{}/overdue", doc.id, d.id),
                    document_id: doc.id.clone(),
                    deadline_id: d.id.clone(),
                    date: today.format("%Y-%m-%d").to_string(),
                    deadline_date: d.date.clone(),
                    title,
                    body,
                    urgency: Urgency::Overdue,
                    own_event: false,
                });
                continue;
            }
            let steps = policy(d.kind, d.notice_days);
            let mut missed: Option<Step> = None;
            for s in &steps {
                let mut on = date - Duration::days(s.days);
                if s.action {
                    on = dates::working_day_on_or_before(on);
                }
                let urgency = match (s.days, s.action) {
                    (0..=1, _) => Urgency::Last,
                    (_, true) => Urgency::Act,
                    _ => Urgency::Plan,
                };
                if on < today {
                    missed = Some(*s);
                    continue;
                }
                // A missed step is said today, once, unless a later step is today anyway.
                if let Some(m) = missed.take() {
                    if on > today {
                        let left = (date - today).num_days();
                        out.push(mk(
                            &m.days.to_string(),
                            today,
                            m.text,
                            left,
                            if m.action {
                                Urgency::Act
                            } else {
                                Urgency::Plan
                            },
                            false,
                        ));
                    }
                }
                out.push(mk(
                    &s.days.to_string(),
                    on,
                    s.text,
                    s.days,
                    urgency,
                    s.own_event,
                ));
            }
            if let Some(m) = missed {
                let left = (date - today).num_days();
                out.push(mk(
                    &m.days.to_string(),
                    today,
                    m.text,
                    left,
                    Urgency::Last,
                    false,
                ));
            }
        }
    }
    out.sort_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn doc(kind: DeadlineKind, date: &str, notice: Option<u32>) -> Document {
        Document {
            id: "d1".into(),
            kind: DocKind::Other,
            title: "Title".into(),
            added: "2026-01-01".into(),
            pages: vec![],
            fields: vec![],
            deadlines: vec![Deadline {
                id: kind.name().into(),
                kind,
                date: date.into(),
                label: "Label".into(),
                page: 1,
                quote: String::new(),
                source: Source::Rules,
                notice_days: notice,
                done: false,
            }],
            extraction: None,
        }
    }

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn dates_of(r: &[Reminder]) -> Vec<&str> {
        r.iter().map(|x| x.date.as_str()).collect()
    }

    #[test]
    fn itp_books_two_weeks_ahead_on_a_working_day() {
        // ITP expires Monday 21 June 2027; booking 14 days before is Monday 7 June.
        let r = plan(
            &[doc(DeadlineKind::ItpExpiry, "2027-06-21", None)],
            d("2027-01-01"),
            "en",
        );
        assert_eq!(
            dates_of(&r),
            ["2027-05-22", "2027-06-07", "2027-06-18", "2027-06-21"]
        );
        assert_eq!(r[1].title, "Book the ITP for Label");
        assert!(r[1].own_event && r[1].urgency == Urgency::Act);
        // Expires 15 May 2027: 14 days before is Saturday 1 May (Labour Day); Friday 30 April is
        // Orthodox Good Friday that year, so the booking reminder comes on Thursday 29 April.
        let r = plan(
            &[doc(DeadlineKind::ItpExpiry, "2027-05-15", None)],
            d("2027-01-01"),
            "en",
        );
        assert_eq!(r[1].date, "2027-04-29");
    }

    #[test]
    fn contract_notice() {
        let r = plan(
            &[doc(DeadlineKind::ContractEnd, "2027-10-31", Some(30))],
            d("2027-01-01"),
            "ro",
        );
        // Notice: 31 Oct - 30 days = 1 Oct 2027 (Friday); decide two weeks before that.
        assert_eq!(dates_of(&r), ["2027-09-17", "2027-10-01", "2027-10-31"]);
        assert_eq!(r[1].title, "Label: ultima zi pentru preaviz");
        assert!(r[1].body.contains("30 de zile") || r[1].body.contains("30 zile"));
    }

    #[test]
    fn invoice_due_on_a_sunday_is_paid_on_friday() {
        // 15 Nov 2026 is a Sunday.
        let r = plan(
            &[doc(DeadlineKind::InvoiceDue, "2026-11-15", None)],
            d("2026-11-01"),
            "en",
        );
        assert_eq!(dates_of(&r), ["2026-11-12", "2026-11-13"]);
    }

    #[test]
    fn missed_steps_collapse_into_today_and_passed_deadlines_are_overdue() {
        // Passport expires in 40 days: the 270- and 90-day steps have passed.
        let r = plan(
            &[doc(DeadlineKind::PassportExpiry, "2026-12-10", None)],
            d("2026-10-31"),
            "en",
        );
        assert_eq!(dates_of(&r), ["2026-10-31", "2026-11-10", "2026-12-10"]);
        assert_eq!(r[0].title, "Book a passport appointment (Label)");
        let r = plan(
            &[doc(DeadlineKind::RcaExpiry, "2026-10-01", None)],
            d("2026-10-31"),
            "en",
        );
        assert_eq!(r.len(), 1);
        assert_eq!(
            (r[0].urgency, r[0].date.as_str()),
            (Urgency::Overdue, "2026-10-31")
        );
    }

    #[test]
    fn done_deadlines_are_quiet_and_ids_are_stable() {
        let mut x = doc(DeadlineKind::WarrantyEnd, "2027-03-14", None);
        let a = plan(&[x.clone()], d("2026-10-31"), "en");
        let b = plan(&[x.clone()], d("2026-10-31"), "en");
        assert_eq!(a, b);
        assert_eq!(a[0].id, "d1/warranty_end/30");
        x.deadlines[0].done = true;
        assert!(plan(&[x], d("2026-10-31"), "en").is_empty());
    }
}
