//! Tests for `ref_abbreviations` CRUD: round-trip, UNIQUE (case-insensitive)
//! violation, update, delete.

use crate::db::models::*;
use crate::db::Database;

fn db() -> Database {
    Database::open_in_memory().expect("open in-memory db")
}

#[test]
fn crud_roundtrip() {
    let db = db();
    let created = db
        .create_ref_abbreviation(&NewRefAbbreviation {
            kuerzel: "  EnWG  ".into(),
            long_names: vec!["Energiewirtschaftsgesetz".into()],
            enabled: true,
        })
        .unwrap();
    // Trimmed + lowercased on write.
    assert_eq!(created.kuerzel, "enwg");
    assert_eq!(created.long_names, vec!["Energiewirtschaftsgesetz".to_string()]);
    assert!(created.enabled);

    let fetched = db.ref_abbreviation(created.id).unwrap().unwrap();
    assert_eq!(fetched.kuerzel, "enwg");
    assert_eq!(fetched.long_names, created.long_names);

    let all = db.list_ref_abbreviations().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, created.id);
}

#[test]
fn kuerzel_unique_case_insensitive() {
    let db = db();
    db.create_ref_abbreviation(&NewRefAbbreviation {
        kuerzel: "kwg".into(),
        long_names: vec!["Kreditwesengesetz".into()],
        enabled: true,
    })
    .unwrap();

    // Differently-cased duplicate must violate the UNIQUE COLLATE NOCASE
    // constraint (normalize_kuerzel lowercases first, but the DB constraint
    // is the actual guard against any bypass / race).
    let err = db.create_ref_abbreviation(&NewRefAbbreviation {
        kuerzel: "KWG".into(),
        long_names: vec![],
        enabled: true,
    });
    assert!(err.is_err(), "expected UNIQUE violation, got {err:?}");
}

#[test]
fn update_changes_fields_and_bumps_updated_at() {
    let db = db();
    let created = db
        .create_ref_abbreviation(&NewRefAbbreviation {
            kuerzel: "gwg".into(),
            long_names: vec!["Geldwaschegesetz".into()],
            enabled: true,
        })
        .unwrap();

    let updated = db
        .update_ref_abbreviation(
            created.id,
            &NewRefAbbreviation {
                kuerzel: "gwg".into(),
                long_names: vec!["Geldwäschegesetz".into(), "GwG".into()],
                enabled: false,
            },
        )
        .unwrap();
    assert_eq!(updated.long_names, vec!["Geldwäschegesetz".to_string(), "GwG".to_string()]);
    assert!(!updated.enabled);
    assert!(updated.updated_at >= created.updated_at);
}

#[test]
fn delete_removes_row() {
    let db = db();
    let created = db
        .create_ref_abbreviation(&NewRefAbbreviation {
            kuerzel: "vag".into(),
            long_names: vec!["Versicherungsaufsichtsgesetz".into()],
            enabled: true,
        })
        .unwrap();

    assert!(db.delete_ref_abbreviation(created.id).unwrap());
    assert!(db.ref_abbreviation(created.id).unwrap().is_none());
    assert!(db.list_ref_abbreviations().unwrap().is_empty());
    // Deleting a non-existent id returns false, not an error.
    assert!(!db.delete_ref_abbreviation(created.id).unwrap());
}
