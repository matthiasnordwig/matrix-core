//! Shared setup of the property tests (`proptest`, `tests_props.rs` of each module): the case
//! count per property. `PROPTEST_CASES` if set (stage 3 / release: 2000), else 32 so that stage 2
//! stays fast (`handbuch/arbeitsablauf.md` "Teststufen").

use proptest::prelude::ProptestConfig;

/// Cases per property: `PROPTEST_CASES` if set, else 32.
pub(crate) fn config() -> ProptestConfig {
    let mut c = ProptestConfig::default();
    if std::env::var("PROPTEST_CASES").is_err() {
        c.cases = 32;
    }
    c
}
