//! Row types of a grid run's history entry (`grid_chat_results` grouped by
//! run, plus its once-per-run `grid_run_meta` row). Split from `models.rs`
//! (which re-exports them) to keep that file under its size cap.

use serde::{Deserialize, Serialize};

/// How a run was started (schema_v66): model selection, agentic budget,
/// template, context mode. Every field is `None` for runs created before v66;
/// serialized flat into `GridRun`/`GridRunMeta`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GridRunInfo {
    #[serde(default)]
    pub llm_endpoint_id: Option<i64>,
    #[serde(default)]
    pub pool_id: Option<i64>,
    /// Endpoint/pool name at run time — survives deleting the endpoint.
    #[serde(default)]
    pub model_label: Option<String>,
    /// 0 = classic retrieval, > 0 = agentic tool loop with this budget.
    #[serde(default)]
    pub tool_call_budget: Option<i64>,
    #[serde(default)]
    pub template_name: Option<String>,
    #[serde(default)]
    pub context_mode: Option<String>,
}

/// One history entry: a run's first prompt, last update and start settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridRun {
    pub run_id: String,
    pub prompt: String,
    pub updated_at: i64,
    #[serde(flatten)]
    pub info: GridRunInfo,
}

/// Per-run metadata stored once (not per row): the system prompt and the
/// grid-profile JSON schema the run was created with (`{mode,fields}` string,
/// `None` for plain-text profiles). Lets history loading gate row explosion on
/// the run's own mode instead of the currently selected profile; `info` lets
/// it restore the model selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridRunMeta {
    pub system_prompt: String,
    pub json_schema: Option<String>,
    #[serde(flatten)]
    pub info: GridRunInfo,
}
