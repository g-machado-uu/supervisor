//! Password-protected storage: the whole database is serialised to JSON and
//! sealed with XChaCha20-Poly1305 using a key derived by Argon2id.
//!
//! File layout (all integers little-endian):
//! `"SVTR" | version u8 | m_cost u32 | t_cost u32 | p_cost u32 | salt [16] | v2 only: flags u8 | key file id [32] | nonce [24] | ciphertext`
//! Version 2 is written only when a key file is in use (flags bit 0): 32
//! random bytes kept on each computer and passed to Argon2id as its secret, so
//! the key depends on both the password and the key file. Password-only files
//! stay version 1, which older versions of the app can still open.
//! The header is authenticated as associated data, so any tampering is detected.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use argon2::{Algorithm, Argon2, Params, Version};
use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use crate::model::Database;

const MAGIC: &[u8; 4] = b"SVTR";
const V1: u8 = 1;
const V2: u8 = 2;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const ID_LEN: usize = 32;
const V1_HEADER_LEN: usize = 4 + 1 + 12 + SALT_LEN;
const V2_HEADER_LEN: usize = V1_HEADER_LEN + 1 + ID_LEN;
const FLAG_KEY_FILE: u8 = 1;
const KEY_FILE_LEN: usize = 32;
const KEY_ID_LABEL: &[u8] = b"Supervisor key file id v1";

/// Shortest password accepted for a new file or a password change.
pub const MIN_PASSWORD_LEN: usize = 12;

type Blake2b256 = Blake2b<U32>;

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

impl KdfParams {
    /// Settings read from a file must be sane before Argon2 runs, or a crafted
    /// file could make unlocking allocate terabytes or run for days.
    fn check(self) -> Result<()> {
        if (8 * 1024..=1024 * 1024).contains(&self.m_cost_kib)
            && (1..=10).contains(&self.t_cost)
            && (1..=4).contains(&self.p_cost)
        {
            Ok(())
        } else {
            bail!("the data file asks for unusual key-derivation settings, so it was not opened (it may be damaged)")
        }
    }

    /// Re-keying never keeps settings weaker than the defaults.
    fn at_least_default(self) -> Self {
        let d = Self::default();
        Self {
            m_cost_kib: self.m_cost_kib.max(d.m_cost_kib),
            t_cost: self.t_cost.max(d.t_cost),
            p_cost: self.p_cost.max(d.p_cost),
        }
    }
}

/// Failures the UI handles specially; anything else is shown as text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultError {
    WrongPassword,
    KeyFileMissing,
    WrongKeyFile,
    InUse,
    ChangedOnDisk,
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::WrongPassword => "Incorrect password (or the data file is damaged).",
            Self::KeyFileMissing => "This data file also needs its key file, which isn't on this computer.",
            Self::WrongKeyFile => "This key file belongs to a different data file.",
            Self::InUse => "This data file is already open in another Supervisor window.",
            Self::ChangedOnDisk => "The data file was changed somewhere else since it was unlocked.",
        })
    }
}

impl std::error::Error for VaultError {}

/// The [`VaultError`] behind `err`, if it is one.
pub fn vault_error(err: &anyhow::Error) -> Option<VaultError> {
    err.downcast_ref().copied()
}

/// The optional second factor: 32 random bytes kept on each computer, never
/// next to the data file.
#[derive(Clone)]
pub struct KeyFile(Zeroizing<[u8; KEY_FILE_LEN]>);

impl KeyFile {
    pub fn generate() -> Self {
        let mut bytes = Zeroizing::new([0u8; KEY_FILE_LEN]);
        OsRng.fill_bytes(&mut bytes[..]);
        Self(bytes)
    }

    pub fn read(path: &Path) -> Result<Self> {
        let bytes = Zeroizing::new(fs::read(path).with_context(|| format!("reading {}", path.display()))?);
        if bytes.len() != KEY_FILE_LEN {
            bail!("{} is not a Supervisor key file", path.display());
        }
        let mut key = Zeroizing::new([0u8; KEY_FILE_LEN]);
        key.copy_from_slice(&bytes);
        Ok(Self(key))
    }

