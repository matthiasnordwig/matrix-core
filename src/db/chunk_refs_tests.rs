//! Tests for `chunk_refs` CRUD, derivation/rebuild idempotency, cascade,
//! resolution (definition-site heuristic), and the pure `expand_with_refs` caps.

use std::collections::HashMap;

use super::{expand_with_refs, pick_definition_site, ReferencedChunk};
use crate::db::models::*;
use crate::db::Database;

fn db() -> Database {
    Database::open_in_memory().expect("open in-memory db")
}

/// Minimal model→profile→context→document chain. Mirrors `tests::seed`.
fn seed(db: &Database) -> (i64, i64) {
    let model = db
        .create_embedding_model(&NewEmbeddingModel {
            identifier: "test-embed".into(),
            kind: ModelKind::LocalOnnx,
            model_path: Some("/m.onnx".into()),
            tokenizer_path: Some("/t.json".into()),
            api_config: None,
            execution_provider: Some(ExecutionProvider::Ane),
            is_matryoshka: false,
            native_dim: 4,
            default_dim: 4,
            normalize: true,
            tpm_limit: None,
            rpm_limit: None,
            max_concurrency: 1,
        })
        .unwrap();
    let ctx = db
        .create_context(&NewContext {
            name: "Ctx".into(),
            description: None,
            chunking_profile_id: None,
            embedding_model_id: Some(model.id),
            embedding_dim: Some(4),
            llm_id: None,
            fallback_llm_id: None,
            ontology_profile_id: None,
            ontology_pool_id: None,
            ontology_extract_llm_id: None,
            ontology_extract_pool_id: None,
            ontology_extract_reasoning_effort: None,
            extract_title_llm: false,
            auto_merge_ontology: false,
            chunking_strategy: "Semantic".into(),
            structural_profile_id: None,
        })
        .unwrap();
    let doc = db
        .create_document(&NewDocument {
            context_id: ctx.id,
            name: "d.pdf".into(),
            zip_entry: None,
            byte_size: None,
            page_count: None,
            content_hash: None,
            extracted_text: None,
        })
        .unwrap();
    (ctx.id, doc.id)
}

fn mk_chunk(db: &Database, ctx: i64, doc: i64, idx: i64, sig: Option<&str>, text: &str) -> i64 {
    db.create_chunk(&NewChunk {
        context_id: ctx,
        document_id: doc,
        chunk_index: idx,
        char_start: 0,
        char_end: text.len() as i64,
        text: text.into(),
        signature: sig.map(|s| s.into()),
        is_omitted: false,
        metadata: "{}".into(),
    })
    .unwrap()
    .id
}

#[test]
fn set_chunk_refs_roundtrip_and_idempotent() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let cid = mk_chunk(&db, ctx, doc, 0, None, "Nach § 25a KWG und Art. 28 DORA.");

    let n = db.set_chunk_refs(cid, ctx, "Nach § 25a KWG und Art. 28 DORA.").unwrap();
    assert_eq!(n, 2);
    let mut keys: Vec<String> = db.chunk_refs_for_chunk(cid).unwrap().into_iter().map(|r| r.ref_key).collect();
    keys.sort();
    assert_eq!(keys, vec!["DORA:Art.28", "KWG:§25a"]);

    // Re-running must not duplicate.
    db.set_chunk_refs(cid, ctx, "Nach § 25a KWG und Art. 28 DORA.").unwrap();
    assert_eq!(db.chunk_refs_for_chunk(cid).unwrap().len(), 2);
}

#[test]
fn rebuild_chunk_refs_is_idempotent() {
    let db = db();
    let (ctx, doc) = seed(&db);
    mk_chunk(&db, ctx, doc, 0, None, "§ 25a KWG");
    mk_chunk(&db, ctx, doc, 1, None, "AT 4.3.2 der MaRisk");
    mk_chunk(&db, ctx, doc, 2, None, "kein Verweis hier");

    let first = db.rebuild_chunk_refs(ctx).unwrap();
    assert_eq!(first, 2);
    let second = db.rebuild_chunk_refs(ctx).unwrap();
    assert_eq!(second, 2, "rebuild must be idempotent");

    // Total rows in the context = 2.
    let at = db.chunks_with_ref(&[ctx], "MARISK:AT4.3.2").unwrap();
    assert_eq!(at.len(), 1);
}

