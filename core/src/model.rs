//! What Docket knows about a document. Dates cross the FFI boundary as "YYYY-MM-DD" strings.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "snake_case")]
pub enum DocKind {
    /// Certificat de înmatriculare (the "talon"), with the ITP validity.
    VehicleRegistration,
    /// Poliță RCA (mandatory car insurance).
    Rca,
    Casco,
    /// The ITP inspection report.
    Itp,
    Passport,
    IdCard,
    DrivingLicence,
    /// Certificat de garanție.
    Warranty,
    RentContract,
    Invoice,
    Other,
}

impl DocKind {
    pub const ALL: [DocKind; 11] = [
        DocKind::VehicleRegistration,
        DocKind::Rca,
        DocKind::Casco,
        DocKind::Itp,
        DocKind::Passport,
        DocKind::IdCard,
        DocKind::DrivingLicence,
        DocKind::Warranty,
        DocKind::RentContract,
        DocKind::Invoice,
        DocKind::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            DocKind::VehicleRegistration => "vehicle_registration",
            DocKind::Rca => "rca",
            DocKind::Casco => "casco",
            DocKind::Itp => "itp",
            DocKind::Passport => "passport",
            DocKind::IdCard => "id_card",
            DocKind::DrivingLicence => "driving_licence",
            DocKind::Warranty => "warranty",
            DocKind::RentContract => "rent_contract",
            DocKind::Invoice => "invoice",
            DocKind::Other => "other",
        }
    }

    pub fn from_name(s: &str) -> Option<DocKind> {
        DocKind::ALL.into_iter().find(|k| k.name() == s)
    }

    /// What the document is, for the language model and the UI.
    pub fn describe(self) -> &'static str {
        match self {
            DocKind::VehicleRegistration => {
                "vehicle registration certificate (certificat de înmatriculare, talon), with the ITP validity stamp"
            }
            DocKind::Rca => "RCA policy (mandatory third-party car insurance)",
            DocKind::Casco => "CASCO policy (optional car insurance)",
            DocKind::Itp => "ITP periodic technical inspection report",
            DocKind::Passport => "passport",
            DocKind::IdCard => "identity card (carte de identitate)",
            DocKind::DrivingLicence => "driving licence (permis de conducere)",
            DocKind::Warranty => {
                "warranty certificate (certificat de garanție) or a receipt with a warranty"
            }
            DocKind::RentContract => "rent contract (contract de închiriere)",
            DocKind::Invoice => "invoice or bill (factură) with a due date",
            DocKind::Other => "another document",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "snake_case")]
pub enum DeadlineKind {
    ItpExpiry,
    RcaExpiry,
    CascoExpiry,
    PassportExpiry,
    IdCardExpiry,
    LicenceExpiry,
    WarrantyEnd,
    ContractEnd,
    InvoiceDue,
    Other,
}