    fn write(&self, path: &Path) -> Result<()> {
        write_atomic(path, &self.0[..]).with_context(|| format!("writing {}", path.display()))
    }

    /// Stored in the header so a wrong key file is reported before Argon2
    /// runs. It reveals nothing, since the key file is random.
    pub fn id(&self) -> [u8; ID_LEN] {
        let mut h = Blake2b256::new();
        h.update(KEY_ID_LABEL);
        h.update(&self.0[..]);
        h.finalize().into()
    }
}

/// An unlocked data file. Holds the derived key in memory; it is wiped on drop.
pub struct Vault {
    path: PathBuf,
    seal: Seal,
    /// Hash of the file as last read or written, to notice other writers.
    on_disk: Option<[u8; 32]>,
    /// Held while unlocked so a second window can't open the same file.
    _lock: Option<fs::File>,
}

/// Everything re-keying replaces. A new one is saved with before it's swapped
/// in, so a failed save leaves the old password and key file working.
struct Seal {
    params: KdfParams,
    salt: [u8; SALT_LEN],
    key: Zeroizing<[u8; 32]>,
    key_file: Option<KeyFile>,
}

impl Seal {
    fn new(password: &str, key_file: Option<KeyFile>, params: KdfParams) -> Result<Self> {
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let key = derive_key(password, &salt, params, key_file.as_ref())?;
        Ok(Self {
            params,
            salt,
            key,
            key_file,
        })
    }

    fn header(&self) -> Header {
        Header {
            params: self.params,
            salt: self.salt,
            key_id: self.key_file.as_ref().map(KeyFile::id),
        }
    }
}

struct Header {
    params: KdfParams,
    salt: [u8; SALT_LEN],
    key_id: Option<[u8; ID_LEN]>,
}

impl Header {
    /// Parses and sanity-checks the header; also returns its length.
    fn parse(bytes: &[u8], path: &Path) -> Result<(Self, usize)> {
        let not_ours = || anyhow!("{} is not a Supervisor data file", path.display());
        if bytes.len() < V1_HEADER_LEN + NONCE_LEN || &bytes[..4] != MAGIC {
            return Err(not_ours());
        }
        let len = match bytes[4] {
            V1 => V1_HEADER_LEN,
            V2 => V2_HEADER_LEN,
            v => bail!("unsupported data file version {v} – it may need a newer Supervisor"),
        };
        if bytes.len() < len + NONCE_LEN {
            return Err(not_ours());
        }
        let params = KdfParams {
            m_cost_kib: read_u32(bytes, 5),
            t_cost: read_u32(bytes, 9),
            p_cost: read_u32(bytes, 13),
        };
        params.check()?;
        let salt = bytes[17..V1_HEADER_LEN].try_into().expect("salt");
        let mut key_id = None;
        if len == V2_HEADER_LEN {
            let flags = bytes[V1_HEADER_LEN];
            if flags & !FLAG_KEY_FILE != 0 {
                bail!("this data file needs a newer Supervisor");
            }
            if flags & FLAG_KEY_FILE != 0 {
                key_id = Some(bytes[V1_HEADER_LEN + 1..V2_HEADER_LEN].try_into().expect("id"));
            }
        }
        Ok((Self { params, salt, key_id }, len))
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut h = Vec::with_capacity(V2_HEADER_LEN);
        h.extend_from_slice(MAGIC);
        h.push(if self.key_id.is_some() { V2 } else { V1 });
        h.extend_from_slice(&self.params.m_cost_kib.to_le_bytes());
        h.extend_from_slice(&self.params.t_cost.to_le_bytes());
        h.extend_from_slice(&self.params.p_cost.to_le_bytes());
        h.extend_from_slice(&self.salt);
        if let Some(id) = &self.key_id {
            h.push(FLAG_KEY_FILE);
            h.extend_from_slice(id);
        }
        h
    }
}

fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", "Supervisor")
}

pub fn default_path() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().join("data.svt"))
        .unwrap_or_else(|| PathBuf::from("supervisor-data.svt"))
}

