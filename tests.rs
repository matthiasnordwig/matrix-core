fn main() {}

#[test]
fn test_grammar() {
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
    let g = llama_cpp_2::json_schema_to_grammar(&schema.to_string()).unwrap();
    println!("GRAMMAR:\n{}", g);
}
