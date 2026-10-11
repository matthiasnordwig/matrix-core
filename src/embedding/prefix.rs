//! Task prefixes for asymmetric embedding models (EmbeddingGemma:
//! `task: search result | query: ` / `title: none | text: `).
//!
//! `EmbeddingModel::query_prefix` is prepended to every text embedded as a
//! query (retrieval, eval, tools, attribution, ontology semantic search),
//! `document_prefix` to every text embedded as a stored vector (chunks,
//! ontology nodes/types/communities). This is the ONE place that applies them —
//! the embedders themselves (`onnx.rs`, `gguf_embed.rs`, remote API) always
//! embed the text they are given verbatim. None or an empty prefix returns the
//! input unchanged (borrowed), i.e. exactly the pre-prefix behaviour.

use std::borrow::Cow;

use crate::db::models::EmbeddingModel;

/// Which side of an asymmetric retrieval pair a text is embedded for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedRole {
    /// Compared against stored vectors at search time.
    Query,
    /// Stored in the index (chunk text, ontology node/type/community text).
    Document,
}

/// The model's prefix for `role`, if set and non-empty.
pub fn task_prefix(model: &EmbeddingModel, role: EmbedRole) -> Option<&str> {
    let p = match role {
        EmbedRole::Query => model.query_prefix.as_deref(),
        EmbedRole::Document => model.document_prefix.as_deref(),
    };
    p.filter(|s| !s.is_empty())
}

/// `text` with the model's task prefix for `role` prepended (verbatim — no
/// separator is inserted; the prefix carries its own trailing space).
pub fn apply_task_prefix<'a>(model: &EmbeddingModel, role: EmbedRole, text: &'a str) -> Cow<'a, str> {
    match task_prefix(model, role) {
        Some(p) => Cow::Owned(format!("{p}{text}")),
        None => Cow::Borrowed(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::ModelKind;

    fn model(q: Option<&str>, d: Option<&str>) -> EmbeddingModel {
        EmbeddingModel {
            id: 1,
            identifier: "m".into(),
            kind: ModelKind::LocalOnnx,
            model_path: None,
            tokenizer_path: None,
            api_config: None,
            execution_provider: None,
            is_matryoshka: false,
            native_dim: 768,
            default_dim: 768,
            normalize: true,
            tpm_limit: None,
            rpm_limit: None,
            max_concurrency: 1,
            query_prefix: q.map(Into::into),
            document_prefix: d.map(Into::into),
            created_at: 0,
        }
    }

    #[test]
    fn none_and_empty_leave_text_unchanged_and_borrowed() {
        for m in [model(None, None), model(Some(""), Some(""))] {
            for role in [EmbedRole::Query, EmbedRole::Document] {
                let out = apply_task_prefix(&m, role, "Rechnung");
                assert!(matches!(out, Cow::Borrowed("Rechnung")));
                assert_eq!(task_prefix(&m, role), None);
            }
        }
    }

    #[test]
    fn role_selects_its_own_prefix() {
        let m = model(Some("task: search result | query: "), Some("title: none | text: "));
        assert_eq!(
            apply_task_prefix(&m, EmbedRole::Query, "Frist?"),
            "task: search result | query: Frist?"
        );
        assert_eq!(
            apply_task_prefix(&m, EmbedRole::Document, "Rechnungen sind fällig."),
            "title: none | text: Rechnungen sind fällig."
        );
    }

    #[test]
    fn only_one_side_set() {
        let m = model(Some("q: "), None);
        assert_eq!(apply_task_prefix(&m, EmbedRole::Query, "x"), "q: x");
        assert_eq!(apply_task_prefix(&m, EmbedRole::Document, "x"), "x");
    }
}