/// Where this computer keeps its key file: `~/.config/supervisor` on Linux,
/// `%APPDATA%\Supervisor\config` on Windows and `~/Library/Preferences/Supervisor`
/// on macOS (not Application Support, where the data file lives by default).
pub fn key_file_path() -> Option<PathBuf> {
    project_dirs().map(|d| d.preference_dir().join("key.bin"))
}

fn derive_key(
    password: &str,
    salt: &[u8],
    params: KdfParams,
    key_file: Option<&KeyFile>,
) -> Result<Zeroizing<[u8; 32]>> {
    params.check()?;
    let p = Params::new(params.m_cost_kib, params.t_cost, params.p_cost, Some(32))
        .map_err(|e| anyhow!("invalid key-derivation parameters: {e}"))?;
    let argon2 = match key_file {
        Some(k) => Argon2::new_with_secret(&k.0[..], Algorithm::Argon2id, Version::V0x13, p)
            .map_err(|e| anyhow!("key derivation failed: {e}"))?,
        None => Argon2::new(Algorithm::Argon2id, Version::V0x13, p),
    };
    let mut key = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| anyhow!("key derivation failed: {e}"))?;
    Ok(key)
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

fn decrypt(bytes: &[u8], path: &Path, password: &str, key_file: Option<&KeyFile>) -> Result<(Seal, Database)> {
    let (header, len) = Header::parse(bytes, path)?;
    let key_file = match header.key_id {
        None => None,
        Some(id) => {
            let k = key_file.ok_or(VaultError::KeyFileMissing)?;
            if k.id() != id {
                return Err(VaultError::WrongKeyFile.into());
            }
            Some(k.clone())
        }
    };
    let key = derive_key(password, &header.salt, header.params, key_file.as_ref())?;
    let cipher = XChaCha20Poly1305::new(key.as_ref().into());
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&bytes[len..len + NONCE_LEN]),
                Payload {
                    msg: &bytes[len + NONCE_LEN..],
                    aad: &bytes[..len],
                },
            )
            .map_err(|_| VaultError::WrongPassword)?,
    );
    let db = serde_json::from_slice(&plaintext).context("decrypted data could not be read")?;
    let seal = Seal {
        params: header.params,
        salt: header.salt,
        key,
        key_file,
    };
    Ok((seal, db))
}

impl Vault {
    /// Create a new data file at `path`, protected by `password`.
    pub fn create(path: &Path, password: &str, db: &Database) -> Result<Self> {
        Self::create_with_params(path, password, db, KdfParams::default())
    }

    fn create_with_params(path: &Path, password: &str, db: &Database, params: KdfParams) -> Result<Self> {
        let mut vault = Self {
            _lock: lock_file(path)?,
            path: path.to_path_buf(),
            seal: Seal::new(password, None, params)?,
            on_disk: None,
        };
        // Refuses if a file appeared there in the meantime.
        vault.save(db)?;
        Ok(vault)
    }

