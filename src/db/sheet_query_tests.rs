//! The read-only sheet query (`sheet_query.rs`): correct results and CSV,
//! every fence (other tables incl. `llm_endpoints` keys, `sqlite_master`,
//! PRAGMA, ATTACH, writes/DDL, `load_extension`, a second statement), the time
//! limit against a recursive CTE without end, and result truncation.

use std::path::PathBuf;
use std::time::Duration;

use super::llm_endpoint_tests::seed_endpoint;
use super::sheet_query::{run_sheet_query, SheetQueryLimits};
use super::sheet_tables::sheet_tables_tests::{sales_table, seed_doc};
use super::Database;

/// A file database with one sales table in context A and one in context B.
struct Fixture {
    dir: PathBuf,
    path: PathBuf,
    db: Database,
    table_a: String,
    table_b: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(tag: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("matrix-sheet-query-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("matrix.db");
    let db = Database::open(&path).unwrap();
    let (ctx, doc) = seed_doc(&db, "a");
    let table_a = db.create_sheet_table(&sales_table(ctx, doc)).unwrap().table_name;
    let (ctx_b, doc_b) = seed_doc(&db, "b");
    let table_b = db.create_sheet_table(&sales_table(ctx_b, doc_b)).unwrap().table_name;
    let ep = seed_endpoint(&db, "secret-ep", "openai");
    db.conn.execute("UPDATE llm_endpoints SET api_key_ref = 'sk-secret' WHERE id = ?1", [ep]).unwrap();
    Fixture { dir, path, db, table_a, table_b }
}

fn run(f: &Fixture, sql: &str) -> Result<super::sheet_query::SheetQueryResult, String> {
    run_sheet_query(&f.path, std::slice::from_ref(&f.table_a), sql, &SheetQueryLimits::default()).map_err(|e| e.to_string())
}

#[test]
fn aggregates_come_back_as_csv() {
    let f = fixture("agg");
    let r = run(&f, &format!("SELECT region, SUM(umsatz_eur) AS total, COUNT(*) AS n FROM {} GROUP BY region ORDER BY region", f.table_a)).unwrap();
    assert_eq!(r.csv, "region,total,n\nNord,1200.5,2\nSüd,800,1");
    assert_eq!((r.rows, r.truncated), (2, false));
    assert_eq!(r.tables_read, vec![f.table_a.clone()]);
    // CTEs, window functions and a trailing semicolon are fine.
    let r = run(&f, &format!("WITH t AS (SELECT * FROM {}) SELECT sheet_row, rank() OVER (ORDER BY stueck DESC) FROM t;", f.table_a)).unwrap();
    assert_eq!(r.rows, 3);
    let r = run(&f, &format!("WITH k AS (SELECT * FROM {}) SELECT COUNT(*) FROM k", f.table_a)).unwrap();
    assert_eq!(r.csv, "COUNT(*)\n3");
    let r = run(&f, &format!("SELECT COUNT(*) FROM {}", f.table_a)).unwrap();
    assert_eq!(r.csv, "COUNT(*)\n3");
    let r = run(&f, "SELECT 'a,b', NULL, 'say \"hi\"'").unwrap();
    assert_eq!(r.csv.lines().nth(1).unwrap(), "\"a,b\",,\"say \"\"hi\"\"\"");
}

#[test]
fn other_tables_are_denied() {
    let f = fixture("deny");
    for sql in [
        "SELECT api_key_ref FROM llm_endpoints".to_string(),
        "SELECT text FROM chunks".to_string(),
        "SELECT COUNT(*) FROM llm_endpoints".to_string(),
        "WITH k AS (SELECT * FROM llm_endpoints) SELECT COUNT(*) FROM k".to_string(),
        "SELECT COUNT(*) FROM sqlite_master".to_string(),
        "SELECT COUNT(*) FROM chunks_fts_data".to_string(),
        "SELECT * FROM sheet_tables".to_string(),
        "SELECT sql FROM sqlite_master".to_string(),
        "SELECT * FROM pragma_table_info('llm_endpoints')".to_string(),
        format!("SELECT * FROM {}", f.table_b),
        format!("SELECT * FROM {} WHERE region IN (SELECT name FROM llm_endpoints)", f.table_a),
    ] {
        let e = run(&f, &sql).unwrap_err();
        assert!(e.contains("not one of the spreadsheet tables") || e.contains("not allowed"), "{sql}: {e}");
    }
}

#[test]
fn writes_pragmas_attach_and_extensions_are_denied() {
    let f = fixture("write");
    let t = &f.table_a;
    for sql in [
        format!("INSERT INTO {t} (region) VALUES ('x')"),
        format!("UPDATE {t} SET region = 'x'"),
        format!("DELETE FROM {t}"),
        format!("DROP TABLE {t}"),
        "CREATE TABLE evil (x)".to_string(),
        "UPDATE sqlite_master SET sql = ''".to_string(),
        "PRAGMA query_only = OFF".to_string(),
        "PRAGMA table_info(llm_endpoints)".to_string(),
        "ATTACH DATABASE ':memory:' AS other".to_string(),
        "SELECT load_extension('/tmp/x')".to_string(),
        "SELECT zeroblob(10)".to_string(),
        format!("SELECT 1 FROM {t}; DROP TABLE {t}"),
        format!("SELECT 1 FROM {t}; SELECT 2"),
        "BEGIN".to_string(),
    ] {
        let e = run(&f, &sql);
        assert!(e.is_err(), "{sql} must fail, got {e:?}");
    }
    let e = run(&f, "SELECT load_extension('/tmp/x')").unwrap_err();
    assert!(e.contains("load_extension"), "{e}");
    let e = run(&f, &format!("SELECT 1 FROM {t}; SELECT 2")).unwrap_err();
    assert!(e.contains("exactly one"), "{e}");
    // Nothing changed.
    let n: i64 = f.db.conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap();
    assert_eq!(n, 3);
    let key: String = f.db.conn.query_row("SELECT api_key_ref FROM llm_endpoints", [], |r| r.get(0)).unwrap();
    assert_eq!(key, "sk-secret");
}

#[test]
fn time_limit_stops_an_endless_recursive_cte() {
    let f = fixture("time");
    let limits = SheetQueryLimits { timeout: Duration::from_millis(300), ..Default::default() };
    let started = std::time::Instant::now();
    let e = run_sheet_query(
        &f.path,
        std::slice::from_ref(&f.table_a),
        "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n) SELECT COUNT(*) FROM n",
        &limits,
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("time limit"), "{e}");
    assert!(started.elapsed() < Duration::from_secs(5));
    // Huge strings are capped inside SQLite.
    let big = run(&f, "SELECT printf('%.*c', 50000000, 'x')");
    assert!(big.as_ref().map_or(true, |r| r.csv.len() < 10_000), "{big:?}");
    let e = run(&f, "SELECT length(replace(printf('%.*c', 900000, 'x'), 'x', 'yy'))").unwrap_err();
    assert!(e.contains("too big"), "{e}");
}

#[test]
fn results_are_truncated_by_rows_and_chars() {
    let f = fixture("trunc");
    let sql = "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 1000) SELECT x, 'row ' || x FROM n";
    let r = run(&f, sql).unwrap();
    assert_eq!((r.rows, r.truncated), (200, true));
    let small = SheetQueryLimits { max_rows: 1000, max_chars: 300, ..Default::default() };
    let r = run_sheet_query(&f.path, &[], sql, &small).unwrap();
    assert!(r.truncated && r.csv.len() <= 300 && r.rows > 5, "{} rows, {} chars", r.rows, r.csv.len());
    let r = run(&f, "SELECT 1 WHERE 0").unwrap();
    assert_eq!((r.csv.as_str(), r.rows, r.truncated), ("1", 0, false));
}

#[test]
fn in_memory_database_has_no_file_for_the_query() {
    let db = Database::open_in_memory().unwrap();
    assert!(db.file_path().is_none());
}
