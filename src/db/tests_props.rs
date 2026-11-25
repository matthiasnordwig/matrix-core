//! Property tests (proptest) for the pure helpers of the database layer: the
//! vector BLOB codec, cosine, the FTS5 query escape and the SQL column names of
//! sheet tables. Case count per test from `PROPTEST_CASES` (stage 2 default
//! here: 32; stage 3: 2000), see `handbuch/arbeitsablauf.md` "Teststufen".
//! Counterexamples are stored in `core/proptest-regressions/db/` (checked in)
//! and get a named example test at the end of this file.
//!
//! Rules (from the doc comments at each function):
//! (19) `blob_to_vector(vector_to_blob(v))` gives `v` back bit for bit (NaN
//!      payloads and -0.0 included); a length not divisible by 4 is an error;
//!      `cosine` of finite vectors lies in [-1, 1] (up to f32 rounding, 1e-5).
//! (20) every `escape_fts_query` output is a valid `MATCH` expression in a
//!      throwaway FTS5 table with the production tokenizer ("would otherwise be
//!      parsed as operators or raise a syntax error"); `None` only when no term
//!      holds a letter or digit.
//! (21) `sql_column_names`: one name per header, all distinct, none empty, none
//!      `sheet_row` ("sanitised, `col_<n>` when empty, unique (also against
//!      `sheet_row`)"); only `[a-z0-9_]`, so SQLite's case-insensitive names
//!      cannot collide either.

use proptest::prelude::*;

use super::embeddings::{blob_to_vector, cosine, vector_to_blob};
use super::fts::escape_fts_query;
use super::sheet_tables::{sql_column_names, SHEET_ROW_COLUMN};


/// Words that mean something to the FTS5 query grammar, plus symbols and
/// characters the `unicode61` tokenizer drops or splits.
fn fts_query() -> impl Strategy<Value = String> {
    let piece = prop::sample::select(vec![
        "AND", "OR", "NOT", "NEAR", "NEAR(", "\"", "\"\"", "'", "*", "^", "-", "+", ":", "(", ")", "{", "}", "§", "25a",
        "KWG", "Art.", "AT", "4.3.2", "ä", "²", "½", "日本", "\u{301}", "col:", "chunks", " ", " ", "\t", "\n", "x",
    ]);
    prop_oneof![
        4 => prop::collection::vec(piece, 0..10).prop_map(|v| v.concat()),
        1 => any::<String>(),
    ]
}

proptest! {
    #![proptest_config(crate::props::config())]

    /// Rule (19).
    #[test]
    fn rule_19_blob_round_trip_and_cosine_range(
        bits in prop::collection::vec(any::<u32>(), 0..40),
        odd in prop::collection::vec(any::<u8>(), 0..40),
        a in prop::collection::vec(-1000.0f32..1000.0, 0..40),
        b in prop::collection::vec(-1000.0f32..1000.0, 0..40),
    ) {
        let v: Vec<f32> = bits.iter().map(|&x| f32::from_bits(x)).collect();
        let back = blob_to_vector(&vector_to_blob(&v)).unwrap();
        prop_assert_eq!(back.iter().map(|x| x.to_bits()).collect::<Vec<_>>(), bits);
        prop_assert_eq!(blob_to_vector(&odd).is_ok(), odd.len() % 4 == 0);
        let c = cosine(&a, &b);
        prop_assert!((-1.0 - 1e-5..=1.0 + 1e-5).contains(&c), "cosine {} for {:?} / {:?}", c, a, b);
        prop_assert!(cosine(&a, &a) == 0.0 || (cosine(&a, &a) - 1.0).abs() <= 1e-5);
    }

    /// Rule (20).
    #[test]
    fn rule_20_escaped_query_is_valid_fts5(q in fts_query()) {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE t USING fts5(text, tokenize = 'unicode61 remove_diacritics 2');
             INSERT INTO t(text) VALUES ('§ 25a KWG Art. 28 AT 4.3.2 Ärger x');",
        )
        .unwrap();
        match escape_fts_query(&q) {
            Some(m) => {
                let r: rusqlite::Result<i64> = conn.query_row("SELECT count(*) FROM t WHERE t MATCH ?1", [&m], |r| r.get(0));
                prop_assert!(r.is_ok(), "{:?} → {:?}: {:?}", q, m, r.err());
            }
            None => prop_assert!(!q.split_whitespace().any(|t| t.chars().any(char::is_alphanumeric)), "None for {:?}", q),
        }
    }

    /// Rule (21).
    #[test]
    fn rule_21_sql_column_names_unique(
        headers in prop::collection::vec(
            prop_oneof![
                prop::sample::select(vec!["", " ", "Name", "name", "NAME", "sheet_row", "Sheet Row", "col_1", "col_2", "Umsatz €", "%",
                    "1. Quartal", "select", "Größe", "a__b", "_", "İ"]).prop_map(String::from),
                ".{0,6}",
                "[a-z]{60}",
            ],
            0..12,
        ),
    ) {
        let names = sql_column_names(&headers);
        prop_assert_eq!(names.len(), headers.len());
        let mut seen = std::collections::HashSet::new();
        for n in &names {
            prop_assert!(!n.is_empty() && n != SHEET_ROW_COLUMN, "bad name {:?} for {:?}", n, headers);
            prop_assert!(n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'), "{:?}", n);
            prop_assert!(seen.insert(n.clone()), "{:?} twice for {:?}", n, headers);
        }
    }
}