    /// Decrypt an existing data file. `key_file` is used only if the file
    /// asks for one.
    pub fn open(path: &Path, password: &str, key_file: Option<&KeyFile>) -> Result<(Self, Database)> {
        let lock = lock_file(path)?;
        let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let (seal, db) = decrypt(&bytes, path, password, key_file)?;
        let vault = Self {
            path: path.to_path_buf(),
            seal,
            on_disk: Some(hash(&bytes)),
            _lock: lock,
        };
        Ok((vault, db))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn uses_key_file(&self) -> bool {
        self.seal.key_file.is_some()
    }

    /// Encrypt and write atomically, keeping the previous file as `<name>.bak`.
    /// Fails with [`VaultError::ChangedOnDisk`] rather than overwrite a file
    /// that another window or computer has written since.
    pub fn save(&mut self, db: &Database) -> Result<()> {
        self.on_disk = Some(self.write(&self.seal, db, true)?);
        Ok(())
    }

    /// Save even though the file changed elsewhere; that version becomes the `.bak`.
    pub fn overwrite(&mut self, db: &Database) -> Result<()> {
        self.on_disk = Some(self.write(&self.seal, db, false)?);
        Ok(())
    }

    fn write(&self, seal: &Seal, db: &Database, check: bool) -> Result<[u8; 32]> {
        let plaintext = Zeroizing::new(serde_json::to_vec(db)?);
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let header = seal.header().to_bytes();
        let cipher = XChaCha20Poly1305::new(seal.key.as_ref().into());
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &header,
                },
            )
            .map_err(|_| anyhow!("encryption failed"))?;
        let mut bytes = Vec::with_capacity(header.len() + NONCE_LEN + ciphertext.len());
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&nonce);
        bytes.extend_from_slice(&ciphertext);

        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            create_private_dir(dir)?;
        }
        match fs::read(&self.path) {
            Ok(old) => {
                if check && Some(hash(&old)) != self.on_disk {
                    return Err(VaultError::ChangedOnDisk.into());
                }
                write_atomic(&sibling(&self.path, "bak"), &old).context("writing backup")?;
            }
            // Deleted or moved elsewhere: nothing to lose by writing it again.
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.path.display())),
        }
        write_atomic(&self.path, &bytes).context("replacing data file")?;
        Ok(hash(&bytes))
    }

    /// Whether `password` is the one the file is sealed with (no disk access).
    pub fn check_password(&self, password: &str) -> Result<bool> {
        let key = derive_key(password, &self.seal.salt, self.seal.params, self.seal.key_file.as_ref())?;
        Ok(key.iter().zip(self.seal.key.iter()).fold(0, |acc, (a, b)| acc | (a ^ b)) == 0)
    }

    /// Re-key the file with a new password (fresh salt) and save immediately.
    pub fn change_password(&mut self, current: &str, new: &str, db: &Database) -> Result<()> {
        let key_file = self.seal.key_file.clone();
        self.reseal(current, new, key_file, db)
    }

    /// Re-key the file to need `key_file` as well as the password, or (with
    /// `None`) the password alone.
    pub fn set_key_file(&mut self, password: &str, key_file: Option<KeyFile>, db: &Database) -> Result<()> {
        self.reseal(password, password, key_file, db)
    }

    fn reseal(&mut self, current: &str, password: &str, key_file: Option<KeyFile>, db: &Database) -> Result<()> {
        // A typo here would otherwise become the new password.
        if !self.check_password(current)? {
            return Err(VaultError::WrongPassword.into());
        }
        let next = Seal::new(password, key_file, self.seal.params.at_least_default())?;
        self.on_disk = Some(self.write(&next, db, true)?);
        self.seal = next;
        // The backup is still sealed the old way; don't leave it behind.
        let _ = fs::remove_file(sibling(&self.path, "bak"));
        Ok(())
    }
}

/// The key file a data file asks for, if any (reads only the header).
pub fn required_key_id(path: &Path) -> Result<Option<[u8; ID_LEN]>> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(Header::parse(&bytes, path)?.0.key_id)
}

/// This computer's key file, if it has one.
pub fn local_key_file() -> Result<Option<KeyFile>> {
    match key_file_path() {
        Some(path) if path.exists() => KeyFile::read(&path).map(Some),
        _ => Ok(None),
    }
}

/// Make `key` this computer's key file.
pub fn install_key_file(key: &KeyFile) -> Result<()> {
    install_key_file_at(key, &key_file_path().context("no folder for settings on this computer")?)
}

/// A different key file already at `dest` is kept beside it as
/// `key-<id>.bin`, in case another data file needs it.
fn install_key_file_at(key: &KeyFile, dest: &Path) -> Result<()> {
    if dest.exists() {
        let existing = KeyFile::read(dest)
            .with_context(|| format!("{} is in the way; move it somewhere else first", dest.display()))?;
        if existing.id() == key.id() {
            return Ok(());
        }
        let aside = dest.with_file_name(format!("key-{}.bin", &hex(&existing.id())[..8]));
        fs::rename(dest, &aside).context("moving the previous key file aside")?;
    }
    if let Some(dir) = dest.parent() {
        create_private_dir(dir)?;
    }
    key.write(dest)
}

/// Copy a key file (e.g. from a backup) into place, if it's the one `expected` names.
pub fn import_key_file(src: &Path, expected: [u8; ID_LEN]) -> Result<KeyFile> {
    let key = KeyFile::read(src)?;
    if key.id() != expected {
        return Err(VaultError::WrongKeyFile.into());
    }
    install_key_file(&key)?;
    Ok(key)
}

