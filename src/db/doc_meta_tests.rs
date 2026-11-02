//! Search filter by sender, date and folder (`doc_meta.rs`, schema_v75): reading the mail header
//! block, date bounds, the SQL conditions, attachments inheriting their mail's facts, the scope a
//! filter leaves, the covering index, the folder of a file on disk, and the backfill of an
//! existing v74 database.

use super::doc_meta::{date_bound, filter_sql, folder_of, is_mail_name, like_pattern, mail_header, MailMetaTracker};
use super::models::*;
use super::vision_tests::migrate_to;
use super::Database;

const MAIL: &str = "# Gas prices\n\n**From:** Kenneth Lay <Kenneth.Lay@enron.com>  \n**To:** Jeff Skilling  \n**Date:** 2001-10-05 14:30 -07:00  \n**Folder:** Sent\n\nBody.\n\n**From:** someone quoted\n**Date:** 1999-01-01 00:00 Z";

#[test]
fn the_header_block_gives_sender_and_date_and_the_body_does_not() {
    assert_eq!(mail_header(MAIL), (Some("kenneth lay <kenneth.lay@enron.com>".into()), Some("2001-10-05".into())));
    // No subject heading, no date line.
    assert_eq!(mail_header("**From:** A\n\ntext"), (Some("a".into()), None));
    // A date the reader could not normalize is no date.
    assert_eq!(mail_header("# s\n\n**Date:** Mon, 5 Oct"), (None, None));
    // Bold lines further down (a forwarded HTML mail) are not the header.
    assert_eq!(mail_header("# s\n\nHello\n\n**From:** B\n**Date:** 2001-01-01 10:00 Z"), (None, None));
}

#[test]
fn dates_widen_to_full_bounds_and_bad_ones_are_named() {
    assert_eq!(date_bound("2001", false).unwrap(), "2001-01-01");
    assert_eq!(date_bound("2001", true).unwrap(), "2001-12-31");
    assert_eq!(date_bound("2001-09", false).unwrap(), "2001-09-01");
    assert_eq!(date_bound("2001-11", true).unwrap(), "2001-11-31");
    assert_eq!(date_bound(" 2001-10-05 ", true).unwrap(), "2001-10-05");
    for bad in ["01", "2001-13", "2001-1-5", "2001-10-05-1", "autumn", "2001-00"] {
        assert!(date_bound(bad, false).unwrap_err().contains(bad.trim()), "{bad}");
    }
}

#[test]
fn a_filter_is_normalized_and_a_reversed_range_refused() {
    let f = DocFilter { sender: Some("  Lay ".into()), date_from: Some("2001-09".into()), date_to: Some("2001-11".into()), folder: Some("Lay-K\\Sent".into()) };
    assert_eq!(
        f.normalized().unwrap(),
        DocFilter { sender: Some("lay".into()), date_from: Some("2001-09-01".into()), date_to: Some("2001-11-31".into()), folder: Some("lay-k/sent".into()) }
    );
    assert!(DocFilter { sender: Some(" ".into()), ..Default::default() }.is_empty());
    let reversed = DocFilter { date_from: Some("2002".into()), date_to: Some("2001".into()), ..Default::default() };
    assert!(reversed.normalized().unwrap_err().contains("after"));
}

#[test]
fn conditions_follow_the_set_fields_and_escape_like_marks() {
    assert_eq!(like_pattern("50%_a\\b"), "%50\\%\\_a\\\\b%");
    let f = DocFilter { sender: Some("lay".into()), date_to: Some("2001-12-31".into()), ..Default::default() };
    let (sql, args) = filter_sql(&f);
    assert_eq!(sql, "doc_date <= ? AND sender LIKE ? ESCAPE '\\'");
    assert_eq!(args, vec!["2001-12-31".to_string(), "%lay%".into()]);
    assert_eq!(filter_sql(&DocFilter::default()).0, "");
}

#[test]
fn folders_and_mail_names_come_from_the_path() {
    assert_eq!(folder_of("maildir/Lay-K/inbox/1.eml"), Some("maildir/lay-k/inbox".into()));
    assert_eq!(folder_of("a\\b\\c.txt"), Some("a/b".into()));
    assert_eq!(folder_of("dir/mail.eml › x/report.pdf"), Some("dir".into()));
    assert_eq!(folder_of("report.pdf"), None);
    assert!(is_mail_name("x/1.EML") && is_mail_name("a.eml › fwd.msg"));
    assert!(!is_mail_name("a.eml › report.pdf"));
}

