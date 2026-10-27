//! The vault: a folder of sealed records.
//!
//! ```text
//! vault.json             format, the passphrase salt (desktop) and a sealed check value; no secrets
//! docs/<id>.bin          a Document, as sealed JSON
//! pages/<id>-<n>.bin     the image of page n, sealed
//! ```
//!
//! Every file is written to a temporary name and renamed over the old one, so a crash leaves either
//! the old record or the new one, never half of one. Each record's name is its associated data
//! (see crypto.rs): a document file moved to another name does not open.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::crypto::{self, KdfParams, Key};
use crate::model::Document;

const FORMAT: u32 = 1;
const CHECK: &[u8] = b"docket vault check";

#[derive(Serialize, Deserialize)]
struct Header {
    format: u32,
    /// Present when the key comes from a passphrase.
    kdf: Option<Kdf>,
    /// CHECK sealed with the key: tells a wrong key from a damaged record.
    check: String,
}

#[derive(Serialize, Deserialize)]
struct Kdf {
    salt: String,
    params: KdfParams,
}

pub struct Vault {
    dir: PathBuf,
    key: Key,
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), Error> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn valid_id(id: &str) -> Result<(), Error> {
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Error::Invalid(format!("bad document id {id:?}")));
    }
    Ok(())
}

impl Vault {
    /// Creates a vault in `dir` (which must be empty or missing) with a key from the platform.
    pub fn create(dir: &Path, key: Key) -> Result<Vault, Error> {
        Self::create_with(dir, key, None)
    }

    fn create_with(dir: &Path, key: Key, kdf: Option<Kdf>) -> Result<Vault, Error> {
        if dir.join("vault.json").exists() {
            return Err(Error::Invalid(format!(
                "{} already holds a vault",
                dir.display()
            )));
        }
        fs::create_dir_all(dir.join("docs"))?;
        fs::create_dir_all(dir.join("pages"))?;
        let header = Header {
            format: FORMAT,
            kdf,
            check: b64().encode(crypto::seal(&key, "vault.json", CHECK)),
        };
        write_atomic(
            &dir.join("vault.json"),
            &serde_json::to_vec_pretty(&header).unwrap(),
        )?;
        Ok(Vault {
            dir: dir.to_path_buf(),
            key,
        })
    }

    /// Opens a vault with its key; fails if the key is not the vault's.
    pub fn open(dir: &Path, key: Key) -> Result<Vault, Error> {
        let header = Self::header(dir)?;
        let sealed = b64()
            .decode(&header.check)
            .map_err(|e| Error::Corrupt(e.to_string()))?;
        match crypto::open(&key, "vault.json", &sealed) {
            Ok(v) if v == CHECK => Ok(Vault {
                dir: dir.to_path_buf(),
                key,
            }),
            _ => Err(Error::BadKey("this is not the vault's key".into())),
        }
    }

    /// Creates a vault whose key comes from a passphrase (the desktop).
    pub fn create_with_passphrase(
        dir: &Path,
        passphrase: &str,
        params: KdfParams,
    ) -> Result<Vault, Error> {
        let mut salt = [0u8; 16];
        crypto::random_bytes(&mut salt);
        let key = crypto::key_from_passphrase(passphrase, &salt, params)?;
        Self::create_with(
            dir,
            key,
            Some(Kdf {
                salt: hex::encode(salt),
                params,
            }),
        )
    }

    pub fn open_with_passphrase(dir: &Path, passphrase: &str) -> Result<Vault, Error> {
        let header = Self::header(dir)?;
        let kdf = header
            .kdf
            .ok_or_else(|| Error::BadKey("this vault's key is not a passphrase".into()))?;
        let salt = hex::decode(&kdf.salt).map_err(|e| Error::Corrupt(e.to_string()))?;
        let key = crypto::key_from_passphrase(passphrase, &salt, kdf.params)?;
        Self::open(dir, key).map_err(|_| Error::BadKey("wrong passphrase".into()))
    }

    fn header(dir: &Path) -> Result<Header, Error> {
        let data = fs::read(dir.join("vault.json"))
            .map_err(|_| Error::NotFound(format!("no vault in {}", dir.display())))?;
        let h: Header = serde_json::from_slice(&data)
            .map_err(|e| Error::Corrupt(format!("vault.json: {e}")))?;
        if h.format != FORMAT {
            return Err(Error::Corrupt(format!(
                "vault format {} (this version reads {FORMAT})",
                h.format
            )));
        }
        Ok(h)
    }

    fn doc_path(&self, id: &str) -> (PathBuf, String) {
        (
            self.dir.join("docs").join(format!("{id}.bin")),
            format!("docs/{id}"),
        )
    }

    fn page_path(&self, id: &str, n: u32) -> (PathBuf, String) {
        (
            self.dir.join("pages").join(format!("{id}-{n}.bin")),
            format!("pages/{id}-{n}"),
        )
    }

    pub fn put(&self, doc: &Document) -> Result<(), Error> {
        valid_id(&doc.id)?;
        let (path, name) = self.doc_path(&doc.id);
        let json = serde_json::to_vec(doc).map_err(|e| Error::Invalid(e.to_string()))?;
        write_atomic(&path, &crypto::seal(&self.key, &name, &json))
    }

    pub fn get(&self, id: &str) -> Result<Document, Error> {
        valid_id(id)?;
        let (path, name) = self.doc_path(id);
        let sealed = fs::read(&path).map_err(|_| Error::NotFound(format!("document {id}")))?;
        let json = crypto::open(&self.key, &name, &sealed)?;
        serde_json::from_slice(&json).map_err(|e| Error::Corrupt(format!("{name}: {e}")))
    }