/// Save a backup copy of `key` at `dest`, which must not be in the data
/// file's folder (a backup there would travel with the data file) or the
/// folder the key file already lives in.
pub fn save_key_backup(key: &KeyFile, dest: &Path, data_file: &Path) -> Result<()> {
    if same_folder(dest, data_file) {
        bail!("Choose another place: a backup next to the data file would let anyone with the data file open it.");
    }
    if key_file_path().is_some_and(|k| same_folder(dest, &k)) {
        bail!("Choose another place, such as a USB stick: this is where the key file already is.");
    }
    key.write(dest)
}

/// Whether two files are (or would be) in the same folder.
pub fn same_folder(a: &Path, b: &Path) -> bool {
    let folder = |p: &Path| canonical(p.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new(".")));
    folder(a) == folder(b)
}

/// Why `password` is too weak for a new file or a password change, if it is.
pub fn password_problem(password: &str) -> Option<String> {
    const COMMON: &[&str] = &[
        "password", "passw0rd", "qwerty", "letmein", "welcome", "iloveyou", "admin", "monkey",
        "dragon", "football", "sunshine", "princess", "baseball", "superman", "trustno1",
        "supervisor", "university", "ulster", "student", "changeme", "secret", "abc",
    ];
    const RUNS: &[&str] = &[
        "abcdefghijklmnopqrstuvwxyz",
        "01234567890123456789",
        "qwertyuiopasdfghjklzxcvbnm",
    ];
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Some(format!(
            "Use at least {MIN_PASSWORD_LEN} characters – a short sentence you'll remember works well."
        ));
    }
    let lower = password.to_lowercase();
    let core = lower.trim_matches(|c: char| !c.is_alphabetic());
    let mut distinct: Vec<char> = lower.chars().collect();
    distinct.sort_unstable();
    distinct.dedup();
    if distinct.len() < 5
        || COMMON.contains(&core)
        || RUNS.iter().any(|run| run.contains(lower.as_str()) || (!core.is_empty() && run.contains(core)))
    {
        return Some("That password is too easy to guess – try a short sentence instead.".into());
    }
    None
}

/// A fingerprint of the database for spotting edits. The JSON streams straight
/// into the hash, so no plaintext copy is left behind.
pub fn fingerprint(db: &Database) -> [u8; 32] {
    struct Hasher(Blake2b256);
    impl Write for Hasher {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.update(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut h = Hasher(Blake2b256::new());
    serde_json::to_writer(&mut h, db).expect("the database always serialises");
    h.0.finalize().into()
}

fn hash(bytes: &[u8]) -> [u8; 32] {
    Blake2b256::digest(bytes).into()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Write `bytes` to `path` through a new owner-only temp file with a random
/// name, then rename it into place: a crash never leaves a half-written file,
/// and a link planted at a fixed temp name can't redirect the write.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut suffix = [0u8; 8];
    OsRng.fill_bytes(&mut suffix);
    let tmp = sibling(path, &format!("{}.tmp", hex(&suffix)));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options.open(&tmp).and_then(|mut f| {
        f.write_all(bytes)?;
        f.sync_all()
    });
    if let Err(e) = written.and_then(|()| fs::rename(&tmp, path)) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    // Make the rename itself survive a crash.
    #[cfg(unix)]
    {
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let _ = fs::File::open(dir).and_then(|d| d.sync_all());
    }
    Ok(())
}

/// Create `dir` (and parents) readable only by you. Existing folders are left alone.
fn create_private_dir(dir: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir).with_context(|| format!("creating {}", dir.display()))
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path)
        .ok()
        .or_else(|| {
            let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
            Some(fs::canonicalize(dir).ok()?.join(path.file_name()?))
        })
        .unwrap_or_else(|| path.to_path_buf())
}

