//! Encryption of stored API keys (pure Rust, builds for iOS and manylinux).
//!
//! A random 32-byte key lives in `secrets.key` in the app data folder, next to
//! (but separate from) `matrix.db`. Secret fields are stored as
//! `enc:v1:<base64(nonce ‖ ciphertext)>`, sealed with XChaCha20-Poly1305 and a
//! fresh random 24-byte nonce per value (large enough that random nonces never
//! repeat in practice). The associated data (`aad`) names the field
//! (e.g. `"llm_endpoints.api_key_ref"`), so a value copied into a different
//! field does not decrypt.
//!
//! Scope and limit: this protects copies of `matrix.db` alone (backups, work
//! copies, mail attachments). Whoever has the whole data folder also has
//! `secrets.key`. Which fields are sealed and when is decided in
//! `db/secret_fields.rs`; this file only knows bytes and strings.

use std::fmt;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

/// File name of the key inside the data folder.
pub const KEY_FILE: &str = "secrets.key";
/// Prefix of every encrypted value (format version 1).
pub const ENC_PREFIX: &str = "enc:v1:";
/// Prefix shared by all (present and future) encrypted formats. A value that
/// starts with it is never encrypted again and never sent anywhere.
pub const ENC_ANY_PREFIX: &str = "enc:";

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("secrets key file {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("secrets key file {path} has {len} bytes, expected {KEY_LEN}")]
    BadKeyLength { path: PathBuf, len: usize },
    #[error("not an encrypted value (expected prefix {ENC_PREFIX})")]
    NotEncrypted,
    #[error("encrypted value is malformed")]
    Malformed,
    #[error("encrypted value can't be decrypted (wrong key or wrong field)")]
    Decrypt,
}

/// The 32-byte data key. `Debug` never prints the bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretKey([u8; KEY_LEN]);

impl SecretKey {
    /// A new random key from the operating system's random source.
    pub fn generate() -> Self {
        let k = XChaCha20Poly1305::generate_key(&mut OsRng);
        let mut b = [0u8; KEY_LEN];
        b.copy_from_slice(&k);
        Self(b)
    }

    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new((&self.0).into())
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey(<redacted>)")
    }
}

/// Path of the key file inside `dir`.
pub fn key_path(dir: &Path) -> PathBuf {
    dir.join(KEY_FILE)
}

/// Read an existing key file. Missing file = `Io` error with `NotFound`;
/// a file of the wrong length is an error, never silently replaced.
pub fn load(path: &Path) -> Result<SecretKey, SecretError> {
    let io = |source| SecretError::Io { path: path.to_path_buf(), source };
    let mut f = std::fs::File::open(path).map_err(io)?;
    let mut buf = Vec::with_capacity(KEY_LEN);
    f.read_to_end(&mut buf).map_err(io)?;
    let bytes: [u8; KEY_LEN] = buf
        .as_slice()
        .try_into()
        .map_err(|_| SecretError::BadKeyLength { path: path.to_path_buf(), len: buf.len() })?;
    Ok(SecretKey(bytes))
}

/// Load `dir/secrets.key`, or create it with a fresh random key when absent
/// (`create_new`, so two processes never overwrite each other's key; on Unix
/// with permissions 0600). Creates `dir` if needed.
pub fn load_or_create(dir: &Path) -> Result<SecretKey, SecretError> {
    let path = key_path(dir);
    let io = |source| SecretError::Io { path: path.clone(), source };
    if path.exists() {
        return load(&path);
    }
    std::fs::create_dir_all(dir).map_err(io)?;
    // Write the full key to a private temp file first, then hard-link it to the
    // final name: the link fails atomically if the name exists, so a crash or a
    // racing process never leaves a partial `secrets.key` behind.
    let tmp = dir.join(format!("{KEY_FILE}.tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let key = SecretKey::generate();
    let written = opts.open(&tmp).and_then(|mut f| f.write_all(&key.0).and_then(|_| f.sync_all()));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(io(e));
    }
    let linked = std::fs::hard_link(&tmp, &path);
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(key),
        // Another process created it in the meantime: use theirs.
        Err(e) if e.kind() == ErrorKind::AlreadyExists => load(&path),
        Err(e) => Err(io(e)),
    }
}

/// Whether `s` is an encrypted value (any `enc:` format version).
pub fn is_encrypted(s: &str) -> bool {
    s.starts_with(ENC_ANY_PREFIX)
}

/// Seal `plain` for the field named by `aad` → `enc:v1:<b64(nonce ‖ ct)>`.
pub fn encrypt(key: &SecretKey, aad: &str, plain: &str) -> String {
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ct = key
        .cipher()
        .encrypt(&nonce, Payload { msg: plain.as_bytes(), aad: aad.as_bytes() })
        // Only fails for plaintexts beyond ~256 GiB.
        .expect("XChaCha20-Poly1305 encryption");
    let mut blob = Vec::with_capacity(NONCE_LEN + ct.len());
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    format!("{ENC_PREFIX}{}", B64.encode(blob))
}

/// Open a value produced by `encrypt` with the same key and `aad`.
pub fn decrypt(key: &SecretKey, aad: &str, s: &str) -> Result<String, SecretError> {
    let b64 = s.strip_prefix(ENC_PREFIX).ok_or(SecretError::NotEncrypted)?;
    let blob = B64.decode(b64).map_err(|_| SecretError::Malformed)?;
    if blob.len() < NONCE_LEN {
        return Err(SecretError::Malformed);
    }
    let (nonce, ct) = blob.split_at(NONCE_LEN);
    let plain = key
        .cipher()
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad: aad.as_bytes() })
        .map_err(|_| SecretError::Decrypt)?;
    String::from_utf8(plain).map_err(|_| SecretError::Malformed)
}

#[cfg(test)]
mod tests;
