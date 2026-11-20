//! schema_v82: the seeded heading pattern of "Legal Texts (EU/MaRisk)" learns BT/BTO/BTR; an edited
//! pattern stays as the user left it.

use super::Database;

const OLD: &str = r"^((?:Article|Artikel|Art\.|§|AT|Kapitel|Abschnitt|TITEL|TITLE|CHAPTER)\s*[\d.a-zA-Z]+)\s*(.*)";
const NEW: &str = r"^((?:Article|Artikel|Art\.|§|AT|BTO|BTR|BT|Kapitel|Abschnitt|TITEL|TITLE|CHAPTER)\s*[\d.a-zA-Z]+)\s*(.*)";

#[test]
fn v82_widens_only_the_seeded_heading_pattern() {
    let old = super::vision_tests::migrate_to(81);
    old.execute("INSERT INTO structural_profiles (id, name, definition_triggers, heading_triggers, ignore_patterns, target_chunk_size) VALUES (2, 'Legal Texts (EU/MaRisk)', '', '', '', 1500), (3, 'Mine', '', '', '', 1500)", []).unwrap();
    let insert = "INSERT INTO structural_patterns (profile_id, group_name, role, regex, flags, priority, sort_order) VALUES (?1, 'h', 'heading_l1', ?2, 'i', 100, 0)";
    old.execute(insert, rusqlite::params![2, OLD]).unwrap();
    old.execute(insert, rusqlite::params![3, r"^(§\s*\d+)\s*(.*)"]).unwrap();
    let db = Database::init(old).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);
    let regex = |id| db.structural_profile(id).unwrap().unwrap().patterns[0].regex.clone();
    assert_eq!(regex(2), NEW);
    assert_eq!(regex(3), r"^(§\s*\d+)\s*(.*)");
    let re = regex::RegexBuilder::new(NEW).case_insensitive(true).build().unwrap();
    let caps = re.captures("BTO 1.2 Anforderungen an die Prozesse im Kreditgeschäft").unwrap();
    assert_eq!((&caps[1], &caps[2]), ("BTO 1.2", "Anforderungen an die Prozesse im Kreditgeschäft"));
}