/// Take this computer's lock on `path` for as long as the file is unlocked.
/// Lock files live in the app's own folder rather than next to the data file,
/// so nothing extra lands in a synced folder; another computer writing the
/// same file is caught by the changed-on-disk check instead.
fn lock_file(path: &Path) -> Result<Option<fs::File>> {
    // Tests open throwaway files; keep their locks out of the real app folder.
    let dir = if cfg!(test) {
        Some(std::env::temp_dir().join("supervisor-test-locks"))
    } else {
        project_dirs().map(|d| d.data_local_dir().join("locks"))
    };
    let Some(dir) = dir else { return Ok(None) };
    if create_private_dir(&dir).is_err() {
        return Ok(None);
    }
    let name = hex(&hash(canonical(path).as_os_str().as_encoded_bytes())[..16]);
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let Ok(file) = options.open(dir.join(format!("{name}.lock"))) else { return Ok(None) };
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Err(VaultError::InUse.into()),
        // Filesystems without locking: carry on without it.
        Err(fs::TryLockError::Error(_)) => Ok(None),
    }
}

/// Non-secret preference file remembering which data file was last opened.
fn config_file() -> Option<PathBuf> {
    project_dirs().map(|d| d.config_dir().join("last_file.txt"))
}

pub fn last_used_path() -> PathBuf {
    config_file()
        .and_then(|f| fs::read_to_string(f).ok())
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(default_path)
}

pub fn remember_path(path: &Path) {
    // Tests open throwaway files; don't point the real app at them.
    if cfg!(test) {
        return;
    }
    if let Some(f) = config_file()
        && f.parent().is_none_or(|dir| create_private_dir(dir).is_ok())
    {
        let _ = write_atomic(&f, path.to_string_lossy().as_bytes());
    }
}

fn sibling(path: &Path, ext: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(ext);
    path.with_file_name(name)
}

