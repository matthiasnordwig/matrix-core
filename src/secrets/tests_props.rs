//! Property tests (proptest) for the value encryption. Case count from `crate::props`
//! (`PROPTEST_CASES`, stage 2: 32), see `handbuch/arbeitsablauf.md` "Teststufen".
//!
//! Rule (30): `decrypt(encrypt(p)) == p`; another `aad` fails ("a value copied into a different
//! field does not decrypt"); any other text gives an error, never a panic.

use proptest::prelude::*;

use super::{decrypt, encrypt, SecretKey, ENC_PREFIX};

proptest! {
    #![proptest_config(crate::props::config())]

    /// Rule (30).
    #[test]
    fn rule_30_round_trip_and_errors(
        plain in ".{0,40}",
        aad in "[a-z_.]{0,20}",
        other in "[a-z_.]{0,20}",
        junk in prop_oneof![".{0,60}", "[A-Za-z0-9+/=]{0,80}".prop_map(|s| format!("{ENC_PREFIX}{s}"))],
    ) {
        let key = SecretKey::generate();
        let sealed = encrypt(&key, &aad, &plain);
        prop_assert_eq!(decrypt(&key, &aad, &sealed).unwrap(), plain);
        if other != aad {
            prop_assert!(decrypt(&key, &other, &sealed).is_err());
        }
        prop_assert!(decrypt(&key, &aad, &junk).is_err());
    }
}
