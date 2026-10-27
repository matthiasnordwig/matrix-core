//! Hybrid fusion across languages (`fusion.rs`, „Across languages"): the keyword list of a
//! context drops out when the question's language differs from that of most of its best vector
//! hits, a per-entry factor in the traced fusion, and the pure rule behind both.

use std::collections::HashMap;

use super::fusion::{cross_language_factor, FusionParams};
use super::provenance::{rrf_fuse_traced, rrf_fuse_traced_with, ListKind};
use super::tests::model;
use crate::db::models::*;
use crate::Database;

#[test]
fn keyword_factor_applies_when_most_top_hits_are_in_another_language() {
    let de = Some("de");
    let en = Some("en");
    assert_eq!(cross_language_factor("en", &[de, de, en], 0.0), 0.0);
    assert_eq!(cross_language_factor("en", &[de, en], 0.0), 1.0, "half is not most");
    assert_eq!(cross_language_factor("de", &[de, de, en], 0.0), 1.0);
    // Unknown languages do not count either way; nothing known keeps the list.
    assert_eq!(cross_language_factor("en", &[None, None, de], 0.25), 0.25);
    assert_eq!(cross_language_factor("en", &[None, None], 0.0), 1.0);
    assert_eq!(cross_language_factor("en", &[], 0.0), 1.0);
}

#[test]
fn per_entry_factor_scales_only_that_share() {
    let v = vec![1, 2, 3];
    let i = vec![3, 1];
    let lists = [(Some(ListKind::Vector), v.as_slice(), 1.0), (Some(ListKind::Image), i.as_slice(), 1.0)];
    let bits = |x: &[ScoredChunk]| x.iter().map(|s| (s.chunk_id, s.score.to_bits())).collect::<Vec<_>>();
    // Factor 1 everywhere is the plain traced fusion, bit for bit.
    assert_eq!(bits(&rrf_fuse_traced_with(&lists, 10, 10, |_, _| 1.0)), bits(&rrf_fuse_traced(&lists, 10, 10)));
    let got = rrf_fuse_traced_with(&lists, 10, 10, |kind, id| if kind == Some(ListKind::Image) && id == 3 { 0.5 } else { 1.0 });
    let c3 = got.iter().find(|s| s.chunk_id == 3).unwrap();
    assert_eq!(c3.provenance.image.unwrap().share, 0.5 / 11.0);
    assert_eq!(c3.score, 1.0 / 13.0 + 0.5 / 11.0);
    let c1 = got.iter().find(|s| s.chunk_id == 1).unwrap();
    assert_eq!(c1.provenance.image.unwrap().share, 1.0 / 12.0);
}

const DE: &[&str] = &[
    "Der Anbieter hält die technische Dokumentation für einen Zeitraum von zehn Jahren für die Behörden bereit, und sie wird auf Verlangen vorgelegt.",
    "Die Aufbewahrung der Unterlagen ist Pflicht des Anbieters, der die Dokumentation mit den Protokollen für die Aufsicht bereithält.",
    "Die Behörde kann von dem Anbieter die Unterlagen verlangen, die für die Prüfung der Konformität erforderlich sind.",
];
const EN: &[&str] = &[
    "The report describes how the provider of the service shall keep the technical documentation of the incident for the analysis.",
    "This section of the report is about documentation that the provider keeps for internal use and which is shared with the team.",
];

/// One context: German passages close to the query vector (ranks 1–3), English passages far from
/// it but full of the question's words. Returns (model id, context id, German ids, English ids).
fn mixed(db: &Database) -> (i64, i64, Vec<i64>, Vec<i64>) {
    let m = model(db, "gemma", 3).id;
    let ctx = db
        .create_context(&NewContext {
            name: "Gemischt".into(),
            description: None,
            chunking_strategy: "Semantic".into(),
            chunking_profile_id: None,
            structural_profile_id: None,
            embedding_model_id: Some(m),
            embedding_dim: Some(3),
            llm_id: None,
            fallback_llm_id: None,
            ontology_profile_id: None,
            ontology_pool_id: None,
            ontology_extract_llm_id: None,
            ontology_extract_pool_id: None,
            ontology_extract_reasoning_effort: None,
            extract_title_llm: false,
            auto_merge_ontology: false,
            image_llm_id: None,
        })
        .unwrap()
        .id;
    let doc = db
        .create_document(&NewDocument {
            context_id: ctx,
            name: "d.pdf".into(),
            zip_entry: None,
            byte_size: None,
            page_count: None,
            content_hash: None,
            extracted_text: None,
        })
        .unwrap()
        .id;
    let vectors = [[1.0, 0.0, 0.0], [0.9, 0.3, 0.0], [0.8, 0.5, 0.0], [0.1, 1.0, 0.0], [0.0, 1.0, 0.2]];
    let ids: Vec<i64> = DE
        .iter()
        .chain(EN)
        .zip(vectors)
        .enumerate()
        .map(|(i, (text, v))| {
            let c = db
                .create_chunk(&NewChunk {
                    context_id: ctx,
                    document_id: doc,
                    chunk_index: i as i64,
                    char_start: 0,
                    char_end: 1,
                    text: (*text).into(),
                    signature: None,
                    is_omitted: false,
                    metadata: "{}".into(),
                })
                .unwrap();
            db.insert_embedding(&NewEmbedding {
                chunk_id: c.id,
                context_id: ctx,
                document_id: doc,
                embedding_model_id: m,
                dim: 3,
                vector: v.to_vec(),
            })
            .unwrap();
            c.id
        })
        .collect();
    (m, ctx, ids[..3].to_vec(), ids[3..].to_vec())
}

#[test]
fn a_question_in_another_language_than_the_vector_hits_ignores_the_keyword_list() {
    let db = Database::open_in_memory().unwrap();
    let (m, ctx, de, en) = mixed(&db);
    let q = HashMap::from([(m, vec![1.0, 0.0, 0.0])]);
    let ask = |raw: &str, f: &FusionParams| db.retrieve_hybrid_with_params(&[ctx], &q, raw, 5, 5, None, f).unwrap();
    let english = "How long must the provider keep the technical documentation?";
    let off = FusionParams { cross_lang_fts: 1.0, ..FusionParams::default() };
    // Without the rule an English passage (keyword rank 1, vector rank 4) beats the German answer.
    assert!(en.contains(&ask(english, &off)[0].chunk_id));
    // With it the German vector hit leads and no hit carries a keyword share.
    let got = ask(english, &FusionParams::default());
    assert_eq!(got[0].chunk_id, de[0]);
    assert!(got.iter().all(|s| s.provenance.keyword.map_or(true, |k| k.share == 0.0)));
    // A German question on German vector hits keeps its keyword list, bit for bit.
    let german = "Wie lange muss der Anbieter die technische Dokumentation aufbewahren?";
    let bits = |x: &[ScoredChunk]| x.iter().map(|s| (s.chunk_id, s.score.to_bits())).collect::<Vec<_>>();
    assert_eq!(bits(&ask(german, &FusionParams::default())), bits(&ask(german, &off)));
    // A question of unknown language (no function word) keeps it, too.
    assert_eq!(bits(&ask("provider documentation", &FusionParams::default())), bits(&ask("provider documentation", &off)));
}