    pub fn put_page_image(&self, id: &str, n: u32, image: &[u8]) -> Result<(), Error> {
        valid_id(id)?;
        let (path, name) = self.page_path(id, n);
        write_atomic(&path, &crypto::seal(&self.key, &name, image))
    }

    pub fn page_image(&self, id: &str, n: u32) -> Result<Vec<u8>, Error> {
        valid_id(id)?;
        let (path, name) = self.page_path(id, n);
        let sealed = fs::read(&path).map_err(|_| Error::NotFound(format!("page {n} of {id}")))?;
        crypto::open(&self.key, &name, &sealed)
    }

    /// Every document, sorted by id. A record that does not open is an error: the vault is never
    /// shown half-read.
    pub fn all(&self) -> Result<Vec<Document>, Error> {
        let mut ids: Vec<String> = fs::read_dir(self.dir.join("docs"))?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                e.file_name()
                    .to_str()
                    .and_then(|n| n.strip_suffix(".bin"))
                    .map(String::from)
            })
            .collect();
        ids.sort();
        ids.iter().map(|id| self.get(id)).collect()
    }

    /// Deletes a document and its page images. The files are overwritten with zeros first (on
    /// flash storage that is no guarantee, but the content was encrypted anyway).
    pub fn delete(&self, id: &str) -> Result<(), Error> {
        let doc = self.get(id)?;
        for p in &doc.pages {
            let (path, _) = self.page_path(id, p.number);
            wipe(&path)?;
        }
        wipe(&self.doc_path(id).0)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn wipe(path: &Path) -> Result<(), Error> {
    if let Ok(meta) = fs::metadata(path) {
        fs::write(path, vec![0u8; meta.len() as usize])?;
        fs::remove_file(path)?;
    }
    Ok(())
}

/// A new random document id.
pub fn new_id() -> String {
    let mut b = [0u8; 8];
    crypto::random_bytes(&mut b);
    hex::encode(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    pub fn sample(id: &str) -> Document {
        Document {
            id: id.into(),
            kind: DocKind::Rca,
            title: "RCA B-123-ABC".into(),
            added: "2026-10-27".into(),
            pages: vec![Page {
                number: 1,
                text: "Poliță RCA valabilă până la 14.03.2027".into(),
                lines: vec![],
                width: 1000,
                height: 1400,
                confidence: 91.0,
            }],
            fields: vec![],
            deadlines: vec![],
            extraction: None,
        }
    }

    #[test]
    fn documents_and_images_round_trip_encrypted() {
        let dir = tempfile::tempdir().unwrap();
        let key = Key::random();
        let v = Vault::create(dir.path(), key.clone()).unwrap();
        v.put(&sample("a1")).unwrap();
        v.put_page_image("a1", 1, b"\xff\xd8 jpeg bytes").unwrap();
        let v = Vault::open(dir.path(), key).unwrap();
        assert_eq!(v.get("a1").unwrap(), sample("a1"));
        assert_eq!(v.page_image("a1", 1).unwrap(), b"\xff\xd8 jpeg bytes");
        // Nothing readable on disk.
        for sub in ["docs", "pages"] {
            for e in fs::read_dir(dir.path().join(sub)).unwrap() {
                let data = fs::read(e.unwrap().path()).unwrap();
                let s = String::from_utf8_lossy(&data);
                assert!(!s.contains("RCA") && !s.contains("jpeg") && !s.contains("2027"));
            }
        }
    }

    #[test]
    fn wrong_key_and_moved_records_fail() {
        let dir = tempfile::tempdir().unwrap();
        let key = Key::random();
        let v = Vault::create(dir.path(), key.clone()).unwrap();
        v.put(&sample("a1")).unwrap();
        v.put(&sample("b2")).unwrap();
        assert!(matches!(
            Vault::open(dir.path(), Key::random()),
            Err(Error::BadKey(_))
        ));
        // Swapping two records' files is detected.
        let docs = dir.path().join("docs");
        fs::copy(docs.join("a1.bin"), docs.join("b2.bin")).unwrap();
        assert!(matches!(v.get("b2"), Err(Error::Decrypt(_))));
        assert!(v.all().is_err());
    }

    #[test]
    fn passphrase_vault() {
        let dir = tempfile::tempdir().unwrap();
        let p = KdfParams {
            memory_kib: 1024,
            passes: 1,
            lanes: 1,
        };
        Vault::create_with_passphrase(dir.path(), "parola lungă", p)
            .unwrap()
            .put(&sample("a1"))
            .unwrap();
        assert!(matches!(
            Vault::open_with_passphrase(dir.path(), "alta"),
            Err(Error::BadKey(_))
        ));
        let v = Vault::open_with_passphrase(dir.path(), "parola lungă").unwrap();
        assert_eq!(v.all().unwrap().len(), 1);
        assert!(
            Vault::create_with_passphrase(dir.path(), "x", p).is_err(),
            "a second vault over the first"
        );
    }

    #[test]
    fn delete_and_bad_ids() {
        let dir = tempfile::tempdir().unwrap();
        let v = Vault::create(dir.path(), Key::random()).unwrap();
        v.put(&sample("a1")).unwrap();
        v.put_page_image("a1", 1, b"img").unwrap();
        v.delete("a1").unwrap();
        assert!(v.all().unwrap().is_empty());
        assert!(matches!(v.page_image("a1", 1), Err(Error::NotFound(_))));
        assert!(v.get("../vault").is_err());
        assert!(v.put(&sample("a/b")).is_err());
    }
}
