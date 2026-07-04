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
mod dedup_cache;
mod quarantine;
