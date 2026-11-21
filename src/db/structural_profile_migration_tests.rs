//! schema_v82 + v84: the seeded heading pattern of "Legal Texts (EU/MaRisk)" learns BT/BTO/BTR (v82) and then
//! needs a number after the label (v84); an edited pattern stays as the user left it.

use super::Database;

const V81: &str = r"^((?:Article|Artikel|Art\.|§|AT|Kapitel|Abschnitt|TITEL|TITLE|CHAPTER)\s*[\d.a-zA-Z]+)\s*(.*)";
const V82: &str = r"^((?:Article|Artikel|Art\.|§|AT|BTO|BTR|BT|Kapitel|Abschnitt|TITEL|TITLE|CHAPTER)\s*[\d.a-zA-Z]+)\s*(.*)";
const V84: &str = r"^((?:Article|Artikel|Art\.|§|AT|BTO|BTR|BT|Kapitel|Abschnitt|TITEL|TITLE|CHAPTER)(?:\s*\d[\d.]*(?-i:[a-z])?|\s+(?-i:[IVXLC]+)))((?:\s|(?-i:[A-ZÄÖÜ(])).*|)$";
const MINE: &str = r"^(§\s*\d+)\s*(.*)";

/// A database at `version` with the seeded profile (id 2, `seeded` pattern) and one of the user's (id 3).
fn profiles_at(version: usize, seeded: &str) -> Database {
    let old = super::vision_tests::migrate_to(version);
    old.execute("INSERT INTO structural_profiles (id, name, definition_triggers, heading_triggers, ignore_patterns, target_chunk_size) VALUES (2, 'Legal Texts (EU/MaRisk)', '', '', '', 1500), (3, 'Mine', '', '', '', 1500)", []).unwrap();
    let insert = "INSERT INTO structural_patterns (profile_id, group_name, role, regex, flags, priority, sort_order) VALUES (?1, 'h', 'heading_l1', ?2, 'i', 100, 0)";
    old.execute(insert, rusqlite::params![2, seeded]).unwrap();
    old.execute(insert, rusqlite::params![3, MINE]).unwrap();
    let db = Database::init(old).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);
    db
}

fn regex(db: &Database, id: i64) -> String {
    db.structural_profile(id).unwrap().unwrap().patterns[0].regex.clone()
}

#[test]
fn v82_and_v84_change_only_the_seeded_heading_pattern() {
    let db = profiles_at(81, V81);
    assert_eq!(regex(&db, 2), V84);
    assert_eq!(regex(&db, 3), MINE);
    let db = profiles_at(83, V82);
    assert_eq!(regex(&db, 2), V84);
    assert_eq!(regex(&db, 3), MINE);
}

/// (line, section, title) for headings; `None` = no heading.
#[test]
fn v84_pattern_needs_a_number_and_keeps_glued_titles() {
    let re = regex::RegexBuilder::new(V84).case_insensitive(true).build().unwrap();
    let cut = |line: &str| re.captures(line).map(|c| (c[1].to_string(), c[2].trim().to_string()));
    let pair = |a: &str, b: &str| Some((a.to_string(), b.to_string()));
    assert_eq!(cut("BTO 1.2 Anforderungen an die Prozesse im Kreditgeschäft"), pair("BTO 1.2", "Anforderungen an die Prozesse im Kreditgeschäft"));
    assert_eq!(cut("§ 25fBesondere Anforderungen an die ordnungsgemäße Geschäftsorganisation"), pair("§ 25f", "Besondere Anforderungen an die ordnungsgemäße Geschäftsorganisation"));
    assert_eq!(cut("Artikel 92a(1)"), pair("Artikel 92a", "(1)"));
    assert_eq!(cut("AT 4.3.2 Risikosteuerungs- und -controllingprozesse"), pair("AT 4.3.2", "Risikosteuerungs- und -controllingprozesse"));
    assert_eq!(cut("TITEL II"), pair("TITEL II", ""));
    assert_eq!(cut("Artikel 4"), pair("Artikel 4", ""));
    for prose in ["Attention is drawn to", "BTO Anforderungen an das Kreditgeschäft", "Artikels 26 gilt", "Kapitel bestehen", "Abschnitt Civil", "Artikel 12, 13 und 14 gelten"] {
        assert_eq!(cut(prose), None, "{prose}");
    }
}
