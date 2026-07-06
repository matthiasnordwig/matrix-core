// CRUD + retrieval tests for the ontology/GraphRAG schema, all against an
// in-memory database. Split into focused files (convention: CLAUDE.md
// "tests.rs > ~600 lines -> split into tests/foo.rs + mod tests { mod foo; }")
// once the original flat tests.rs approached that size.
mod communities;
mod fixtures;
mod nodes_edges;
mod retrieval;
mod misc_state;
mod lenses_reviews;
mod type_vector_cache;
