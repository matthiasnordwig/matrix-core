//! Repeat keys: passages of mail attachments count, passages of stand-alone files never.

use super::repeats::{is_mail_attachment, repeat_key};
use crate::db::models::{NewChunk, NewDocument};
use crate::db::Database;

fn passage(text: &str) -> NewChunk {
    NewChunk {
        context_id: 0,
        document_id: 0,
        chunk_index: 0,
        char_start: 0,
        char_end: text.len() as i64,
        text: text.to_string(),
        signature: None,
        is_omitted: false,
        metadata: "{}".to_string(),
    }
}

const PAGE: &str = "Amendment to the master agreement. The parties agree that the schedule of fees is replaced in full by the schedule attached, effective on the first day of the next quarter. All other terms remain unchanged.";

#[test]
fn only_files_attached_to_a_mail_are_attachments() {
    assert!(is_mail_attachment("inbox/mail.eml › AmendABX1.doc"));
    assert!(is_mail_attachment("a.zip/x/1.MSG › report.pdf"));
    assert!(is_mail_attachment("mail.eml › inner.zip › deep.doc"));
    assert!(!is_mail_attachment("report.pdf"));
    assert!(!is_mail_attachment("mail.eml"));
    assert!(!is_mail_attachment("deck.pptx › image 3 (slide 2).png"));
    assert!(!is_mail_attachment("mails.zip/readme.txt"));
}

#[test]
fn the_same_attachment_in_two_mails_has_one_key_and_a_changed_text_another() {
    let a = repeat_key(&passage(PAGE), "box1/m1.eml › AmendABX1.doc").unwrap();
    let b = repeat_key(&passage(&PAGE.replace(", effective", ",\n   effective")), "box2/m9.eml › AmendABX1.doc").unwrap();
    assert_eq!(a, b);
    assert_ne!(a, repeat_key(&passage(&format!("{PAGE} Changed.")), "m1.eml › x.doc").unwrap());
}

#[test]
fn a_short_attachment_passage_has_no_key() {
    assert!(repeat_key(&passage("See attached."), "m1.eml › x.doc").is_none());
}

#[test]
fn stand_alone_files_never_have_a_key() {
    assert!(repeat_key(&passage(PAGE), "contract.pdf").is_none());
    assert!(repeat_key(&passage(PAGE), "folder/contract.docx").is_none());
}

#[test]
fn an_attachment_of_two_mails_makes_the_second_a_repeat_but_a_pdf_added_twice_stays_whole() {
    let db = Database::open_in_memory().unwrap();
    let (ctx, _) = crate::db::chunks::chunks_tests::seed(&db);
    let docs: Vec<i64> = (0..4)
        .map(|i| {
            db.create_document(&NewDocument {
                context_id: ctx, name: format!("d{i}"), zip_entry: None, byte_size: None, page_count: None, content_hash: None, extracted_text: None,
            })
            .unwrap()
            .id
        })
        .collect();
    let store = |doc: i64, source: &str| {
        let mut c = passage(PAGE);
        c.context_id = ctx;
        c.document_id = doc;
        db.create_chunk_once(&c, repeat_key(&c, source).as_deref()).unwrap()
    };
    let first = store(docs[0], "m1.eml › AmendABX1.doc");
    let second = store(docs[1], "m2.eml › AmendABX1.doc");
    assert!(!first.is_omitted && second.is_omitted);
    assert_eq!(db.chunk_duplicate_rows(ctx).unwrap().iter().find(|r| r.0 == second.id).unwrap().1, Some(first.id));
    assert!(!store(docs[2], "a.pdf").is_omitted);
    assert!(!store(docs[3], "b.pdf").is_omitted);
}
