//! `Database` methods for the ontology/GraphRAG schema (profiles, nodes,
//! edges, communities, retrieval, metrics, dedup cache, quarantine +
//! resumability). Split into one file per concern — see HANDBUCH.md for the
//! full map. Each submodule adds methods to `Database` via its own
//! `impl Database { ... }` block; nothing needs to be re-exported here.
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

#[cfg(test)]
mod tests;