/// Decrypt without taking the lock, to check what was saved while a vault is open.
#[cfg(test)]
pub fn read_database(path: &Path, password: &str, key_file: Option<&KeyFile>) -> Result<Database> {
    let bytes = fs::read(path)?;
    Ok(decrypt(&bytes, path, password, key_file)?.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Student;

    /// The weakest settings a file may have; keeps the tests quick.
    const FAST: KdfParams = KdfParams {
        m_cost_kib: 8 * 1024,
        t_cost: 1,
        p_cost: 1,
    };

    fn sample() -> Database {
        let mut db = Database::default();
        db.settings.my_email = "someone@ulster.ac.uk".into();
        db.students.push(Student {
            name: "Grace".into(),
            b_number: "B00123456".into(),
            ..Default::default()
        });
        db
    }

    fn error_of<T>(result: Result<T>) -> Option<VaultError> {
        vault_error(&result.err().expect("should fail"))
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let db = sample();
        Vault::create_with_params(&path, "correct horse", &db, FAST).unwrap();
        let (_, back) = Vault::open(&path, "correct horse", None).unwrap();
        assert_eq!(db, back);
        // Plaintext must not appear on disk.
        let raw = fs::read(&path).unwrap();
        assert!(!raw.windows(5).any(|w| w == b"Grace"));
        assert!(
            !raw.windows(7).any(|w| w == b"someone"),
            "own email is encrypted too"
        );
        // Password-only files stay readable by older versions.
        assert_eq!(raw[4], V1);
    }

    #[test]
    fn wrong_password_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        Vault::create_with_params(&path, "right", &sample(), FAST).unwrap();
        let err = Vault::open(&path, "wrong", None).err().unwrap();
        assert_eq!(vault_error(&err), Some(VaultError::WrongPassword));
        assert_eq!(err.to_string(), "Incorrect password (or the data file is damaged).");
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
        assert!(Vault::open(&path, "pw", None).is_err());
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
        let names: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 2, "no temp files left behind: {names:?}");

        assert_eq!(error_of(vault.change_password("typo", "new", &db)), Some(VaultError::WrongPassword));
        vault.change_password("old", "new", &db).unwrap();
        assert!(read_database(&path, "old", None).is_err());
        let back = read_database(&path, "new", None).unwrap();
        assert_eq!(back.students[0].name, "Grace Hopper");
        assert!(!sibling(&path, "bak").exists(), "backup under the old password removed");
        drop(vault);
        // Re-keying never keeps weaker-than-default settings.
        let raw = fs::read(&path).unwrap();
        assert_eq!(read_u32(&raw, 5), KdfParams::default().m_cost_kib);
    }

    /// A non-empty folder where the backup goes makes every save fail.
    fn block_saves(path: &Path) -> PathBuf {
        let bak = sibling(path, "bak");
        let _ = fs::remove_file(&bak);
        fs::create_dir_all(bak.join("blocker")).unwrap();
        bak
    }

    #[test]
    fn failed_password_change_keeps_old_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut db = sample();
        let mut vault = Vault::create_with_params(&path, "old", &db, FAST).unwrap();
        let blocker = block_saves(&path);
        assert!(vault.change_password("old", "new", &db).is_err());
        // The next autosave, once the disk works again, must still use the old password.
        fs::remove_dir_all(blocker).unwrap();
        db.students[0].name = "Changed".into();
        vault.save(&db).unwrap();
        assert!(vault.check_password("old").unwrap());
        assert_eq!(read_database(&path, "old", None).unwrap().students[0].name, "Changed");
        assert!(read_database(&path, "new", None).is_err());
    }

    #[test]
    fn crafted_kdf_settings_rejected_before_argon2() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        Vault::create_with_params(&path, "pw", &sample(), FAST).unwrap();
        let good = fs::read(&path).unwrap();
        for (at, value) in [(5, u32::MAX), (5, 16), (9, u32::MAX), (9, 0), (13, 255)] {
            let mut raw = good.clone();
            raw[at..at + 4].copy_from_slice(&u32::to_le_bytes(value));
            fs::write(&path, raw).unwrap();
            let start = std::time::Instant::now();
            let err = Vault::open(&path, "pw", None).err().unwrap();
            assert!(err.to_string().contains("unusual"), "{err}");
            assert!(start.elapsed().as_secs() < 2);
        }
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private_and_links_not_followed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("new-folder");
        let path = folder.join("data.svt");
        let mut vault = Vault::create_with_params(&path, "pw", &sample(), FAST).unwrap();
        // A link planted at the old fixed temp name must not be written through.
        let victim = dir.path().join("victim.txt");
        fs::write(&victim, "keep me").unwrap();
        std::os::unix::fs::symlink(&victim, sibling(&path, "tmp")).unwrap();
        vault.save(&sample()).unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "keep me");
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&folder), 0o700);
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&sibling(&path, "bak")), 0o600);
    }

    #[test]
    fn second_window_refused_and_outside_changes_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let mut db = sample();
        let mut vault = Vault::create_with_params(&path, "pw", &db, FAST).unwrap();
        assert_eq!(error_of(Vault::open(&path, "pw", None)), Some(VaultError::InUse));

        // Another computer (via a synced folder) writes the file.
        let mut theirs = sample();
        theirs.students[0].name = "Theirs".into();
        let mut other = Vault {
            path: path.clone(),
            seal: Seal::new("pw", None, FAST).unwrap(),
            on_disk: Some(hash(&fs::read(&path).unwrap())),
            _lock: None,
        };
        other.save(&theirs).unwrap();

        db.students[0].name = "Mine".into();
        assert_eq!(error_of(vault.save(&db)), Some(VaultError::ChangedOnDisk));
        assert_eq!(read_database(&path, "pw", None).unwrap().students[0].name, "Theirs");
        vault.overwrite(&db).unwrap();
        assert_eq!(read_database(&path, "pw", None).unwrap().students[0].name, "Mine");
        let backup = sibling(&path, "bak");
        assert_eq!(read_database(&backup, "pw", None).unwrap().students[0].name, "Theirs");
        // Back in step: ordinary saves work again.
        vault.save(&db).unwrap();
        drop(vault);
        assert!(Vault::open(&path, "pw", None).is_ok(), "lock released");
    }

    #[test]
    fn key_file_needed_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let db = sample();
        let key = KeyFile::generate();
        let mut vault = Vault::create_with_params(&path, "pw", &db, FAST).unwrap();
        assert_eq!(error_of(vault.set_key_file("typo", Some(key.clone()), &db)), Some(VaultError::WrongPassword));
        vault.set_key_file("pw", Some(key.clone()), &db).unwrap();
        assert!(vault.uses_key_file());
        assert!(vault.check_password("pw").unwrap());
        assert!(!sibling(&path, "bak").exists(), "password-only backup removed");
        drop(vault);

        assert_eq!(fs::read(&path).unwrap()[4], V2);
        assert_eq!(required_key_id(&path).unwrap(), Some(key.id()));
        assert_eq!(error_of(Vault::open(&path, "pw", None)), Some(VaultError::KeyFileMissing));
        let other = KeyFile::generate();
        assert_eq!(error_of(Vault::open(&path, "pw", Some(&other))), Some(VaultError::WrongKeyFile));
        assert_eq!(error_of(Vault::open(&path, "wrong", Some(&key))), Some(VaultError::WrongPassword));

        let (mut vault, back) = Vault::open(&path, "pw", Some(&key)).unwrap();
        assert_eq!(back, db);
        // A password change keeps the key file.
        vault.change_password("pw", "pw2", &db).unwrap();
        assert!(read_database(&path, "pw2", None).is_err());
        assert!(read_database(&path, "pw2", Some(&key)).is_ok());

        vault.set_key_file("pw2", None, &db).unwrap();
        drop(vault);
        assert_eq!(fs::read(&path).unwrap()[4], V1);
        assert_eq!(required_key_id(&path).unwrap(), None);
        assert!(Vault::open(&path, "pw2", None).is_ok());
    }

    #[test]
    fn failed_key_file_change_keeps_old_setup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.svt");
        let db = sample();
        let mut vault = Vault::create_with_params(&path, "pw", &db, FAST).unwrap();
        let blocker = block_saves(&path);
        assert!(vault.set_key_file("pw", Some(KeyFile::generate()), &db).is_err());
        fs::remove_dir_all(blocker).unwrap();
        vault.save(&db).unwrap();
        assert!(!vault.uses_key_file());
        assert!(read_database(&path, "pw", None).is_ok());
    }

    #[test]
    fn key_file_install_import_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let dest = config.join("key.bin");
        let key = KeyFile::generate();
        install_key_file_at(&key, &dest).unwrap();
        assert_eq!(KeyFile::read(&dest).unwrap().id(), key.id());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&dest).unwrap().permissions().mode() & 0o777, 0o600);
        }

        // A different key already installed is kept aside, not overwritten.
        let newer = KeyFile::generate();
        install_key_file_at(&newer, &dest).unwrap();
        assert_eq!(KeyFile::read(&dest).unwrap().id(), newer.id());
        let aside = config.join(format!("key-{}.bin", &hex(&key.id())[..8]));
        assert_eq!(KeyFile::read(&aside).unwrap().id(), key.id());

        fs::write(dir.path().join("short.bin"), [1u8; 5]).unwrap();
        assert!(KeyFile::read(&dir.path().join("short.bin")).is_err());

        let data = dir.path().join("sync").join("data.svt");
        fs::create_dir_all(data.parent().unwrap()).unwrap();
        assert!(save_key_backup(&key, &data.with_file_name("key-backup.bin"), &data).is_err());
        let usb = dir.path().join("usb");
        fs::create_dir_all(&usb).unwrap();
        save_key_backup(&key, &usb.join("key-backup.bin"), &data).unwrap();
        assert_eq!(KeyFile::read(&usb.join("key-backup.bin")).unwrap().id(), key.id());
    }

    #[test]
    fn password_rules() {
        assert!(password_problem("short").is_some());
        assert!(password_problem("12345678901").is_some(), "11 characters");
        for weak in ["password1234", "Password2026!", "123456789012", "aaaaaaaaaaaa", "abcabcabcabc", "qwertyuiop12", "Supervisor2026"] {
            assert!(password_problem(weak).is_some(), "{weak}");
        }
        for fine in ["correct horse battery", "Tea at 4 in MG.201", "my-dog-is-called-Rex"] {
            assert!(password_problem(fine).is_none(), "{fine}");
        }
    }

    #[test]
    fn fingerprint_tracks_changes() {
        let mut db = sample();
        let before = fingerprint(&db);
        assert_eq!(before, fingerprint(&db));
        db.students[0].notes.push('x');
        assert_ne!(before, fingerprint(&db));
    }
}
