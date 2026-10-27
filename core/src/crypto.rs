//! Encryption of everything Docket writes to disk.
//!
//! Every record (a document, a page image) is sealed on its own with XChaCha20-Poly1305: a random
//! 192-bit nonce per record (no counter to keep, no risk of reuse), and the record's name as
//! associated data. A record therefore cannot be swapped with another one or renamed without the
//! authentication failing.
//!
//! The 256-bit vault key comes from the platform: on Android a random key that the Android Keystore
//! protects (the app unwraps it and hands it over), on the desktop a passphrase stretched with
//! Argon2id. The key lives in memory only while the vault is open and is wiped when it is dropped.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::Error;

/// Sealed records start with this, then the format version.
const MAGIC: &[u8; 4] = b"DKT1";
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;

/// The vault key. Wiped from memory when dropped.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Key([u8; 32]);

impl Key {
    pub fn from_bytes(bytes: &[u8]) -> Result<Key, Error> {
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| Error::BadKey(format!("{} bytes, want 32", bytes.len())))?;
        Ok(Key(arr))
    }

    /// A new random key (for a new vault).
    pub fn random() -> Key {
        let mut k = [0u8; 32];
        random_bytes(&mut k);
        Key(k)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(<hidden>)")
    }
}

/// Argon2id parameters for a passphrase. OWASP's recommendation is 19 MiB and 2 passes at the
/// least; Docket takes 64 MiB and 3 passes (about half a second on a phone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KdfParams {
    pub memory_kib: u32,
    pub passes: u32,
    pub lanes: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        KdfParams {
            memory_kib: 64 * 1024,
            passes: 3,
            lanes: 1,
        }
    }
}

/// Stretches a passphrase into a key.
pub fn key_from_passphrase(passphrase: &str, salt: &[u8], p: KdfParams) -> Result<Key, Error> {
    let params = Params::new(p.memory_kib, p.passes, p.lanes, Some(32))
        .map_err(|e| Error::BadKey(e.to_string()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = [0u8; 32];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut out)
        .map_err(|e| Error::BadKey(e.to_string()))?;
    let key = Key(out);
    out.zeroize();
    Ok(key)
}

pub fn random_bytes(buf: &mut [u8]) {
    getrandom::fill(buf).expect("the system random number generator failed");
}

/// Encrypts `plaintext`; `name` (the record's name) is authenticated with it.
pub fn seal(key: &Key, name: &str, plaintext: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.0.as_ref().try_into().expect("32-byte key"));
    let mut nonce = [0u8; NONCE_LEN];
    random_bytes(&mut nonce);
    let nonce_arr = XNonce::try_from(&nonce[..]).expect("24-byte nonce");
    let ct = cipher
        .encrypt(
            &nonce_arr,
            Payload {
                msg: plaintext,
                aad: name.as_bytes(),
            },
        )
        .expect("encryption cannot fail for in-memory buffers");
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    out
}

/// Decrypts what `seal` wrote under the same name. Fails on a wrong key, a wrong name or any
/// changed byte.
pub fn open(key: &Key, name: &str, sealed: &[u8]) -> Result<Vec<u8>, Error> {
    if sealed.len() < MAGIC.len() + NONCE_LEN + TAG_LEN || &sealed[..4] != MAGIC {
        return Err(Error::Corrupt(format!("{name}: not a Docket record")));
    }
    let cipher = XChaCha20Poly1305::new(key.0.as_ref().try_into().expect("32-byte key"));
    let nonce = XNonce::try_from(&sealed[4..4 + NONCE_LEN]).expect("24-byte nonce");
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: &sealed[4 + NONCE_LEN..],
                aad: name.as_bytes(),
            },
        )
        .map_err(|_| Error::Decrypt(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let k = Key::random();
        let sealed = seal(&k, "docs/1.json", b"polita RCA");
        assert_eq!(open(&k, "docs/1.json", &sealed).unwrap(), b"polita RCA");
        // Two seals of the same text differ (random nonce).
        assert_ne!(seal(&k, "docs/1.json", b"polita RCA"), sealed);
    }

    #[test]
    fn wrong_key_name_or_byte_fails() {
        let k = Key::random();
        let sealed = seal(&k, "docs/1.json", b"secret");
        assert!(matches!(
            open(&Key::random(), "docs/1.json", &sealed),
            Err(Error::Decrypt(_))
        ));
        assert!(matches!(
            open(&k, "docs/2.json", &sealed),
            Err(Error::Decrypt(_))
        ));
        for i in 0..sealed.len() {
            let mut bad = sealed.clone();
            bad[i] ^= 1;
            assert!(
                open(&k, "docs/1.json", &bad).is_err(),
                "byte {i} changed and still opened"
            );
        }
        assert!(matches!(open(&k, "x", b"short"), Err(Error::Corrupt(_))));
    }

    #[test]
    fn passphrase_is_deterministic_per_salt() {
        let p = KdfParams {
            memory_kib: 1024,
            passes: 1,
            lanes: 1,
        };
        let a = key_from_passphrase("corect cal baterie capsa", b"0123456789abcdef", p).unwrap();
        let b = key_from_passphrase("corect cal baterie capsa", b"0123456789abcdef", p).unwrap();
        let c = key_from_passphrase("corect cal baterie capsa", b"fedcba9876543210", p).unwrap();
        assert_eq!(a.as_bytes(), b.as_bytes());
        assert_ne!(a.as_bytes(), c.as_bytes());
        assert_eq!(format!("{a:?}"), "Key(<hidden>)");
    }

    #[test]
    fn key_length_is_checked() {
        assert!(Key::from_bytes(&[0u8; 31]).is_err());
        assert!(Key::from_bytes(&[0u8; 32]).is_ok());
    }
}