#[test]
fn cascade_delete_with_chunk() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let cid = mk_chunk(&db, ctx, doc, 0, None, "§ 6 GwG");
    db.set_chunk_refs(cid, ctx, "§ 6 GwG").unwrap();
    assert_eq!(db.chunk_refs_for_chunk(cid).unwrap().len(), 1);

    db.delete_chunk(cid).unwrap();
    assert!(db.chunk_refs_for_chunk(cid).unwrap().is_empty(), "refs cascade with chunk");
}

#[test]
fn resolve_prefers_definition_site_by_signature() {
    let db = db();
    let (ctx, doc) = seed(&db);
    // Mention chunk (mentions § 25a KWG in the middle of prose).
    let mention = mk_chunk(&db, ctx, doc, 0, Some("General remarks"), "Die Vorgaben aus § 25a KWG sind zu beachten.");
    // Definition site (signature carries § 25a; text starts with it).
    let def = mk_chunk(&db, ctx, doc, 1, Some("§ 25a KWG — Besondere organisatorische Pflichten"), "§ 25a KWG regelt die Geschäftsorganisation ...");
    db.set_chunk_refs(mention, ctx, "Die Vorgaben aus § 25a KWG sind zu beachten.").unwrap();
    db.set_chunk_refs(def, ctx, "§ 25a KWG regelt die Geschäftsorganisation ...").unwrap();

    let target = db.resolve_ref_target(&[ctx], "KWG:§25a").unwrap().unwrap();
    assert_eq!(target.id, def, "definition site (signature) should win over a mention");
}

#[test]
fn resolve_none_when_unknown_ref() {
    let db = db();
    let (ctx, doc) = seed(&db);
    let cid = mk_chunk(&db, ctx, doc, 0, None, "§ 25a KWG");
    db.set_chunk_refs(cid, ctx, "§ 25a KWG").unwrap();
    assert!(db.resolve_ref_target(&[ctx], "GWG:§6").unwrap().is_none());
}

// --- EU-bound article resolution (EU:YYYY/NNNN:Art.X) ---

fn mk_doc(db: &Database, ctx: i64, name: &str) -> i64 {
    db.create_document(&NewDocument {
        context_id: ctx,
        name: name.into(),
        zip_entry: None,
        byte_size: None,
        page_count: None,
        content_hash: None,
        extracted_text: None,
    })
    .unwrap()
    .id
}

/// Create a chunk AND derive its refs (mirrors ingest+rebuild).
fn mk_chunk_reffed(db: &Database, ctx: i64, doc: i64, idx: i64, sig: Option<&str>, text: &str) -> i64 {
    let id = mk_chunk(db, ctx, doc, idx, sig, text);
    db.set_chunk_refs(id, ctx, text).unwrap();
    id
}

#[test]
fn resolve_eu_bound_article_to_regulation_def_chunk() {
    let db = db();
    let (ctx, law_doc) = seed(&db);
    // The citing law: a chunk with the EU long form — it CARRIES the compound
    // key EU:2013/575:Art.395 (mention candidate).
    let citing = mk_chunk_reffed(
        &db, ctx, law_doc, 0, None,
        "Es gelten die Artikel 387 bis 410 der Verordnung (EU) Nr. 575/2013 entsprechend.",
    );
    // The regulation document itself: early chunk carries the base EU ref
    // (title page), a later chunk IS the article definition (text starts with
    // "Artikel 395" — inside the regulation there is no Kürzel).
    let reg_doc = mk_doc(&db, ctx, "crr.pdf");
    mk_chunk_reffed(
        &db, ctx, reg_doc, 0, None,
        "Verordnung (EU) Nr. 575/2013 des Europäischen Parlaments und des Rates",
    );
    let def = mk_chunk_reffed(
        &db, ctx, reg_doc, 7, None,
        "Artikel 395 Obergrenze für Großkredite (1) Ein Institut darf ... 25 % ...",
    );
    // Prefix-collision guard inside the regulation: Artikel 395a must not win.
    mk_chunk_reffed(&db, ctx, reg_doc, 8, None, "Artikel 395a Sonderfall ...");

    let target = db.resolve_ref_target(&[ctx], "EU:2013/575:Art.395").unwrap().unwrap();
    assert_eq!(
        target.id, def,
        "EU-bound article must resolve to the regulation's own definition chunk"
    );
    assert_ne!(target.id, citing, "the citing chunk is a mention, not the definition");
}

