fn main() {
    let schema = serde_json::json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "Anforderung": {"type": "string"}
            },
            "required": ["Anforderung"],
            "additionalProperties": false
        }
    });
    match llama_cpp_2::json_schema_to_grammar(&schema.to_string()) {
        Ok(g) => println!("Grammar: {}", g),
        Err(e) => println!("Error: {}", e),
    }
}