impl DeadlineKind {
    pub const ALL: [DeadlineKind; 10] = [
        DeadlineKind::ItpExpiry,
        DeadlineKind::RcaExpiry,
        DeadlineKind::CascoExpiry,
        DeadlineKind::PassportExpiry,
        DeadlineKind::IdCardExpiry,
        DeadlineKind::LicenceExpiry,
        DeadlineKind::WarrantyEnd,
        DeadlineKind::ContractEnd,
        DeadlineKind::InvoiceDue,
        DeadlineKind::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            DeadlineKind::ItpExpiry => "itp_expiry",
            DeadlineKind::RcaExpiry => "rca_expiry",
            DeadlineKind::CascoExpiry => "casco_expiry",
            DeadlineKind::PassportExpiry => "passport_expiry",
            DeadlineKind::IdCardExpiry => "id_card_expiry",
            DeadlineKind::LicenceExpiry => "licence_expiry",
            DeadlineKind::WarrantyEnd => "warranty_end",
            DeadlineKind::ContractEnd => "contract_end",
            DeadlineKind::InvoiceDue => "invoice_due",
            DeadlineKind::Other => "other",
        }
    }

    pub fn from_name(s: &str) -> Option<DeadlineKind> {
        DeadlineKind::ALL.into_iter().find(|k| k.name() == s)
    }

    pub fn describe(self) -> &'static str {
        match self {
            DeadlineKind::ItpExpiry => "the date the ITP inspection stops being valid",
            DeadlineKind::RcaExpiry => "the last day the RCA policy covers",
            DeadlineKind::CascoExpiry => "the last day the CASCO policy covers",
            DeadlineKind::PassportExpiry => "the passport's expiry date",
            DeadlineKind::IdCardExpiry => "the identity card's expiry date",
            DeadlineKind::LicenceExpiry => "the driving licence's expiry date",
            DeadlineKind::WarrantyEnd => {
                "the last day of the warranty (purchase date plus the warranty period)"
            }
            DeadlineKind::ContractEnd => "the date the contract ends",
            DeadlineKind::InvoiceDue => "the payment due date (scadență)",
            DeadlineKind::Other => "another date that needs action",
        }
    }

    /// The deadline a document kind usually has.
    pub fn typical_for(kind: DocKind) -> Option<DeadlineKind> {
        Some(match kind {
            DocKind::VehicleRegistration | DocKind::Itp => DeadlineKind::ItpExpiry,
            DocKind::Rca => DeadlineKind::RcaExpiry,
            DocKind::Casco => DeadlineKind::CascoExpiry,
            DocKind::Passport => DeadlineKind::PassportExpiry,
            DocKind::IdCard => DeadlineKind::IdCardExpiry,
            DocKind::DrivingLicence => DeadlineKind::LicenceExpiry,
            DocKind::Warranty => DeadlineKind::WarrantyEnd,
            DocKind::RentContract => DeadlineKind::ContractEnd,
            DocKind::Invoice => DeadlineKind::InvoiceDue,
            DocKind::Other => return None,
        })
    }
}

/// Where a value came from: the user's word beats the model's, which beats the rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Rules,
    Model,
    User,
}

/// A line of OCR text and where it is on the page image (pixels).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct Line {
    pub text: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// Tesseract's confidence, 0–100.
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct Page {
    /// 1-based.
    pub number: u32,
    pub text: String,
    pub lines: Vec<Line>,
    pub width: u32,
    pub height: u32,
    /// Mean OCR confidence of the page, 0–100.
    pub confidence: f32,
}

/// An important value on the document: a plate number, an insurer, a product.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct Field {
    pub key: String,
    pub value: String,
    pub page: u32,
    /// The text on the page it was read from.
    pub quote: String,
    pub source: Source,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct Deadline {
    /// Stable within the document (its kind, numbered if it repeats).
    pub id: String,
    pub kind: DeadlineKind,
    /// YYYY-MM-DD.
    pub date: String,
    /// What it is, in the user's words or the document's ("ITP B-123-ABC").
    pub label: String,
    pub page: u32,
    pub quote: String,
    pub source: Source,
    /// For a contract: the notice period (preaviz) in days.
    pub notice_days: Option<u32>,
    /// The user does not want reminders for it any more (renewed, sold the car).
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct Extraction {
    /// "rules" or the model's name.
    pub method: String,
    pub seconds: f64,
    /// What the checks threw away: values whose quote is not on the page, dates that do not parse.
    pub rejected: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct Document {
    pub id: String,
    pub kind: DocKind,
    pub title: String,
    /// YYYY-MM-DD, when it was added.
    pub added: String,
    pub pages: Vec<Page>,
    pub fields: Vec<Field>,
    pub deadlines: Vec<Deadline>,
    pub extraction: Option<Extraction>,
}

/// A document as the list shows it (no page text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct Summary {
    pub id: String,
    pub kind: DocKind,
    pub title: String,
    pub added: String,
    pub pages: u32,
    /// The next deadline that is not done, if any.
    pub next_deadline: Option<Deadline>,
}

impl Document {
    pub fn summary(&self, today: &str) -> Summary {
        let next = self
            .deadlines
            .iter()
            .filter(|d| !d.done && d.date.as_str() >= today)
            .min_by(|a, b| a.date.cmp(&b.date))
            .or_else(|| {
                self.deadlines
                    .iter()
                    .filter(|d| !d.done)
                    .max_by(|a, b| a.date.cmp(&b.date))
            })
            .cloned();
        Summary {
            id: self.id.clone(),
            kind: self.kind,
            title: self.title.clone(),
            added: self.added.clone(),
            pages: self.pages.len() as u32,
            next_deadline: next,
        }
    }

    pub fn page(&self, n: u32) -> Option<&Page> {
        self.pages.iter().find(|p| p.number == n)
    }
}
