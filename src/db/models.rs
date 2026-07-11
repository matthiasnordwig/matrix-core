//! Typed row models, mirrored 1:1 by the future `ipc.ts` so the frontend and
//! core never drift. All structs derive `Serialize`/`Deserialize`.

use serde::{Deserialize, Serialize};

// --- Enums -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    LocalOnnx,
    RemoteApi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionProvider {
    Ane,
    Coreml,
    Cpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextStatus {
    Created,
    Ingesting,
    Staged,
    Embedded,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStrategy {
    ExactForward,
    Fuzzy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrechunkStatus {
    Pending,
    Sent,
    Done,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatStatus {
    Queued,
    Retrieving,
    Llm,
    Done,
    Error,
}

/// `Chunk` = row is backed by a real `chunks` row (`row_ref_id` = chunk id).
/// `GridRow` = repurposed for Excel/CSV upload rows (the `grid_sheets`/
/// `grid_rows` tables this variant was originally named for are dead code —
/// no command creates them): `row_ref_id` is a synthetic, per-run-stable id
/// the frontend assigns (negative index), and `row_source_text` on
/// `GridChatResult`/`GridChatUpsert` carries the row's source text since
/// there's no `chunks` row to reconstruct it from on history load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowRefType {
    Chunk,
    GridRow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GridDataFormat {
    Plain,
    Json,
}

// --- Registries ------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingModel {
    pub id: i64,
    pub identifier: String,
    pub kind: ModelKind,
    pub model_path: Option<String>,
    pub tokenizer_path: Option<String>,
    pub api_config: Option<String>,
    pub execution_provider: Option<ExecutionProvider>,
    pub is_matryoshka: bool,
    pub native_dim: i64,
    pub default_dim: i64,
    pub normalize: bool,
    // Rate/throughput limits for remote endpoints (None = unlimited); local ONNX ignores these.
    pub tpm_limit: Option<i64>,
    pub rpm_limit: Option<i64>,
    pub max_concurrency: i64,
    pub created_at: i64,
}

/// Fields required to create an [`EmbeddingModel`]; `id`/`created_at` are assigned by the DB.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEmbeddingModel {
    pub identifier: String,
    pub kind: ModelKind,
    pub model_path: Option<String>,
    pub tokenizer_path: Option<String>,
    pub api_config: Option<String>,
    pub execution_provider: Option<ExecutionProvider>,
    pub is_matryoshka: bool,
    pub native_dim: i64,
    pub default_dim: i64,
    pub normalize: bool,
    pub tpm_limit: Option<i64>,
    pub rpm_limit: Option<i64>,
    pub max_concurrency: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmEndpoint {
    pub id: i64,
    pub name: String,
    pub base_url: String,
    pub model_id: String,
    pub api_key_ref: Option<String>,
    pub timeout_ms: i64,
    pub max_retries: i64,
    pub provider: String,
    pub window_tokens: i64,
    pub context_window: i64,
    pub output_reserve_tokens: i64,
    pub tpm_limit: Option<i64>,
    pub rpm_limit: Option<i64>,
    pub max_concurrency: i64,
    pub is_reasoning: bool,
    pub supports_structured_output: bool,
    pub stream_fallback: bool,
    pub kv_quantization: Option<String>,
    pub cpu_threads: Option<i64>,
    /// Optional FK to a `ReasoningEffortList` — the levels this endpoint's model
    /// accepts. Only meaningful when `is_reasoning`; consumed by the ontology
    /// extraction phase to constrain/validate its per-context effort setting.
    #[serde(default)]
    pub reasoning_list_id: Option<i64>,
    pub created_at: i64,
}

/// Default for `NewLlmEndpoint::stream_fallback` so JSON payloads (web adapter /
/// stored context bundles) predating the column still deserialize to the
/// enabled-by-default behaviour.
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewLlmEndpoint {
    pub name: String,
    pub base_url: String,
    pub model_id: String,
    pub api_key_ref: Option<String>,
    pub timeout_ms: i64,
    pub max_retries: i64,
    pub provider: String,
    pub window_tokens: i64,
    pub context_window: i64,
    pub output_reserve_tokens: i64,
    pub tpm_limit: Option<i64>,
    pub rpm_limit: Option<i64>,
    pub max_concurrency: i64,
    pub is_reasoning: bool,
    pub supports_structured_output: bool,
    #[serde(default = "default_true")]
    pub stream_fallback: bool,
    pub kv_quantization: Option<String>,
    pub cpu_threads: Option<i64>,
    #[serde(default)]
    pub reasoning_list_id: Option<i64>,
}

/// A reasoning-effort allow-list (`reasoning_effort_lists`): the set of
/// `reasoning_effort` levels a model family accepts, maintained in the Profile
/// tab and assigned to an `LlmEndpoint` via `reasoning_list_id`. `allowed_efforts`
/// is stored as a JSON array of level strings in the DB and (de)serialized in the
/// CRUD layer. Consumed by the ontology extraction phase — see the services-crate
/// `reasoning::clamp_effort`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningEffortList {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    pub allowed_efforts: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewReasoningEffortList {
    pub title: String,
    pub description: Option<String>,
    pub allowed_efforts: Vec<String>,
}

/// A named pool of `llm_endpoints`; members are resolved via
/// `Database::pool_members`/`list_pools_with_members`. Enforced invariant (see
/// `Database::set_pool_members`): at most one member may have `provider == "gguf"`
/// (only one on-device model can run at a time on this device).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmEndpointPool {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewLlmEndpointPool {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmEndpointPoolWithMembers {
    pub pool: LlmEndpointPool,
    pub members: Vec<LlmEndpoint>,
}

// --- Profiles & Contexts ---------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingProfile {
    pub id: i64,
    pub name: String,
    pub prompt: String,
    pub overlap_ratio: f64,
    pub max_signature_len: i64,
    pub metadata_fields: String,
    pub match_strategy: MatchStrategy,
    pub fuzzy_threshold: Option<f64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewChunkingProfile {
    pub name: String,
    pub prompt: String,
    pub overlap_ratio: f64,
    pub max_signature_len: i64,
    pub metadata_fields: String,
    pub match_strategy: MatchStrategy,
    pub fuzzy_threshold: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralPattern {
    pub id: i64,
    pub profile_id: i64,
    pub group_name: String,
    pub role: String,
    pub regex: String,
    pub flags: String,
    pub priority: i64,
    pub label: Option<String>,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewStructuralPattern {
    pub group_name: String,
    pub role: String,
    pub regex: String,
    pub flags: String,
    pub priority: i64,
    pub label: Option<String>,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralProfile {
    pub id: i64,
    pub name: String,
    pub min_chunk_chars: i64,
    pub max_chunk_chars: i64,
    pub patterns: Vec<StructuralPattern>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewStructuralProfile {
    pub name: String,
    pub min_chunk_chars: i64,
    pub max_chunk_chars: i64,
    pub patterns: Vec<NewStructuralPattern>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridProfile {
    pub id: i64,
    pub name: String,
    pub system_prompt: String,
    pub data_format: GridDataFormat,
    pub json_schema: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewGridProfile {
    pub name: String,
    pub system_prompt: String,
    pub data_format: GridDataFormat,
    pub json_schema: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub chunking_strategy: String,
    pub chunking_profile_id: Option<i64>,
    pub structural_profile_id: Option<i64>,
    pub embedding_model_id: Option<i64>,
    pub embedding_dim: Option<i64>,
    pub llm_id: Option<i64>,
    pub fallback_llm_id: Option<i64>,
    pub ontology_profile_id: Option<i64>,
    /// Endpoint pool used for ontology extraction/dedup/community instead of
    /// `llm_id`, if set — persisted so the choice survives an app restart
    /// (previously only ephemeral frontend state, see ISSUES.md).
    pub ontology_pool_id: Option<i64>,
    /// Optional dedicated endpoint/pool used ONLY for the extraction +
    /// polarity-verification phases (see BACKLOG.md "Phasen-getrennte
    /// Modellwahl", `schema_v36.sql`) — the judgment-critical phases where a
    /// stronger/different model plausibly helps most. Dedup/community keep
    /// using the main source (`llm_id`/`ontology_pool_id`) regardless. At
    /// most one of `ontology_extract_llm_id`/`ontology_extract_pool_id`
    /// should be set at a time (same convention as `llm_id`/`ontology_pool_id`);
    /// both `None` = today's behavior (one source for everything).
    pub ontology_extract_llm_id: Option<i64>,
    pub ontology_extract_pool_id: Option<i64>,
    /// Reasoning-effort level requested for the ontology EXTRACTION phase only
    /// (not verify/dedup/community/chunking). `None` = unset = provider default.
    /// Validated at send-time against the runtime extraction endpoint's
    /// `reasoning_list_id` (see services `reasoning::clamp_effort`).
    #[serde(default)]
    pub ontology_extract_reasoning_effort: Option<String>,
    pub extract_title_llm: bool,
    pub auto_merge_ontology: bool,
    /// The lens (see `OntologyLens`) currently used to filter/relabel this
    /// context's graph for retrieval/dedup/community — `None` means "show raw,
    /// unfiltered" (the only possible state for a pre-Lens context, and the
    /// state a context falls back to if its active lens is deleted).
    pub active_lens_id: Option<i64>,
    pub status: ContextStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewContext {
    pub name: String,
    pub description: Option<String>,
    pub chunking_strategy: String,
    pub chunking_profile_id: Option<i64>,
    pub structural_profile_id: Option<i64>,
    pub embedding_model_id: Option<i64>,
    pub embedding_dim: Option<i64>,
    pub llm_id: Option<i64>,
    pub fallback_llm_id: Option<i64>,
    pub ontology_profile_id: Option<i64>,
    pub ontology_pool_id: Option<i64>,
    pub ontology_extract_llm_id: Option<i64>,
    pub ontology_extract_pool_id: Option<i64>,
    #[serde(default)]
    pub ontology_extract_reasoning_effort: Option<String>,
    pub extract_title_llm: bool,
    pub auto_merge_ontology: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: i64,
    pub context_id: i64,
    pub name: String,
    pub zip_entry: Option<String>,
    pub byte_size: Option<i64>,
    pub page_count: Option<i64>,
    pub content_hash: Option<String>,
    pub extracted_text: Option<String>,
    pub ingested_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDocument {
    pub context_id: i64,
    pub name: String,
    pub zip_entry: Option<String>,
    pub byte_size: Option<i64>,
    pub page_count: Option<i64>,
    pub content_hash: Option<String>,
    pub extracted_text: Option<String>,
}

// --- Staging ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prechunk {
    pub id: i64,
    pub document_id: i64,
    pub idx: i64,
    pub start_sentence: i64,
    pub end_sentence: i64,
    pub char_start: i64,
    pub char_end: i64,
    pub text: String,
    pub llm_status: PrechunkStatus,
    pub llm_response: Option<String>,
    pub attempts: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewPrechunk {
    pub document_id: i64,
    pub idx: i64,
    pub start_sentence: i64,
    pub end_sentence: i64,
    pub char_start: i64,
    pub char_end: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: i64,
    pub context_id: i64,
    pub document_id: i64,
    pub chunk_index: i64,
    pub char_start: i64,
    pub char_end: i64,
    pub text: String,
    pub signature: Option<String>,
    pub is_omitted: bool,
    pub metadata: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewChunk {
    pub context_id: i64,
    pub document_id: i64,
    pub chunk_index: i64,
    pub char_start: i64,
    pub char_end: i64,
    pub text: String,
    pub signature: Option<String>,
    pub is_omitted: bool,
    pub metadata: String,
}

// --- Vectors ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEmbedding {
    pub chunk_id: i64,
    pub context_id: i64,
    pub document_id: i64,
    pub embedding_model_id: i64,
    pub dim: i64,
    pub vector: Vec<f32>,
}

/// A vector loaded back for a brute-force similarity scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredVector {
    pub chunk_id: i64,
    pub document_id: i64,
    pub dim: i64,
    pub vector: Vec<f32>,
}

/// A scored retrieval hit returned by a cosine scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredChunk {
    pub chunk_id: i64,
    pub score: f32,
}

// --- Norm references (schema_v49, RETRIEVAL_QUALITY_PLAN.md AP2) -------------

/// One outgoing legal-norm reference of a chunk (a `(chunk_id, ref_key)` edge),
/// derived deterministically from `chunks.text` by `crate::refs::parse_refs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRef {
    pub id: i64,
    pub chunk_id: i64,
    pub context_id: i64,
    pub ref_key: String,
}

// --- Retrieval eval (schema_v47, RETRIEVAL_QUALITY_PLAN.md AP0) -------------

/// A named golden question-set for the LLM-free retrieval eval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalGoldenSet {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEvalGoldenSet {
    pub title: String,
    pub description: String,
}

/// One golden entry: a question + the anchor substrings that mark a chunk as
/// relevant (at least one must appear, case-insensitive/whitespace-collapsed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalGoldenEntry {
    pub id: i64,
    pub set_id: i64,
    pub entry_key: String,
    pub question: String,
    /// JSON array text as stored; parse with `anchors()`.
    pub anchors_any: String,
    pub note: String,
}

impl EvalGoldenEntry {
    /// The `anchors_any` JSON array, parsed (empty on malformed JSON).
    pub fn anchors(&self) -> Vec<String> {
        serde_json::from_str(&self.anchors_any).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEvalGoldenEntry {
    pub set_id: i64,
    pub entry_key: String,
    pub question: String,
    /// JSON array text (e.g. `["AT 4.1","Risikodeckungspotenzial"]`).
    pub anchors_any: String,
    pub note: String,
}

/// A single eval run over a golden set against a set of scoped contexts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalRun {
    pub id: i64,
    pub set_id: i64,
    /// JSON array of context ids.
    pub context_ids: String,
    /// JSON config: `{k, hybrid, follow_refs, rerank, top_k}`.
    pub config: String,
    pub status: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    /// JSON aggregate metrics (Hit@5/Hit@10/MRR + counts).
    pub metrics: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEvalRun {
    pub set_id: i64,
    pub context_ids: String,
    pub config: String,
}

/// Per-entry result of one eval run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalRunResult {
    pub id: i64,
    pub run_id: i64,
    pub entry_id: i64,
    pub entry_key: String,
    pub question: String,
    pub resolved_chunks: i64,
    pub first_rank: Option<i64>,
    pub hit5: bool,
    pub hit10: bool,
    pub skipped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEvalRunResult {
    pub run_id: i64,
    pub entry_id: i64,
    pub entry_key: String,
    pub question: String,
    pub resolved_chunks: i64,
    pub first_rank: Option<i64>,
    pub hit5: bool,
    pub hit10: bool,
    pub skipped: bool,
}

// --- Grid & async matrix-chat ----------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridSheet {
    pub id: i64,
    pub name: String,
    pub columns: String,
    pub row_count: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewGridSheet {
    pub name: String,
    pub columns: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridRow {
    pub id: i64,
    pub sheet_id: Option<i64>,
    pub source_chunk_id: Option<i64>,
    pub row_index: i64,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewGridRow {
    pub sheet_id: Option<i64>,
    pub source_chunk_id: Option<i64>,
    pub row_index: i64,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridChatResult {
    pub id: i64,
    pub run_id: String,
    pub row_uid: String,
    pub row_ref_type: RowRefType,
    pub row_ref_id: i64,
    pub prompt: String,
    pub columns_context: Option<String>,
    pub retrieved_refs: Option<String>,
    pub response: Option<String>,
    pub status: ChatStatus,
    pub error: Option<String>,
    pub prompt_snapshot: Option<String>,
    /// Source text of an upload row (`row_ref_type = GridRow`), so history
    /// loading can rebuild a synthetic `Chunk` without a real `chunks` row.
    /// `NULL` for chunk-backed rows (schema_v42).
    pub row_source_text: Option<String>,
    pub updated_at: i64,
}

/// Upsert payload for a grid chat cell (overwrite-per-row; chat is not history-aware).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridChatUpsert {
    pub run_id: String,
    pub row_uid: String,
    pub row_ref_type: RowRefType,
    pub row_ref_id: i64,
    pub prompt: String,
    pub columns_context: Option<String>,
    pub retrieved_refs: Option<String>,
    pub response: Option<String>,
    pub status: ChatStatus,
    pub error: Option<String>,
    pub prompt_snapshot: Option<String>,
    pub row_source_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridRun {
    pub run_id: String,
    pub prompt: String,
    pub updated_at: i64,
}

/// Per-run metadata stored once (not per row): the system prompt and the
/// grid-profile JSON schema the run was created with (`{mode,fields}` string,
/// `None` for plain-text profiles). Lets history loading gate row explosion on
/// the run's own mode instead of the currently selected profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridRunMeta {
    pub system_prompt: String,
    pub json_schema: Option<String>,
}

// --- Ontology (GraphRAG) ---------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyProfile {
    pub id: i64,
    pub name: String,
    pub entity_types_json: String,
    pub relation_types_json: String,
    pub extract_prompt: Option<String>,
    pub dedup_prompt: Option<String>,
    pub community_prompt: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOntologyProfile {
    pub name: String,
    pub entity_types_json: String,
    pub relation_types_json: String,
    pub extract_prompt: Option<String>,
    pub dedup_prompt: Option<String>,
    pub community_prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyNode {
    pub id: i64,
    pub context_id: i64,
    pub label: String,
    pub entity_type: String,
    /// The type the LLM actually produced at extraction time, kept permanently
    /// (never overwritten by lens materialization) — see
    /// `core/src/db/ontology/lenses.rs`/BACKLOG.md's "Lens" system.
    /// `#[serde(default)]` so a context bundle exported before this field
    /// existed still deserializes on import (falls back to `entity_type`'s
    /// value being re-derived at insert time, not this default directly —
    /// see `context_transfer::import`).
    #[serde(default)]
    pub raw_entity_type: String,
    pub description: String,
    /// Legacy single-valued community assignment. Since per-lens communities
    /// (schema_v37) this column is **no longer written** by the pipeline —
    /// membership lives in `ontology_community_members` (per lens, a node can
    /// be in different communities under different lenses). Kept readable for
    /// pre-v37 databases/bundles only.
    pub community_id: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOntologyNode {
    pub context_id: i64,
    pub label: String,
    pub entity_type: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyEdge {
    pub id: i64,
    pub context_id: i64,
    pub source_id: i64,
    pub target_id: i64,
    pub relation_type: String,
    /// The relation type the LLM actually produced at extraction time, kept
    /// permanently — see `OntologyNode::raw_entity_type`/BACKLOG.md's Lens
    /// system. `#[serde(default)]` for the same reason as
    /// `OntologyNode::raw_entity_type`.
    #[serde(default)]
    pub raw_relation_type: String,
    pub chunk_evidences: std::collections::HashMap<i64, Option<String>>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOntologyEdge {
    pub context_id: i64,
    pub source_id: i64,
    pub target_id: i64,
    pub relation_type: String,
    pub chunk_id: i64,
    pub evidence: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphContext {
    pub nodes: Vec<String>,
    pub edges: Vec<String>,
    pub community_summaries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyCommunity {
    pub id: i64,
    pub context_id: i64,
    pub community_label: String,
    pub node_count: i64,
    pub summary_text: String,
    /// Which lens this community was computed under; `None` = raw/unfiltered
    /// view (and all pre-v37 legacy rows). `#[serde(default)]` for bundles
    /// exported before per-lens communities existed.
    #[serde(default)]
    pub lens_id: Option<i64>,
    /// Summary-cache key: member node ids sorted ascending, comma-joined.
    /// `None` on legacy rows (they never cache-hit). Always derived from
    /// live node ids at (re)compute time — see schema_v37.sql.
    #[serde(default)]
    pub members_key: Option<String>,
    pub created_at: i64,
}

/// `OntologyCommunity` plus its member node ids — what the frontend needs to
/// build the node→community map for coloring/filtering (replaces reading the
/// legacy `ontology_nodes.community_id`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyCommunityWithMembers {
    pub id: i64,
    pub context_id: i64,
    pub community_label: String,
    pub node_count: i64,
    pub summary_text: String,
    pub lens_id: Option<i64>,
    pub members_key: Option<String>,
    pub created_at: i64,
    pub member_ids: Vec<i64>,
}

/// Materializes one `OntologyProfile`'s cosine-snap + relation-constraint
/// resolution against a context's permanently-kept raw types, without
/// mutating them (see `OntologyNode::raw_entity_type`/BACKLOG.md's Lens
/// system). A context's `active_lens_id` picks which lens (if any) filters/
/// relabels its graph for retrieval/dedup/community.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyLens {
    pub id: i64,
    pub context_id: i64,
    pub name: String,
    pub ontology_profile_id: i64,
    /// Whether this lens was ever materialized as part of an actual
    /// extraction run (as opposed to a standalone "Add Lens" re-labeling
    /// call against already-stored raw data) — purely informational, used
    /// only to warn before deleting it.
    pub is_extraction_lens: bool,
    pub created_at: i64,
}


#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct OntologyQuarantineChunk {
    pub chunk_id: i64,
    pub context_id: i64,
    pub graph_json: String,
    pub error_reason: String,
    pub created_at: String,
}

/// Non-blocking counterpart to `OntologyQuarantineChunk`: an edge whose
/// evidence hinted at a possible negation/polarity error, but stage-2 LLM
/// verification came back "unclear" or failed — see
/// `ontology/extract/verify.rs`. Doesn't block the pipeline, just flags the
/// edge for later human review.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OntologyEdgeReview {
    pub id: i64,
    pub context_id: i64,
    pub edge_id: i64,
    pub chunk_id: Option<i64>,
    pub relation_type: String,
    pub evidence: Option<String>,
    pub reason: String,
    pub created_at: i64,
    /// How many times verification has been attempted for this edge (0 for
    /// non-verification reviews). Bumped by `upsert_verification_failure`; once
    /// it reaches `MAX_VERIFY_ATTEMPTS` the review is auto-ejected. See
    /// `ontology/extract/verify.rs`.
    pub attempts: i64,
    /// Denormalized from the edge's source/target nodes at read time (see
    /// `list_ontology_edge_reviews`) so the UI viewer doesn't need a second
    /// round-trip to render the reviewed triplet.
    pub source_label: String,
    pub target_label: String,
}

/// One row of `ontology_merge_log` (schema_v38, see ISSUES.md
/// "ontology_dedup_cache — keine Merge-Historie ..."): records the losing
/// node's label/type right before `merge_ontology_nodes` hard-deletes it.
/// Purely additive/retrospective — never read by the extraction/dedup
/// pipeline itself, only for later retrieval (`list_ontology_merge_log`) and
/// `scripts/eval_extraction.py`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeLogEntry {
    pub id: i64,
    pub context_id: i64,
    pub winner_id: i64,
    pub loser_id: i64,
    pub loser_label: String,
    pub loser_entity_type: String,
    pub merged_at: i64,
}

/// One row of `ontology_run_log` (schema_v43): per-phase success/failure
/// counters that survive app restart, unlike the in-memory applog ring
/// buffer. Only the community-summarization phase writes rows for now (see
/// BACKLOG.md) — data source for the Ontology admin window's optional status
/// lines, never consulted by the pipeline itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntologyRunLogEntry {
    pub id: i64,
    pub run_id: String,
    pub context_id: i64,
    pub phase: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub attempted: i64,
    pub succeeded: i64,
    pub failed: i64,
    pub sample_error: Option<String>,
}
