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
    pub created_at: i64,
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
}

// --- Profiles & Contexts ---------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingProfile {
    pub id: i64,
    pub name: String,
    pub prompt: String,
    pub overlap_ratio: f64,
    pub max_signature_len: i64,
    pub llm_endpoint_id: Option<i64>,
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
    pub llm_endpoint_id: Option<i64>,
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
    pub chunk_endpoint_id: Option<i64>,
    pub extract_title_llm: bool,
    pub auto_merge_ontology: bool,
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
    pub chunk_endpoint_id: Option<i64>,
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
    pub row_ref_type: RowRefType,
    pub row_ref_id: i64,
    pub prompt: String,
    pub columns_context: Option<String>,
    pub retrieved_refs: Option<String>,
    pub response: Option<String>,
    pub status: ChatStatus,
    pub error: Option<String>,
    pub prompt_snapshot: Option<String>,
    pub updated_at: i64,
}

/// Upsert payload for a grid chat cell (overwrite-per-row; chat is not history-aware).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridChatUpsert {
    pub run_id: String,
    pub row_ref_type: RowRefType,
    pub row_ref_id: i64,
    pub prompt: String,
    pub columns_context: Option<String>,
    pub retrieved_refs: Option<String>,
    pub response: Option<String>,
    pub status: ChatStatus,
    pub error: Option<String>,
    pub prompt_snapshot: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridRun {
    pub run_id: String,
    pub prompt: String,
    pub updated_at: i64,
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
    pub description: String,
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
    pub chunk_id: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOntologyEdge {
    pub context_id: i64,
    pub source_id: i64,
    pub target_id: i64,
    pub relation_type: String,
    pub chunk_id: i64,
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
    pub created_at: i64,
}

