fn main() {
    let entity_type_schema = serde_json::json!({"type": "string", "description": "The type of the entity"});
    let relation_type_schema = serde_json::json!({"type": "string", "description": "The type of relation"});
    let json_schema = serde_json::json!({
        "type": "object",
        "properties": {
            "n": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer" },
                        "l": { "type": "string", "description": "The name of the entity" },
                        "t": entity_type_schema,
                        "d": { "type": "string", "description": "A concise description of the entity in this context" }
                    },
                    "required": ["id", "l", "t", "d"]
                }
            },
            "e": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "s": { "type": "integer", "description": "Source node id" },
                        "t": { "type": "integer", "description": "Target node id" },
                        "r": relation_type_schema,
                        "q": { "type": "string", "description": "A 3-5 word exact quote from the text that proves this relation" }
                    },
                    "required": ["s", "t", "r"]
                }
            }
        },
        "required": ["n", "e"]
    });
    let schema_str = serde_json::to_string(&json_schema).unwrap();
    let grammar_str = llama_cpp_2::json_schema_to_grammar(&schema_str).unwrap();
    println!("Grammar length: {}", grammar_str.len());
    // Try to parse the grammar
    let model = llama_cpp_2::model::LlamaModel::load_from_file("/dev/null", llama_cpp_2::model::params::LlamaModelParams::default());
    // We can't easily load a model here, so we just print the grammar length.
}
