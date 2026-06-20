fn main() {
    let schema_str = r#"{"mode":"array","fields":[{"key":"Gedankengang","type":"string","description":"Analysiere den Chunk Schritt für Schritt: Ist es eine Definition, eine rechtliche Herleitung oder eine konkrete Anforderung an IT-Systeme? Erkläre genau warum."},{"key":"Ist_anforderung","type":"boolean","description":"Basierend auf deinem Gedankengang: „true\" wenn der Chunk eine Anforderung enthält, „false\" falls er keine Anforderung enthält"},{"key":"Kontext_anforderung","type":"string","description":"Welche ähnliche Anforderung gibt es im Kontext? (Oder leer lassen)"},{"key":"Kontext_anforderung_verweis","type":"string","description":"Verweis zum Kontext z.B. das Kürzel, oder der Titel"}]}"#;
    
    match llama_cpp_2::json_schema_to_grammar(schema_str) {
        Ok(g) => println!("GRAMMAR:\n{}", g),
        Err(e) => println!("ERROR:\n{}", e),
    }
}
