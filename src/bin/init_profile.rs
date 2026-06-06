use std::path::PathBuf;
use app::db::Database;
use app::db::models::NewStructuralProfile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME").expect("No HOME");
    let mut db_path = PathBuf::from(home);
    db_path.push("Library");
    db_path.push("Application Support");
    db_path.push("com.matthiasnordwig.matrix");
    db_path.push("matrix.db");

    println!("Opening DB at {:?}", db_path);
    let db = Database::open(db_path)?;

    println!("Creating structural profile...");
    use app::db::models::NewStructuralPattern;
    let profile = NewStructuralProfile {
        name: "Rechtstexte (EU/MaRisk)".to_string(),
        min_chunk_chars: 200,
        max_chunk_chars: 1500,
        patterns: vec![
            NewStructuralPattern {
                group_name: "Überschriften".to_string(),
                role: "heading_l1".to_string(),
                regex: r"^((?:Article|Art\.|§|AT|Kapitel|Abschnitt|TITEL|TITLE|CHAPTER)\s*[\d.a-zA-Z]+)\s*(.*)".to_string(),
                flags: "i".to_string(),
                priority: 100,
                label: None,
                sort_order: 0,
            },
            NewStructuralPattern {
                group_name: "Definitionen".to_string(),
                role: "definition".to_string(),
                regex: r"\b(?:means|shall mean|bezeichnet|gilt als|im Sinne)".to_string(),
                flags: "i".to_string(),
                priority: 50,
                label: None,
                sort_order: 1,
            },
            NewStructuralPattern {
                group_name: "Ignorieren".to_string(),
                role: "ignore".to_string(),
                regex: r"(?:Seite|Page|Bundesgesetzblatt|Amtsblatt|BAnz)".to_string(),
                flags: "i".to_string(),
                priority: 200,
                label: None,
                sort_order: 2,
            },
            NewStructuralPattern {
                group_name: "TOC Ignorieren".to_string(),
                role: "ignore".to_string(),
                regex: r"\s{3,}\d+$".to_string(),
                flags: "i".to_string(),
                priority: 210,
                label: None,
                sort_order: 3,
            },
            NewStructuralPattern {
                group_name: "Aufzählungen (Nummern)".to_string(),
                role: "heading_l1".to_string(),
                regex: r"^(\(\d+\))\s*(.*)".to_string(),
                flags: "i".to_string(),
                priority: 110,
                label: None,
                sort_order: 4,
            },
        ]
    };

    match db.create_structural_profile(&profile) {
        Ok(p) => println!("Successfully created profile: {}", p.name),
        Err(e) => println!("Error creating profile: {}", e),
    }

    Ok(())
}
