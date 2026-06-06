use std::fs;
use app::chunking::structural::chunk_pdf_structurally;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run --bin test_chunker <path-to-pdf>");
        return;
    }
    let pdf_path = &args[1];
    let bytes = fs::read(pdf_path).expect("Could not read PDF");

    use app::db::models::{StructuralProfile, StructuralPattern};
    let struct_profile = StructuralProfile {
        id: 1,
        name: "Test Profile".into(),
        min_chunk_chars: 200,
        max_chunk_chars: 1500,
        created_at: 0,
        updated_at: 0,
        patterns: vec![
            StructuralPattern {
                id: 1,
                profile_id: 1,
                group_name: "Überschriften".to_string(),
                role: "heading_l1".to_string(),
                regex: r"^((?:Article|Art\.|§|AT|Kapitel|Abschnitt|TITEL|TITLE|CHAPTER)\s*[\d.a-zA-Z]+)\s*(.*)".to_string(),
                flags: "i".to_string(),
                priority: 100,
                label: None,
                sort_order: 0,
            },
            StructuralPattern {
                id: 2,
                profile_id: 1,
                group_name: "Definitionen".to_string(),
                role: "definition".to_string(),
                regex: r"\b(?:means|shall mean|bezeichnet|gilt als|im Sinne)".to_string(),
                flags: "i".to_string(),
                priority: 50,
                label: None,
                sort_order: 1,
            },
            StructuralPattern {
                id: 3,
                profile_id: 1,
                group_name: "Ignorieren".to_string(),
                role: "ignore".to_string(),
                regex: r"(?:Seite|Page|Bundesgesetzblatt|Amtsblatt|BAnz)".to_string(),
                flags: "i".to_string(),
                priority: 200,
                label: None,
                sort_order: 2,
            },
            StructuralPattern {
                id: 4,
                profile_id: 1,
                group_name: "TOC Ignorieren".to_string(),
                role: "ignore".to_string(),
                regex: r"\s{3,}\d+$".to_string(),
                flags: "i".to_string(),
                priority: 210,
                label: None,
                sort_order: 3,
            },
        ],
    };

    println!("Starting structural chunker...");
    match chunk_pdf_structurally(&bytes, 0, 0, Some(struct_profile)) {
        Ok(chunks) => {
            println!("Generated {} chunks.", chunks.len());
            // Write to JSON
            let out_name = format!("{}_chunks.json", std::path::Path::new(pdf_path).file_stem().unwrap().to_string_lossy());
            if let Ok(json) = serde_json::to_string_pretty(&chunks) {
                fs::write(&out_name, json).unwrap();
                println!("Wrote chunks to {}", out_name);
            }
        }
        Err(e) => {
            println!("Error: {}", e);
        }
    }
}
