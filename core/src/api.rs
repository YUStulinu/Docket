//! The interface the Android app (and the desktop CLI) uses: one `Docket` object per open vault.
//!
//! Documents are kept decrypted in memory while the vault is open (a few hundred documents of a
//! few pages each); the search index is built for each question, with the day's date.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, NaiveDate, Utc};

use crate::crypto::Key;
use crate::extract;
use crate::llm::LanguageModel;
use crate::model::*;
use crate::reminders::{self, Reminder};
use crate::search::{self, Answer, Hit, Index};
use crate::vault::{self, Vault};
use crate::{Error, ics};

/// A page as the scanner hands it over: the cleaned image and its OCR.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PageInput {
    /// JPEG of the corrected page.
    pub image: Vec<u8>,
    pub text: String,
    pub lines: Vec<Line>,
    pub width: u32,
    pub height: u32,
    pub confidence: f32,
}

#[derive(uniffi::Object)]
pub struct Docket {
    vault: Vault,
    state: Mutex<State>,
    model: Mutex<Option<Arc<dyn LanguageModel>>>,
}

struct State {
    docs: Vec<Document>,
}

fn date(s: &str) -> Result<NaiveDate, Error> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| Error::Invalid(format!("not a YYYY-MM-DD date: {s:?}")))
}

/// A new random vault key (the app protects it with the Android Keystore).
#[uniffi::export]
pub fn generate_key() -> Vec<u8> {
    Key::random().as_bytes().to_vec()
}

/// The deadline a kind of document usually has (for a deadline added by hand).
#[uniffi::export]
pub fn typical_deadline(kind: DocKind) -> DeadlineKind {
    DeadlineKind::typical_for(kind).unwrap_or(DeadlineKind::Other)
}