#[test]
fn resolve_eu_bound_article_mention_only_falls_back_to_carrier() {
    let db = db();
    let (ctx, law_doc) = seed(&db);
    // Only a citing chunk exists (regulation not ingested): the compound-key
    // carrier itself is the best available target.
    let citing = mk_chunk_reffed(
        &db, ctx, law_doc, 0, None,
        "Nach Artikel 92 der Verordnung (EU) Nr. 575/2013 gilt ...",
    );
    let target = db.resolve_ref_target(&[ctx], "EU:2013/575:Art.92").unwrap().unwrap();
    assert_eq!(target.id, citing);
}

#[test]
fn resolve_eu_bound_article_ignores_mere_citing_documents() {
    let db = db();
    let (ctx, law_doc) = seed(&db);
    // A law document that cites the regulation ONLY in its body (chunk_index
    // > 2) is not "the regulation itself": its "Artikel 92 …"-starting chunk
    // must NOT be offered as the definition site.
    mk_chunk_reffed(&db, ctx, law_doc, 0, None, "Kreditwesengesetz — Inhaltsübersicht");
    mk_chunk_reffed(
        &db, ctx, law_doc, 5, None,
        "Begriffe im Sinne der Verordnung (EU) Nr. 575/2013 sind ...",
    );
    mk_chunk_reffed(&db, ctx, law_doc, 6, None, "Artikel 92 findet keine Anwendung auf ...");
    // No chunk carries the compound key, and no document qualifies as the
    // regulation → the ref does not resolve at all (precision over recall).
    assert!(db.resolve_ref_target(&[ctx], "EU:2013/575:Art.92").unwrap().is_none());
}

// --- pure pick_definition_site ---

fn chunk(id: i64, sig: Option<&str>, text: &str) -> Chunk {
    Chunk {
        id,
        context_id: 1,
        document_id: 1,
        chunk_index: id,
        char_start: 0,
        char_end: 0,
        text: text.into(),
        signature: sig.map(|s| s.into()),
        is_omitted: false,
        metadata: "{}".into(),
        created_at: 0,
    }
}

#[test]
fn pick_definition_site_prefers_signature_then_density() {
    let candidates = vec![
        chunk(1, Some("General"), "mentions § 25a KWG mid-text"),
        chunk(2, Some("§ 25a KWG — Pflichten"), "§ 25a KWG regelt ..."),
    ];
    let mut density = HashMap::new();
    density.insert(1, 3);
    density.insert(2, 1);
    // Chunk 2 is the definition site (signature carries surface) → wins despite
    // lower density.
    let picked = pick_definition_site(candidates, "KWG:§25a", &density);
    assert_eq!(picked.id, 2);
}

#[test]
fn pick_definition_site_falls_back_to_density_then_earliest() {
    // Neither is a definition site → most ref-dense wins.
    let candidates = vec![chunk(1, None, "x § 25a KWG"), chunk(2, None, "y § 25a KWG")];
    let mut density = HashMap::new();
    density.insert(1, 1);
    density.insert(2, 5);
    let picked = pick_definition_site(candidates, "KWG:§25a", &density);
    assert_eq!(picked.id, 2);

    // Equal density → earliest (first in the ordered candidate list) wins.
    let candidates = vec![chunk(3, None, "a"), chunk(4, None, "b")];
    let mut d2 = HashMap::new();
    d2.insert(3, 2);
    d2.insert(4, 2);
    assert_eq!(pick_definition_site(candidates, "KWG:§25a", &d2).id, 3);
}

#[test]
fn pick_definition_site_word_boundary_no_prefix_collision() {
    // A `§ 25a` signature must NOT satisfy a `KWG:§25` ref (prefix collision).
    // Candidate 1 is a §25a definition (should NOT count as §25's def-site);
    // candidate 2 is the real §25 mention. §25's target must be candidate 2.
    let candidates = vec![
        chunk(1, Some("§ 25a KWG — Besondere Pflichten"), "§ 25a KWG regelt ..."),
        chunk(2, Some("§ 25 KWG — Meldungen"), "§ 25 KWG regelt ..."),
    ];
    let mut density = HashMap::new();
    density.insert(1, 9); // denser, but not a def-site for §25 (prefix collision)
    density.insert(2, 1);
    let picked = pick_definition_site(candidates, "KWG:§25", &density);
    assert_eq!(picked.id, 2, "§25 must not resolve to the §25a definition chunk");

    // MaRisk analogue: `AT 4.3` must not match `AT 4.3.2`.
    let cands = vec![
        chunk(3, Some("AT 4.3.2 Datenmanagement"), "AT 4.3.2 ..."),
        chunk(4, Some("AT 4.3 Besondere Anforderungen"), "AT 4.3 ..."),
    ];
    let mut d = HashMap::new();
    d.insert(3, 9);
    d.insert(4, 1);
    assert_eq!(pick_definition_site(cands, "MARISK:AT4.3", &d).id, 4);
}

