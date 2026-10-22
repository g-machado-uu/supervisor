//! Password-protected storage: the whole database is serialised to JSON and
//! sealed with XChaCha20-Poly1305 using a key derived by Argon2id.
//!
//! File layout (all integers little-endian):
//! `"SVTR" | version u8 | m_cost u32 | t_cost u32 | p_cost u32 | salt [16] | nonce [24] | ciphertext`
//! The header is authenticated as associated data, so any tampering is detected.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use crate::model::Database;

const MAGIC: &[u8; 4] = b"SVTR";
const FORMAT_VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const HEADER_LEN: usize = 4 + 1 + 12 + SALT_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m_cost_kib: 64 * 1024,
            t_cost: 3,
            p_cost: 1,
        }
    }
}

/// Error shown to the user when decryption fails.
pub const WRONG_PASSWORD: &str = "Incorrect password (or the data file is damaged).";

/// An unlocked data file. Holds the derived key in memory; it is wiped on drop.
pub struct Vault {
    path: PathBuf,
    params: KdfParams,
    salt: [u8; SALT_LEN],
    key: Zeroizing<[u8; 32]>,
}

pub fn default_path() -> PathBuf {
    directories::ProjectDirs::from("", "", "Supervisor")
        .map(|d| d.data_dir().join("data.svt"))
        .unwrap_or_else(|| PathBuf::from("supervisor-data.svt"))
}

fn derive_key(password: &str, salt: &[u8], params: KdfParams) -> Result<Zeroizing<[u8; 32]>> {
    let p = Params::new(params.m_cost_kib, params.t_cost, params.p_cost, Some(32))
        .map_err(|e| anyhow!("invalid key-derivation parameters: {e}"))?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, p)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| anyhow!("key derivation failed: {e}"))?;
    Ok(key)
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

impl Vault {
    /// Create a new data file at `path`, protected by `password`.
    pub fn create(path: &Path, password: &str, db: &Database) -> Result<Self> {
        Self::create_with_params(path, password, db, KdfParams::default())
    }

    pub fn create_with_params(
        path: &Path,
        password: &str,
        db: &Database,
        params: KdfParams,
    ) -> Result<Self> {
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let vault = Self {
            path: path.to_path_buf(),
            params,
            salt,
            key: derive_key(password, &salt, params)?,
        };
        vault.save(db)?;
        Ok(vault)
    }

