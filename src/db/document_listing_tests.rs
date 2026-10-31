//! Reads over a context's documents that stay cheap in a large collection: ids and counts without
//! the texts, the documents added after a known id, a thinned selection of texts.

use super::models::{NewContext, NewDocument};
use super::Database;

fn context_with_docs(db: &Database, name: &str, texts: &[&str]) -> (i64, Vec<i64>) {
    let c = db.create_context(&NewContext { name: name.into(), chunking_strategy: "structural".into(), ..Default::default() }).unwrap();
    let ids = texts
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let doc = NewDocument {
                context_id: c.id,
                name: format!("d{i}"),
                zip_entry: None,
                byte_size: None,
                page_count: None,
                content_hash: None,
                extracted_text: Some((*t).into()),
            };
            db.create_document(&doc).unwrap().id
        })
        .collect();
    (c.id, ids)
}

#[test]
fn counts_and_highest_id_stay_within_the_context() {
    let db = Database::open_in_memory().unwrap();
    let (a, ids) = context_with_docs(&db, "a", &["x", "y"]);
    let (b, _) = context_with_docs(&db, "b", &["z"]);
    let (empty, _) = context_with_docs(&db, "c", &[]);
    assert_eq!((db.count_documents(a).unwrap(), db.count_documents(b).unwrap()), (2, 1));
    assert_eq!(db.max_document_id(a).unwrap(), ids[1]);
    assert_eq!((db.count_documents(empty).unwrap(), db.max_document_id(empty).unwrap()), (0, 0));
}

#[test]
fn documents_after_returns_only_the_new_ones_by_name() {
    let db = Database::open_in_memory().unwrap();
    let (c, ids) = context_with_docs(&db, "a", &["old", "new", "newer"]);
    let (_, _) = context_with_docs(&db, "other", &["foreign"]);
    let new: Vec<_> = db.documents_after(c, ids[0]).unwrap().into_iter().map(|d| d.extracted_text.unwrap()).collect();
    assert_eq!(new, ["new", "newer"]);
}

#[test]
fn text_sample_is_complete_up_to_the_limit_and_thinned_above() {
    let db = Database::open_in_memory().unwrap();
    let texts: Vec<String> = (0..100).map(|i| format!("t{i}")).collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let (c, _) = context_with_docs(&db, "a", &refs);
    assert_eq!(db.document_texts_sample(c, 100).unwrap().len(), 100);
    assert_eq!(db.document_texts_sample(c, 1000).unwrap().len(), 100);
    let thinned = db.document_texts_sample(c, 10).unwrap().len();
    assert!((9..=11).contains(&thinned), "{thinned}");
}