#[test]
fn pick_definition_site_kuerzel_must_match() {
    // A `§ 25a VAG` signature must NOT satisfy a `KWG:§25a` ref — different act.
    // Candidate 1 is the VAG def (wrong law, denser); candidate 2 the real KWG
    // definition site. The KWG ref must resolve to candidate 2 despite lower
    // density, because the VAG chunk is disqualified by the Kürzel conflict.
    let candidates = vec![
        chunk(1, Some("§ 25a VAG — Geschäftsorganisation"), "§ 25a VAG regelt ..."),
        chunk(2, Some("§ 25a KWG — Besondere organisatorische Pflichten"), "§ 25a KWG regelt ..."),
    ];
    let mut density = HashMap::new();
    density.insert(1, 9);
    density.insert(2, 1);
    let picked = pick_definition_site(candidates, "KWG:§25a", &density);
    assert_eq!(picked.id, 2, "wrong-Kürzel (VAG) def must not win for a KWG ref");

    // A chunk of the same law WITHOUT the Kürzel in its signature stays a valid
    // def-site (the common in-document case): `§ 25a` alone satisfies `KWG:§25a`.
    let cands = vec![
        chunk(3, None, "irrelevant mention § 25a"),
        chunk(4, Some("§ 25a Besondere organisatorische Pflichten"), "§ 25a regelt ..."),
    ];
    let mut d = HashMap::new();
    d.insert(3, 5);
    d.insert(4, 1);
    assert_eq!(
        pick_definition_site(cands, "KWG:§25a", &d).id,
        4,
        "same-law chunk without explicit Kürzel is still the def-site"
    );
}

#[test]
fn pick_definition_site_eu_legacy_surface() {
    // Ref key `EU:2013/575` must find the legacy prose form "Nr. 575/2013".
    let candidates = vec![
        chunk(1, None, "eine allgemeine Erwähnung ohne Nummer"),
        chunk(2, Some("Verordnung (EU) Nr. 575/2013 (CRR)"), "Verordnung (EU) Nr. 575/2013 ..."),
    ];
    let mut density = HashMap::new();
    density.insert(1, 5);
    density.insert(2, 1);
    let picked = pick_definition_site(candidates, "EU:2013/575", &density);
    assert_eq!(picked.id, 2, "EU legacy order 575/2013 must be recognized as def-site");
}

// --- pure expand_with_refs caps ---

#[test]
fn expand_caps_one_per_hit_and_total() {
    // top_k = 5 → total cap ⌈5/2⌉ = 3. Primary hits [10,11,12,13,14].
    let primary = vec![10, 11, 12, 13, 14];
    let resolved = vec![
        vec![100],      // hit 1 → 100
        vec![101, 102], // hit 2 → 101 (only one per hit)
        vec![103],      // hit 3 → 103
        vec![104],      // hit 4 → would be #4, over the total cap of 3
        vec![105],
    ];
    let out = expand_with_refs(&primary, &resolved, 5);
    assert_eq!(
        out,
        vec![
            ReferencedChunk { chunk_id: 100, referenced_by: 1 },
            ReferencedChunk { chunk_id: 101, referenced_by: 2 },
            ReferencedChunk { chunk_id: 103, referenced_by: 3 },
        ]
    );
}

#[test]
fn expand_never_readds_primary_or_duplicate() {
    let primary = vec![10, 11];
    let resolved = vec![
        vec![11], // resolves to a primary hit → skipped
        vec![10, 200], // 10 is primary → take 200
    ];
    let out = expand_with_refs(&primary, &resolved, 4);
    assert_eq!(out, vec![ReferencedChunk { chunk_id: 200, referenced_by: 2 }]);

    // Same target referenced by two hits → added once.
    let resolved2 = vec![vec![300], vec![300]];
    let out2 = expand_with_refs(&[1, 2], &resolved2, 4);
    assert_eq!(out2, vec![ReferencedChunk { chunk_id: 300, referenced_by: 1 }]);
}
