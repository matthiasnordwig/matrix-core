//! Spreadsheet tables (schema_v63): column naming, creation with typed values,
//! scope listing, dropping with the document/context, the orphan sweep, and
//! the migration on top of an existing v62 database.

use rusqlite::Connection;

use super::*;
use crate::db::models::{NewContext, NewDocument, NewSheetColumn};

pub(crate) fn seed_doc(db: &Database, ctx_name: &str) -> (i64, i64) {
    let ctx = db.create_context(&NewContext { name: ctx_name.into(), chunking_strategy: "structural".into(), ..Default::default() }).unwrap();
    let doc = db
        .create_document(&NewDocument {
            context_id: ctx.id,
            name: format!("{ctx_name}.xlsx"),
            zip_entry: None,
            byte_size: None,
            page_count: None,
            content_hash: None,
            extracted_text: Some("## Sheet1".into()),
        })
        .unwrap();
    (ctx.id, doc.id)
}

fn col(name: &str, t: SheetColumnType) -> NewSheetColumn {
    NewSheetColumn { name: name.into(), sql_type: t, note: None, example: None }
}

/// A small sales table: region (text), amount (real), units (integer), share (percent as fraction).
pub(crate) fn sales_table(ctx: i64, doc: i64) -> NewSheetTable {
    NewSheetTable {
        context_id: ctx,
        document_id: doc,
        sheet_name: "Sales".into(),
        region_index: 0,
        columns: vec![
            col("Region", SheetColumnType::Text),
            col("Umsatz (EUR)", SheetColumnType::Real),
            col("Stück", SheetColumnType::Integer),
            NewSheetColumn { note: Some("percent, stored as a fraction".into()), example: Some("25%".into()), ..col("Anteil %", SheetColumnType::Real) },
        ],
        sheet_rows: vec![2, 3, 4],
        rows: vec![
            vec![SheetValue::Text("Nord".into()), SheetValue::Real(1200.5), SheetValue::Int(10), SheetValue::Real(0.25)],
            vec![SheetValue::Text("Süd".into()), SheetValue::Real(800.0), SheetValue::Int(4), SheetValue::Real(0.5)],
            vec![SheetValue::Text("Nord".into()), SheetValue::Null, SheetValue::Int(6), SheetValue::Real(0.25)],
        ],
    }
}

fn table_exists(db: &Database, name: &str) -> bool {
    db.conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1", [name], |r| r.get::<_, i64>(0)).unwrap() == 1
}