    /// Decrypt an existing data file.
    pub fn open(path: &Path, password: &str) -> Result<(Self, Database)> {
        let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        if bytes.len() < HEADER_LEN + NONCE_LEN || &bytes[..4] != MAGIC {
            bail!("{} is not a Supervisor data file", path.display());
        }
        if bytes[4] != FORMAT_VERSION {
            bail!("unsupported data file version {}", bytes[4]);
        }
        let params = KdfParams {
            m_cost_kib: read_u32(&bytes, 5),
            t_cost: read_u32(&bytes, 9),
            p_cost: read_u32(&bytes, 13),
        };
        let salt: [u8; SALT_LEN] = bytes[17..HEADER_LEN].try_into().expect("salt");
        let nonce = XNonce::from_slice(&bytes[HEADER_LEN..HEADER_LEN + NONCE_LEN]);
        let key = derive_key(password, &salt, params)?;
        let cipher = XChaCha20Poly1305::new(key.as_ref().into());
        let plaintext = Zeroizing::new(
            cipher
                .decrypt(
                    nonce,
                    Payload {
                        msg: &bytes[HEADER_LEN + NONCE_LEN..],
                        aad: &bytes[..HEADER_LEN],
                    },
                )
                .map_err(|_| anyhow!(WRONG_PASSWORD))?,
        );
        let db: Database =
            serde_json::from_slice(&plaintext).context("decrypted data could not be read")?;
        Ok((
            Self {
                path: path.to_path_buf(),
                params,
                salt,
                key,
            },
            db,
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn header(&self) -> Vec<u8> {
        let mut h = Vec::with_capacity(HEADER_LEN);
        h.extend_from_slice(MAGIC);
        h.push(FORMAT_VERSION);
        h.extend_from_slice(&self.params.m_cost_kib.to_le_bytes());
        h.extend_from_slice(&self.params.t_cost.to_le_bytes());
        h.extend_from_slice(&self.params.p_cost.to_le_bytes());
        h.extend_from_slice(&self.salt);
        h
    }

    /// Encrypt and write atomically. The previous file is kept as `<name>.bak`.
    pub fn save(&self, db: &Database) -> Result<()> {
        let plaintext = Zeroizing::new(serde_json::to_vec(db)?);
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let header = self.header();
        let cipher = XChaCha20Poly1305::new(self.key.as_ref().into());
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &header,
                },
            )
            .map_err(|_| anyhow!("encryption failed"))?;

        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let tmp = sibling(&self.path, "tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&header)?;
            f.write_all(&nonce)?;
            f.write_all(&ciphertext)?;
            f.sync_all()?;
        }
        if self.path.exists() {
            fs::copy(&self.path, sibling(&self.path, "bak")).context("writing backup")?;
        }
        fs::rename(&tmp, &self.path).context("replacing data file")?;
        Ok(())
    }

    /// Re-key the file with a new password (fresh salt) and save immediately.
    pub fn change_password(&mut self, new_password: &str, db: &Database) -> Result<()> {
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        self.key = derive_key(new_password, &salt, self.params)?;
        self.salt = salt;
        self.save(db)?;
        // The backup is still sealed with the old password; don't leave it behind.
        let _ = fs::remove_file(sibling(&self.path, "bak"));
        Ok(())
    }
}

/// Non-secret preference file remembering which data file was last opened.
fn config_file() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "Supervisor").map(|d| d.config_dir().join("last_file.txt"))
}

pub fn last_used_path() -> PathBuf {
    config_file()
        .and_then(|f| fs::read_to_string(f).ok())
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(default_path)
}

pub fn remember_path(path: &Path) {
    if let Some(f) = config_file() {
        if let Some(dir) = f.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(f, path.to_string_lossy().as_bytes());
    }
}

fn sibling(path: &Path, ext: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(ext);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Student;

    const FAST: KdfParams = KdfParams {
        m_cost_kib: 1024,
        t_cost: 1,
        p_cost: 1,
    };

    fn sample() -> Database {
        let mut db = Database::default();
        db.students.push(Student {
            name: "Grace".into(),
            b_number: "B00123456".into(),
            ..Default::default()
        });
        db
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let db = sample();
        Vault::create_with_params(&path, "correct horse", &db, FAST).unwrap();
        let (_, back) = Vault::open(&path, "correct horse").unwrap();
        assert_eq!(db, back);
        // Plaintext must not appear on disk.
        let raw = fs::read(&path).unwrap();
        assert!(!raw.windows(5).any(|w| w == b"Grace"));
    }

    #[test]
    fn wrong_password_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        Vault::create_with_params(&path, "right", &sample(), FAST).unwrap();
        let err = Vault::open(&path, "wrong").err().unwrap();
        assert_eq!(err.to_string(), WRONG_PASSWORD);
    }

    #[test]
    fn tampering_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        Vault::create_with_params(&path, "pw", &sample(), FAST).unwrap();
        let mut raw = fs::read(&path).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        fs::write(&path, raw).unwrap();
        assert!(Vault::open(&path, "pw").is_err());
    }

    #[test]
    fn save_keeps_backup_and_change_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut db = sample();
        let mut vault = Vault::create_with_params(&path, "old", &db, FAST).unwrap();
        db.students[0].name = "Grace Hopper".into();
        vault.save(&db).unwrap();
        assert!(sibling(&path, "bak").exists());
        assert!(!sibling(&path, "tmp").exists());

        vault.change_password("new", &db).unwrap();
        assert!(Vault::open(&path, "old").is_err());
        let (_, back) = Vault::open(&path, "new").unwrap();
        assert_eq!(back.students[0].name, "Grace Hopper");
    }
}
