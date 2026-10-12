//! Tests of the value encryption and the key file (`secrets.rs`).

use super::*;

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "matrix-secrets-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn round_trip_and_format() {
    let k = SecretKey::generate();
    let s = encrypt(&k, "llm_endpoints.api_key_ref", "sk-test-123");
    assert!(s.starts_with(ENC_PREFIX));
    assert!(is_encrypted(&s));
    assert!(!s.contains("sk-test-123"));
    assert_eq!(decrypt(&k, "llm_endpoints.api_key_ref", &s).unwrap(), "sk-test-123");
}

#[test]
fn fresh_nonce_per_value() {
    let k = SecretKey::generate();
    let a = encrypt(&k, "f", "same");
    let b = encrypt(&k, "f", "same");
    assert_ne!(a, b);
}

#[test]
fn wrong_aad_fails() {
    let k = SecretKey::generate();
    let s = encrypt(&k, "llm_endpoints.api_key_ref", "sk-x");
    assert!(matches!(
        decrypt(&k, "embedding_models.api_config.api_key", &s),
        Err(SecretError::Decrypt)
    ));
}

#[test]
fn wrong_key_fails() {
    let s = encrypt(&SecretKey::generate(), "f", "sk-x");
    assert!(matches!(decrypt(&SecretKey::generate(), "f", &s), Err(SecretError::Decrypt)));
}

#[test]
fn tampered_or_malformed_fails() {
    let k = SecretKey::generate();
    let s = encrypt(&k, "f", "sk-x");
    let mut bytes = B64.decode(s.strip_prefix(ENC_PREFIX).unwrap()).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    let tampered = format!("{ENC_PREFIX}{}", B64.encode(bytes));
    assert!(matches!(decrypt(&k, "f", &tampered), Err(SecretError::Decrypt)));
    assert!(matches!(decrypt(&k, "f", "enc:v1:!!!"), Err(SecretError::Malformed)));
    assert!(matches!(decrypt(&k, "f", "enc:v1:AAAA"), Err(SecretError::Malformed)));
    assert!(matches!(decrypt(&k, "f", "plain"), Err(SecretError::NotEncrypted)));
    assert!(!is_encrypted("sk-plain"));
}

#[test]
fn debug_redacts_key() {
    let k = SecretKey::from_bytes([7u8; 32]);
    assert_eq!(format!("{k:?}"), "SecretKey(<redacted>)");
}

#[test]
fn load_or_create_creates_once_and_reloads() {
    let dir = tmp_dir("create");
    let a = load_or_create(&dir).unwrap();
    let b = load_or_create(&dir).unwrap();
    assert_eq!(a, b);
    assert_eq!(std::fs::read(key_path(&dir)).unwrap().len(), 32);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(key_path(&dir)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn wrong_length_key_file_is_an_error_and_kept() {
    let dir = tmp_dir("badlen");
    std::fs::write(key_path(&dir), b"short").unwrap();
    assert!(matches!(load_or_create(&dir), Err(SecretError::BadKeyLength { len: 5, .. })));
    assert_eq!(std::fs::read(key_path(&dir)).unwrap(), b"short");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn load_missing_file_is_not_found() {
    let dir = tmp_dir("missing");
    match load(&key_path(&dir)) {
        Err(SecretError::Io { source, .. }) => assert_eq!(source.kind(), ErrorKind::NotFound),
        other => panic!("expected NotFound, got {other:?}"),
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