#[test]
fn column_names_are_sanitised_and_unique() {
    let h = |v: &[&str]| sql_column_names(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(h(&["Umsatz (EUR)", "Stück", "Anteil %", "Größe"]), vec!["umsatz_eur", "stueck", "anteil_pct", "groesse"]);
    assert_eq!(h(&["Name", "name", "NAME"]), vec!["name", "name_2", "name_3"]);
    assert_eq!(h(&["", "  ", "2024", "order", "sheet_row"]), vec!["col_1", "col_2", "c_2024", "c_order", "sheet_row_2"]);
    assert_eq!(h(&["a\"; DROP TABLE x; --"]), vec!["a_drop_table_x"]);
    assert!(is_sheet_table_name("sheet_12_3"));
    assert!(!is_sheet_table_name("sheet_tables") && !is_sheet_table_name("sheet_1_") && !is_sheet_table_name("sheet_1_2_3"));
}

#[test]
fn create_stores_typed_rows_and_registry() {
    let db = Database::open_in_memory().unwrap();
    let (ctx, doc) = seed_doc(&db, "c");
    let info = db.create_sheet_table(&sales_table(ctx, doc)).unwrap();
    assert_eq!(info.table_name, format!("sheet_{doc}_1"));
    assert_eq!((info.row_count, info.document_name.as_str(), info.sheet_name.as_str()), (3, "c.xlsx", "Sales"));
    let sql: Vec<&str> = info.columns.iter().map(|c| c.sql.as_str()).collect();
    assert_eq!(sql, vec!["sheet_row", "region", "umsatz_eur", "stueck", "anteil_pct"]);
    assert_eq!(info.columns[4].note.as_deref(), Some("percent, stored as a fraction"));
    let (sum, units, nulls): (f64, i64, i64) = db
        .conn
        .query_row(&format!("SELECT SUM(umsatz_eur), SUM(stueck), COUNT(*) - COUNT(umsatz_eur) FROM {}", info.table_name), [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((sum, units, nulls), (2000.5, 20, 1));
    let t: String = db.conn.query_row(&format!("SELECT typeof(stueck) FROM {} LIMIT 1", info.table_name), [], |r| r.get(0)).unwrap();
    assert_eq!(t, "integer");
    // Read back exactly as stored (context transfer).
    let rows = db.sheet_table_rows(&info).unwrap();
    let src = sales_table(ctx, doc);
    assert_eq!(rows.iter().map(|(r, _)| *r).collect::<Vec<_>>(), src.sheet_rows);
    assert_eq!(rows.into_iter().map(|(_, v)| v).collect::<Vec<_>>(), src.rows);

    // A second region of the same document gets the next number; scope listing filters.
    let second = db.create_sheet_table(&NewSheetTable { region_index: 2, ..sales_table(ctx, doc) }).unwrap();
    assert_eq!(second.table_name, format!("sheet_{doc}_2"));
    let (ctx2, doc2) = seed_doc(&db, "d");
    db.create_sheet_table(&sales_table(ctx2, doc2)).unwrap();
    assert_eq!(db.sheet_tables_for_scope(&[ctx], None).unwrap().len(), 2);
    assert_eq!(db.sheet_tables_for_scope(&[ctx, ctx2], None).unwrap().len(), 3);
    assert_eq!(db.sheet_tables_for_scope(&[ctx, ctx2], Some(&[doc2])).unwrap().len(), 1);
    assert!(db.sheet_tables_for_scope(&[ctx], Some(&[])).unwrap().is_empty());
}

#[test]
fn malformed_input_leaves_nothing_behind() {
    let db = Database::open_in_memory().unwrap();
    let (ctx, doc) = seed_doc(&db, "c");
    let mut bad = sales_table(ctx, doc);
    bad.rows[1].pop();
    assert!(db.create_sheet_table(&bad).is_err());
    let mut no_cols = sales_table(ctx, doc);
    no_cols.columns.clear();
    assert!(db.create_sheet_table(&no_cols).is_err());
    assert!(!table_exists(&db, &format!("sheet_{doc}_1")));
    assert!(db.sheet_tables_for_scope(&[ctx], None).unwrap().is_empty());
}

#[test]
fn deleting_document_or_context_drops_the_physical_tables() {
    let db = Database::open_in_memory().unwrap();
    let (ctx, doc) = seed_doc(&db, "c");
    let t1 = db.create_sheet_table(&sales_table(ctx, doc)).unwrap().table_name;
    assert!(table_exists(&db, &t1));
    assert!(db.delete_document(doc).unwrap());
    assert!(!table_exists(&db, &t1), "document delete drops its table");
    assert!(db.sheet_tables_for_scope(&[ctx], None).unwrap().is_empty());

    let (ctx2, doc2) = seed_doc(&db, "d");
    let t2 = db.create_sheet_table(&sales_table(ctx2, doc2)).unwrap().table_name;
    assert!(db.delete_context(ctx2).unwrap());
    assert!(!table_exists(&db, &t2), "context delete drops its tables");
    let left: i64 = db.conn.query_row("SELECT COUNT(*) FROM sheet_tables", [], |r| r.get(0)).unwrap();
    assert_eq!(left, 0);
}

#[test]
fn orphan_sweep_drops_unregistered_sheet_tables_only() {
    let db = Database::open_in_memory().unwrap();
    let (ctx, doc) = seed_doc(&db, "c");
    let kept = db.create_sheet_table(&sales_table(ctx, doc)).unwrap().table_name;
    db.conn.execute_batch("CREATE TABLE sheet_999_1 (x INTEGER); CREATE TABLE sheet_notes (x INTEGER);").unwrap();
    assert_eq!(db.drop_orphan_sheet_tables().unwrap(), 1);
    assert!(table_exists(&db, &kept) && table_exists(&db, "sheet_notes") && table_exists(&db, "sheet_tables"));
    assert!(!table_exists(&db, "sheet_999_1"));
}

#[test]
fn migration_v63_applies_on_a_v62_database() {
    let dir = std::env::temp_dir().join(format!("matrix-sheet-v62-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("matrix.db");
    {
        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        for (i, sql) in super::super::MIGRATIONS[..62].iter().enumerate() {
            let target = (i + 1) as i64;
            if target == 51 {
                super::super::migrate_v50_to_v51_embedding_kind(&mut conn).unwrap();
                continue;
            }
            if target == 52 {
                super::super::migrate_v51_to_v52_reranker_kind(&mut conn).unwrap();
                continue;
            }
            let tx = conn.transaction().unwrap();
            tx.execute_batch(sql).unwrap();
            if target == 9 {
                super::super::migrate_v8_to_v9_profiles(&tx).unwrap();
            }
            if target == 50 {
                super::super::migrate_v49_to_v50_reranker(&tx).unwrap();
            }
            tx.pragma_update(None, "user_version", target).unwrap();
            tx.commit().unwrap();
        }
        conn.execute_batch(
            "INSERT INTO contexts (id, name, chunking_strategy) VALUES (1, 'old', 'structural');
             INSERT INTO documents (id, context_id, name) VALUES (1, 1, 'old.xlsx');",
        )
        .unwrap();
    }
    let db = Database::open(&path).unwrap();
    assert_eq!(db.schema_version().unwrap(), super::super::MIGRATIONS.len() as i64);
    assert_eq!(db.document(1).unwrap().unwrap().name, "old.xlsx");
    assert!(db.sheet_tables_for_scope(&[1], None).unwrap().is_empty());
    let info = db.create_sheet_table(&sales_table(1, 1)).unwrap();
    assert_eq!(db.file_path().unwrap().canonicalize().unwrap(), path.canonicalize().unwrap());
    assert!(table_exists(&db, &info.table_name));
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}