impl Docket {
    fn from_vault(vault: Vault) -> Result<Arc<Docket>, Error> {
        let docs = vault.all()?;
        Ok(Arc::new(Docket {
            vault,
            state: Mutex::new(State { docs }),
            model: Mutex::new(None),
        }))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn current_model(&self) -> Option<Arc<dyn LanguageModel>> {
        self.model.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Stores a changed document.
    fn save(&self, doc: Document) -> Result<Document, Error> {
        self.vault.put(&doc)?;
        let mut st = self.state();
        match st.docs.iter_mut().find(|d| d.id == doc.id) {
            Some(d) => *d = doc.clone(),
            None => st.docs.push(doc.clone()),
        }
        Ok(doc)
    }

    /// An index that knows today's date (which deadlines have passed); a few hundred documents
    /// index in milliseconds.
    fn index_at(&self, today: &str) -> Index {
        let st = self.state();
        Index::build_at(&st.docs, NaiveDate::parse_from_str(today, "%Y-%m-%d").ok())
    }

    fn edit(
        &self,
        id: &str,
        f: impl FnOnce(&mut Document) -> Result<(), Error>,
    ) -> Result<Document, Error> {
        let mut doc = self.document(id.to_string())?;
        f(&mut doc)?;
        self.save(doc)
    }

    pub fn create_with_passphrase(dir: &Path, passphrase: &str) -> Result<Arc<Docket>, Error> {
        Self::from_vault(Vault::create_with_passphrase(
            dir,
            passphrase,
            Default::default(),
        )?)
    }

    pub fn open_with_passphrase(dir: &Path, passphrase: &str) -> Result<Arc<Docket>, Error> {
        Self::from_vault(Vault::open_with_passphrase(dir, passphrase)?)
    }
}

#[uniffi::export]
impl Docket {
    /// Creates a new vault in `dir` with a 32-byte key.
    #[uniffi::constructor]
    pub fn create(dir: String, key: Vec<u8>) -> Result<Arc<Docket>, Error> {
        Self::from_vault(Vault::create(Path::new(&dir), Key::from_bytes(&key)?)?)
    }

    /// Opens the vault in `dir`; fails with BadKey if the key is not its key.
    #[uniffi::constructor]
    pub fn open(dir: String, key: Vec<u8>) -> Result<Arc<Docket>, Error> {
        Self::from_vault(Vault::open(Path::new(&dir), Key::from_bytes(&key)?)?)
    }

    /// The language model for extraction and questions (None: rules and search only).
    pub fn set_model(&self, model: Option<Arc<dyn LanguageModel>>) {
        *self.model.lock().unwrap_or_else(|e| e.into_inner()) = model;
    }

    /// Adds a scanned document: stores its pages encrypted, then extracts what it says. Takes
    /// seconds with a model: call it off the main thread.
    pub fn add_document(&self, pages: Vec<PageInput>, today: String) -> Result<Document, Error> {
        if pages.is_empty() {
            return Err(Error::Invalid("a document needs at least one page".into()));
        }
        date(&today)?;
        let id = vault::new_id();
        let mut ps = Vec::new();
        for (i, p) in pages.into_iter().enumerate() {
            let n = i as u32 + 1;
            self.vault.put_page_image(&id, n, &p.image)?;
            ps.push(Page {
                number: n,
                text: p.text,
                lines: p.lines,
                width: p.width,
                height: p.height,
                confidence: p.confidence,
            });
        }
        let model = self.current_model();
        let e = extract::extract(&ps, model.as_deref());
        let doc = Document {
            id,
            kind: e.kind,
            title: e.title,
            added: today,
            pages: ps,
            fields: e.fields,
            deadlines: e.deadlines,
            extraction: Some(e.extraction),
        };
        self.save(doc)
    }

    /// Extracts again (with a model added since, say). What you set by hand stays.
    pub fn reextract(&self, id: String) -> Result<Document, Error> {
        let model = self.current_model();
        self.edit(&id, |doc| {
            let e = extract::extract(&doc.pages, model.as_deref());
            let user_fields: Vec<Field> = doc
                .fields
                .iter()
                .filter(|f| f.source == Source::User)
                .cloned()
                .collect();
            let user_deadlines: Vec<Deadline> = doc
                .deadlines
                .iter()
                .filter(|d| d.source == Source::User)
                .cloned()
                .collect();
            doc.kind = e.kind;
            doc.title = e.title;
            let mut fields: Vec<Field> = e
                .fields
                .into_iter()
                .filter(|f| !user_fields.iter().any(|u| u.key == f.key))
                .collect();
            fields.extend(user_fields);
            doc.fields = fields;
            let mut deadlines: Vec<Deadline> = e
                .deadlines
                .into_iter()
                .filter(|d| !user_deadlines.iter().any(|u| u.kind == d.kind))
                .collect();
            deadlines.extend(user_deadlines);
            doc.deadlines = deadlines;
            doc.deadlines.sort_by(|a, b| a.date.cmp(&b.date));
            extract::assign_ids(&mut doc.deadlines);
            doc.extraction = Some(e.extraction);
            Ok(())
        })
    }

    /// The documents, the next deadline first.
    pub fn documents(&self, today: String) -> Vec<Summary> {
        let st = self.state();
        let mut out: Vec<Summary> = st.docs.iter().map(|d| d.summary(&today)).collect();
        out.sort_by(|a, b| {
            let ka = a
                .next_deadline
                .as_ref()
                .map(|d| d.date.clone())
                .unwrap_or_else(|| "9999".into());
            let kb = b
                .next_deadline
                .as_ref()
                .map(|d| d.date.clone())
                .unwrap_or_else(|| "9999".into());
            ka.cmp(&kb).then(b.added.cmp(&a.added))
        });
        out
    }

    pub fn document(&self, id: String) -> Result<Document, Error> {
        self.state()
            .docs
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or(Error::NotFound(format!("document {id}")))
    }

    pub fn page_image(&self, id: String, page: u32) -> Result<Vec<u8>, Error> {
        self.vault.page_image(&id, page)
    }

    pub fn set_title(&self, id: String, title: String) -> Result<Document, Error> {
        self.edit(&id, |d| {
            d.title = title;
            Ok(())
        })
    }

    pub fn set_kind(&self, id: String, kind: DocKind) -> Result<Document, Error> {
        self.edit(&id, |d| {
            d.kind = kind;
            Ok(())
        })
    }

    /// Adds or changes a deadline by hand (by its id; an empty id adds one).
    pub fn set_deadline(&self, id: String, deadline: Deadline) -> Result<Document, Error> {
        date(&deadline.date)?;
        self.edit(&id, |d| {
            let mut dl = deadline;
            dl.source = Source::User;
            match d
                .deadlines
                .iter_mut()
                .find(|x| !dl.id.is_empty() && x.id == dl.id)
            {
                Some(x) => *x = dl,
                None => {
                    dl.id = String::new();
                    d.deadlines.push(dl);
                    d.deadlines.sort_by(|a, b| a.date.cmp(&b.date));
                    let keep: Vec<String> = d.deadlines.iter().map(|x| x.id.clone()).collect();
                    // Number the new one without renaming the others (their reminders keep their ids).
                    for x in d.deadlines.iter_mut().filter(|x| x.id.is_empty()) {
                        let mut n = 1;
                        let mut candidate = x.kind.name().to_string();
                        while keep.contains(&candidate) {
                            n += 1;
                            candidate = format!("{}-{n}", x.kind.name());
                        }
                        x.id = candidate;
                    }
                }
            }
            Ok(())
        })
    }

    /// Marks a deadline done (renewed, paid) or not.
    pub fn set_deadline_done(
        &self,
        id: String,
        deadline_id: String,
        done: bool,
    ) -> Result<Document, Error> {
        self.edit(&id, |d| {
            let x = d
                .deadlines
                .iter_mut()
                .find(|x| x.id == deadline_id)
                .ok_or(Error::NotFound(deadline_id.clone()))?;
            x.done = done;
            Ok(())
        })
    }

    pub fn remove_deadline(&self, id: String, deadline_id: String) -> Result<Document, Error> {
        self.edit(&id, |d| {
            let before = d.deadlines.len();
            d.deadlines.retain(|x| x.id != deadline_id);
            if d.deadlines.len() == before {
                Err(Error::NotFound(deadline_id.clone()))
            } else {
                Ok(())
            }
        })
    }

    pub fn set_field(&self, id: String, field: Field) -> Result<Document, Error> {
        self.edit(&id, |d| {
            let mut f = field;
            f.source = Source::User;
            d.fields.retain(|x| x.key != f.key);
            d.fields.push(f);
            Ok(())
        })
    }

    pub fn delete_document(&self, id: String) -> Result<(), Error> {
        self.vault.delete(&id)?;
        let mut st = self.state();
        st.docs.retain(|d| d.id != id);
        Ok(())
    }

    /// Reminders from `today` on ("en" or "ro").
    pub fn reminders(&self, today: String, locale: String) -> Result<Vec<Reminder>, Error> {
        Ok(reminders::plan(&self.state().docs, date(&today)?, &locale))
    }

    /// The calendar (.ics) of every deadline and its reminders. `now` is RFC 3339.
    pub fn calendar(&self, today: String, now: String, locale: String) -> Result<String, Error> {
        let now: DateTime<Utc> = DateTime::parse_from_rfc3339(&now)
            .map_err(|e| Error::Invalid(e.to_string()))?
            .with_timezone(&Utc);
        Ok(ics::calendar(
            &self.state().docs,
            date(&today)?,
            now,
            &locale,
        ))
    }

    pub fn search(&self, query: String, limit: u32, today: String) -> Vec<Hit> {
        self.index_at(&today).search(&query, limit as usize)
    }

    /// Answers a question, citing the document and page. Documents still valid on `today` are
    /// preferred over expired ones.
    pub fn ask(&self, question: String, locale: String, today: String) -> Answer {
        let model = self.current_model();
        let index = self.index_at(&today);
        search::ask(&index, &question, model.as_deref(), &locale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(text: &str) -> PageInput {
        PageInput {
            image: b"jpeg".to_vec(),
            text: text.into(),
            lines: vec![],
            width: 1000,
            height: 1400,
            confidence: 90.0,
        }
    }

    #[test]
    fn whole_flow() {
        let dir = tempfile::tempdir().unwrap();
        let key = generate_key();
        let d = Docket::create(dir.path().to_string_lossy().into(), key.clone()).unwrap();
        let doc = d
            .add_document(vec![page("POLIȚĂ RCA\nnr. înmatriculare B 123 ABC\nvalabilă de la 15.03.2026 până la 14.03.2027")], "2026-10-27".into())
            .unwrap();
        assert_eq!(doc.kind, DocKind::Rca);
        assert_eq!(doc.deadlines[0].date, "2027-03-14");
        d.add_document(vec![page("CERTIFICAT DE GARANȚIE\nMașină de spălat\nData vânzării: 14.03.2025\nTermen de garanție: 24 de luni")], "2026-10-27".into())
            .unwrap();
        // Reopen: everything is read back from the encrypted vault.
        drop(d);
        assert!(Docket::open(dir.path().to_string_lossy().into(), generate_key()).is_err());
        let d = Docket::open(dir.path().to_string_lossy().into(), key).unwrap();
        let list = d.documents("2026-10-27".into());
        assert_eq!(list.len(), 2);
        assert_eq!(d.page_image(doc.id.clone(), 1).unwrap(), b"jpeg");
        // A deadline set by hand survives re-extraction.
        let mut dl = d.document(doc.id.clone()).unwrap().deadlines[0].clone();
        dl.date = "2027-03-20".into();
        d.set_deadline(doc.id.clone(), dl).unwrap();
        let again = d.reextract(doc.id.clone()).unwrap();
        assert_eq!(
            again
                .deadlines
                .iter()
                .filter(|x| x.kind == DeadlineKind::RcaExpiry)
                .count(),
            1
        );
        assert_eq!(again.deadlines[0].date, "2027-03-20");
        assert_eq!(again.deadlines[0].source, Source::User);
        // Reminders, the calendar, search.
        let r = d.reminders("2026-10-27".into(), "en".into()).unwrap();
        assert!(r.iter().any(|x| x.deadline_date == "2027-03-20"));
        let cal = d
            .calendar(
                "2026-10-27".into(),
                "2026-10-27T10:00:00Z".into(),
                "en".into(),
            )
            .unwrap();
        assert!(cal.contains("DTSTART;VALUE=DATE:20270320"));
        let a = d.ask(
            "când expiră garanția?".into(),
            "ro".into(),
            "2026-10-27".into(),
        );
        assert_eq!(a.citations[0].title, "Warranty");
        // Done deadlines stop reminding; deleting removes everything.
        d.set_deadline_done(doc.id.clone(), "rca_expiry".into(), true)
            .unwrap();
        assert!(
            !d.reminders("2026-10-27".into(), "en".into())
                .unwrap()
                .iter()
                .any(|x| x.document_id == doc.id)
        );
        d.delete_document(doc.id.clone()).unwrap();
        assert_eq!(d.documents("2026-10-27".into()).len(), 1);
        assert!(d.page_image(doc.id, 1).is_err());
    }

    #[test]
    fn bad_input() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Docket::create(dir.path().to_string_lossy().into(), vec![1, 2, 3]).is_err());
        let d = Docket::create(dir.path().to_string_lossy().into(), generate_key()).unwrap();
        assert!(d.add_document(vec![], "2026-10-27".into()).is_err());
        assert!(
            d.add_document(vec![page("x")], "27.10.2026".into())
                .is_err()
        );
        assert!(d.document("nope".into()).is_err());
    }
}