#[test]
fn attachments_carry_their_mails_facts_and_siblings_do_not() {
    let mut t = MailMetaTracker::default();
    let mail = t.next("in/a.eml", Some(MAIL));
    assert_eq!(mail.sender.as_deref(), Some("kenneth lay <kenneth.lay@enron.com>"));
    let att = t.next("in/a.eml › report.pdf", None);
    assert_eq!((att.sender, att.date, att.folder), (mail.sender.clone(), mail.date.clone(), Some("in".into())));
    // A mail attached to the mail has its own facts, its attachment takes them.
    let inner = t.next("in/a.eml › fwd.eml", Some("**From:** B\n**Date:** 2000-02-02 10:00 Z"));
    assert_eq!(inner.sender.as_deref(), Some("b"));
    assert_eq!(t.next("in/a.eml › fwd.eml › x.pdf", None).date.as_deref(), Some("2000-02-02"));
    assert_eq!(t.next("in/a.eml › later.pdf", None).sender, mail.sender);
    // The next file of the upload is not an attachment.
    assert_eq!(t.next("in/b.pdf", None), DocMeta { folder: Some("in".into()), ..Default::default() });
}

fn context(db: &Database, name: &str) -> i64 {
    db.create_context(&NewContext { name: name.into(), chunking_strategy: "structural".into(), ..Default::default() }).unwrap().id
}

fn doc(db: &Database, ctx: i64, name: &str, m: DocMeta) -> i64 {
    let id = db
        .create_document(&NewDocument { context_id: ctx, name: name.into(), zip_entry: None, byte_size: None, page_count: None, content_hash: None, extracted_text: Some("x".into()) })
        .unwrap()
        .id;
    db.set_document_meta(id, &m).unwrap();
    id
}

fn meta(sender: &str, date: &str, folder: &str) -> DocMeta {
    DocMeta { sender: Some(sender.into()), date: Some(date.into()), folder: Some(folder.into()) }
}

#[test]
fn a_filter_narrows_the_scope_within_the_file_selection() {
    let db = Database::open_in_memory().unwrap();
    let (c1, c2) = (context(&db, "c1"), context(&db, "c2"));
    let lay_oct = doc(&db, c1, "a", meta("kenneth lay <klay@enron.com>", "2001-10-05", "maildir/lay-k/inbox"));
    let lay_old = doc(&db, c1, "b", meta("kenneth lay <klay@enron.com>", "2000-01-01", "maildir/lay-k/inbox_old"));
    let other = doc(&db, c1, "c", meta("jeff skilling", "2001-10-07", "maildir/lay-k/sent"));
    let plain = doc(&db, c1, "d", DocMeta::default());
    let elsewhere = doc(&db, c2, "e", meta("kenneth lay", "2001-10-05", "x"));
    let f = |sender: Option<&str>, from: Option<&str>, to: Option<&str>, folder: Option<&str>| DocFilter {
        sender: sender.map(Into::into),
        date_from: from.map(Into::into),
        date_to: to.map(Into::into),
        folder: folder.map(Into::into),
    };
    let ids = |filter: DocFilter| db.filtered_document_ids(&[c1], &filter).unwrap();
    assert_eq!(ids(f(Some("Lay"), None, None, None)), vec![lay_oct, lay_old]);
    assert_eq!(ids(f(None, Some("2001-09"), Some("2001-11"), None)), vec![lay_oct, other]);
    assert_eq!(ids(f(Some("lay"), Some("2001"), Some("2001"), None)), vec![lay_oct]);
    assert_eq!(ids(f(None, None, None, Some("inbox"))), vec![lay_oct, lay_old]);
    assert_eq!(ids(f(None, None, None, Some("/inbox/"))), vec![lay_oct]);
    assert_eq!(ids(f(None, None, None, Some("LAY-K/sent"))), vec![other]);
    assert!(ids(f(Some("nobody"), None, None, None)).is_empty());
    assert_eq!(db.filtered_document_ids(&[c1, c2], &f(Some("kenneth"), Some("2001-10-05"), None, None)).unwrap(), vec![lay_oct, elsewhere]);

    // Scope: empty filter keeps the selection, a filter applies inside it.
    assert_eq!(db.narrow_scope(&[c1], None, &DocFilter::default()).unwrap(), None);
    assert_eq!(db.narrow_scope(&[c1], Some(vec![plain]), &DocFilter::default()).unwrap(), Some(vec![plain]));
    assert_eq!(db.narrow_scope(&[c1], Some(vec![lay_old, other]), &f(Some("lay"), None, None, None)).unwrap(), Some(vec![lay_old]));
    assert_eq!(db.narrow_scope(&[c1], None, &f(Some("nobody"), None, None, None)).unwrap(), Some(vec![]));
    assert!(db.narrow_scope(&[c1], None, &f(None, Some("autumn"), None, None)).is_err());
}

