//! `Database` methods for the ontology/GraphRAG schema (profiles, nodes,
//! edges, communities, retrieval, metrics, dedup cache, quarantine +
//! resumability, lenses, run log). One file per concern; each submodule adds
//! methods to `Database` via its own `impl Database { ... }` block, so nothing
//! needs to be re-exported here.
//!
//! Lens system: `entity_type`/`relation_type` and their `raw_*` twins are
//! identical from insert on and stay so; only manual curation rewrites both.
//! A lens (`lenses.rs`) is a display-time overlay in separate tables, and every
//! read that shows types to a user or LLM joins `contexts.active_lens_id`
//! (`COALESCE(lens value, raw)`). The un-lensed reads remain for the pipeline
//! and export.
//!
//! Graph vectors carry their embedding model (`vector_model_id`, schema_v67):
//! every reader compares only vectors of the context's current model
//! (`graph_vectors.rs`).
//!
//! Re-made chunks keep the graph's evidence: `evidence_carry.rs` (pure rules),
//! `evidence_carry_db.rs` (snapshot + replace in one savepoint),
//! `evidence_parking.rs` (a deleted document's evidence until the same file is
//! added again), `evidence_restore.rs` (from an old backup).
mod profiles;
mod nodes;
mod edges;
mod communities;
mod lifecycle;
mod retrieval;
mod metrics;
mod run_log;
mod dedup_cache;
mod quarantine;
mod edge_reviews;
mod lenses;
mod schema_suggestions;
mod type_vector_cache;
mod graph_vectors;
pub mod evidence_carry;
mod evidence_carry_db;
pub mod evidence_parking;
pub mod evidence_restore;
pub use evidence_carry_db::{apply_carry, preview_carry, read_chunk_spans, read_document_graph};

#[cfg(test)]
mod tests;