#[test]
fn the_filter_reads_only_the_covering_index() {
    let db = Database::open_in_memory().unwrap();
    for cond in ["sender LIKE '%a%'", "doc_date >= '2001' AND doc_date <= '2002'", "('/' || folder || '/') LIKE '%a%'"] {
        let sql = format!("EXPLAIN QUERY PLAN SELECT id FROM documents INDEXED BY idx_documents_filter WHERE context_id = 1 AND {cond} ORDER BY id");
        let mut stmt = db.conn.prepare(&sql).unwrap();
        let plan: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(3)).unwrap().map(|r| r.unwrap()).collect();
        let plan = plan.join("; ");
        assert!(plan.contains("COVERING INDEX idx_documents_filter"), "{cond}: {plan}");
    }
}

#[test]
fn a_file_on_disk_gives_its_folder_unless_the_document_has_one() {
    let db = Database::open_in_memory().unwrap();
    let c = context(&db, "c");
    let loose = doc(&db, c, "1.eml", DocMeta::default());
    let zipped = doc(&db, c, "x", DocMeta { folder: Some("inner".into()), ..Default::default() });
    for (id, path) in [(loose, "/Data/Maildir/Lay-K/Inbox/1.eml"), (zipped, "/data/a.zip")] {
        let f = SourceFile { document_id: id, context_id: c, path: path.into(), byte_size: 1, mtime_ns: 1, sha256: "h".into(), missing_since: None };
        db.record_source_file(&f, false).unwrap();
    }
    assert_eq!(db.document_meta(loose).unwrap().unwrap().folder.as_deref(), Some("/data/maildir/lay-k/inbox"));
    assert_eq!(db.document_meta(zipped).unwrap().unwrap().folder.as_deref(), Some("inner"));
}

#[test]
fn v75_fills_existing_mails_attachments_and_folders() {
    let conn = migrate_to(74);
    conn.execute(
        "INSERT INTO contexts (id, name, chunking_strategy) VALUES (1, 'old', 'structural')",
        [],
    )
    .unwrap();
    let insert = "INSERT INTO documents (id, context_id, name, zip_entry, extracted_text, chunker) VALUES (?1, 1, ?2, ?3, ?4, ?5)";
    for (id, name, zip_entry, text, chunker) in [
        (1, "Gas prices", Some("maildir/lay-k/sent/1.eml"), MAIL, Some("mail")),
        (2, "report", Some("maildir/lay-k/sent/1.eml › report.pdf"), "pdf text", None),
        (3, "Law", Some("laws/law.pdf"), "**From:** not a mail", None),
        (4, "No extension", Some("2."), "**From:** C\n**Date:** 2000-03-04 05:06 Z", Some("mail")),
        (5, "loose.txt", None, "t", None),
    ] {
        conn.execute(insert, rusqlite::params![id, name, zip_entry, text, chunker]).unwrap();
    }
    conn.execute(
        "INSERT INTO source_files (document_id, context_id, path, byte_size, mtime_ns, sha256) VALUES (5, 1, '/disk/Notes/loose.txt', 1, 1, 'h')",
        [],
    )
    .unwrap();
    let db = Database::init(conn).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::MIGRATIONS.len() as i64);
    let m = |id| db.document_meta(id).unwrap().unwrap();
    assert_eq!(m(1), meta("kenneth lay <kenneth.lay@enron.com>", "2001-10-05", "maildir/lay-k/sent"));
    assert_eq!(m(2), m(1));
    assert_eq!(m(3), DocMeta { folder: Some("laws".into()), ..Default::default() });
    assert_eq!(m(4), DocMeta { sender: Some("c".into()), date: Some("2000-03-04".into()), folder: None });
    assert_eq!(m(5).folder.as_deref(), Some("/disk/notes"));
}

#[test]
fn a_scope_of_more_documents_than_sqlite_has_placeholders_still_searches() {
    let db = Database::open_in_memory().unwrap();
    let c = context(&db, "c");
    let d = doc(&db, c, "a", DocMeta::default());
    db.create_chunk(&NewChunk { context_id: c, document_id: d, chunk_index: 0, char_start: 0, char_end: 5, text: "gas prices".into(), signature: None, is_omitted: false, metadata: "{}".into() })
        .unwrap();
    let mut ids: Vec<i64> = (100_000..140_000).collect();
    ids.push(d);
    assert_eq!(db.keyword_search_context(c, "gas", 5, Some(&ids)).unwrap().len(), 1);
    assert_eq!(db.phrase_search_context(c, "gas prices", 5, Some(&ids)).unwrap().len(), 1);
    assert!(db.keyword_search_context(c, "gas", 5, Some(&ids[..10])).unwrap().is_empty());
    assert!(db.sheet_tables_for_scope(&[c], Some(&ids)).unwrap().is_empty());
}
