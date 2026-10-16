//! Spreadsheet regions as real SQL tables (schema_v63): creating them, the
//! registry `sheet_tables`, listing them for a chat scope, and dropping them.
//!
//! **Names.** A table is `sheet_<document_id>_<n>` (n counts the document's
//! tables from 1), so it never collides with a schema table and the orphan
//! sweep can recognise it by shape ([`is_sheet_table_name`]). Columns get SQL
//! names from the detected header ([`sql_column_names`]: lower case, umlauts
//! spelled out, everything else `_`, SQL keywords and digits-first names
//! prefixed, de-duplicated with `_2`, `_3`); the first column is always
//! `sheet_row` (the row number in the spreadsheet). The original headers stay
//! in `columns_json` for the table description.
//!
//! **Lifetime.** The registry row goes with its document or context by FK
//! CASCADE. The physical table cannot (a trigger cannot DROP TABLE), so
//! `delete_document`/`delete_context` (`contexts.rs`) call
//! [`Database::drop_sheet_tables_where`] in the same savepoint, and
//! [`Database::drop_orphan_sheet_tables`] runs at every open as a safety net
//! for any other path that removes registry rows.
//!
//! Creation runs in one savepoint (registry row + CREATE + inserts), so a
//! failure leaves nothing behind. Querying is `sheet_query.rs` (own read-only
//! connection), never this connection.

use std::collections::HashSet;

use rusqlite::{params, params_from_iter, OptionalExtension};

use super::models::{NewSheetTable, SheetColumnInfo, SheetColumnType, SheetTableInfo, SheetValue};
use super::{CoreError, Database, Result};

/// Name of the extra first column holding the spreadsheet row number.
pub const SHEET_ROW_COLUMN: &str = "sheet_row";
/// Longest SQL column name (characters).
const MAX_NAME_CHARS: usize = 48;
/// Widest table stored (SQLite allows 2000 columns; wider sheets are no tables to query).
pub const MAX_SHEET_COLUMNS: usize = 500;

/// Words that would need quoting as a column name (kept short: the ones a header plausibly is).
const KEYWORDS: &[&str] = &[
    "all", "and", "as", "asc", "between", "by", "case", "cast", "check", "collate", "column", "create", "default",
    "delete", "desc", "distinct", "drop", "else", "end", "except", "exists", "from", "group", "having", "in",
    "index", "insert", "intersect", "into", "is", "join", "key", "like", "limit", "not", "null", "offset", "on",
    "or", "order", "primary", "references", "rowid", "oid", "select", "set", "table", "then", "to", "union",
    "unique", "update", "using", "values", "when", "where", "with",
];

/// Whether `name` has the shape of a physical sheet table (`sheet_<digits>_<digits>`).
pub fn is_sheet_table_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("sheet_") else { return false };
    let mut parts = rest.split('_');
    let num = |p: Option<&str>| p.is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
    num(parts.next()) && num(parts.next()) && parts.next().is_none()
}

/// One header as an SQL identifier (before de-duplication); `""` when nothing usable is left.
fn sanitize(header: &str) -> String {
    let mut out = String::new();
    for ch in header.chars().flat_map(char::to_lowercase) {
        let piece = match ch {
            'ä' => "ae",
            'ö' => "oe",
            'ü' => "ue",
            'ß' => "ss",
            'é' | 'è' | 'ê' => "e",
            'á' | 'à' | 'â' => "a",
            'ó' | 'ò' | 'ô' => "o",
            'í' | 'ì' | 'î' => "i",
            'ú' | 'ù' | 'û' => "u",
            '%' => "pct",
            '€' => "eur",
            c if c.is_ascii_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => "_",
        };
        if piece == "_" && (out.is_empty() || out.ends_with('_')) {
            continue;
        }
        out.push_str(piece);
    }
    let mut s: String = out.trim_matches('_').chars().take(MAX_NAME_CHARS).collect();
    while s.ends_with('_') {
        s.pop();
    }
    if s.starts_with(|c: char| c.is_ascii_digit()) || KEYWORDS.contains(&s.as_str()) {
        s = format!("c_{s}");
    }
    s
}

/// SQL names for `headers` in order: sanitised, `col_<n>` when empty, unique
/// (also against `sheet_row`) with `_2`, `_3` … appended.
pub fn sql_column_names(headers: &[String]) -> Vec<String> {
    let mut used: HashSet<String> = HashSet::from([SHEET_ROW_COLUMN.to_string()]);
    headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let base = match sanitize(h) {
                s if s.is_empty() => format!("col_{}", i + 1),
                s => s,
            };
            let mut name = base.clone();
            let mut k = 2;
            while !used.insert(name.clone()) {
                name = format!("{base}_{k}");
                k += 1;
            }
            name
        })
        .collect()
}

fn to_sql_value(v: &SheetValue) -> rusqlite::types::Value {
    use rusqlite::types::Value;
    match v {
        SheetValue::Null => Value::Null,
        SheetValue::Int(i) => Value::Integer(*i),
        SheetValue::Real(f) if f.is_finite() => Value::Real(*f),
        SheetValue::Real(_) => Value::Null,
        SheetValue::Text(s) => Value::Text(s.clone()),
    }
}

fn row_to_info(r: &rusqlite::Row) -> rusqlite::Result<(SheetTableInfo, String)> {
    Ok((
        SheetTableInfo {
            id: r.get("id")?,
            context_id: r.get("context_id")?,
            document_id: r.get("document_id")?,
            document_name: r.get("document_name")?,
            sheet_name: r.get("sheet_name")?,
            region_index: r.get("region_index")?,
            table_name: r.get("table_name")?,
            columns: Vec::new(),
            row_count: r.get("row_count")?,
        },
        r.get("columns_json")?,
    ))
}

const SELECT_INFO: &str = "SELECT t.id, t.context_id, t.document_id, d.name AS document_name, t.sheet_name, \
     t.region_index, t.table_name, t.columns_json, t.row_count FROM sheet_tables t JOIN documents d ON d.id = t.document_id";

impl Database {
    /// The file this database lives in (`None` for an in-memory database).
    /// The table query opens its own read-only connection on it.
    pub fn file_path(&self) -> Option<std::path::PathBuf> {
        self.conn.path().filter(|p| !p.is_empty()).map(std::path::PathBuf::from)
    }

    /// Store one table region: registry row + physical table + rows, all or nothing.
    pub fn create_sheet_table(&self, t: &NewSheetTable) -> Result<SheetTableInfo> {
        if t.columns.is_empty() || t.columns.len() > MAX_SHEET_COLUMNS {
            return Err(CoreError::InvalidState(format!("a sheet table needs 1–{MAX_SHEET_COLUMNS} columns, got {}", t.columns.len())));
        }
        if t.sheet_rows.len() != t.rows.len() || t.rows.iter().any(|r| r.len() != t.columns.len()) {
            return Err(CoreError::InvalidState("sheet table rows do not match its columns".into()));
        }
        let headers: Vec<String> = t.columns.iter().map(|c| c.name.clone()).collect();
        let names = sql_column_names(&headers);
        let mut columns = vec![SheetColumnInfo {
            name: "(row number in the sheet)".into(),
            sql: SHEET_ROW_COLUMN.into(),
            sql_type: SheetColumnType::Integer,
            note: None,
            example: None,
        }];
        columns.extend(t.columns.iter().zip(&names).map(|(c, sql)| SheetColumnInfo {
            name: c.name.clone(),
            sql: sql.clone(),
            sql_type: c.sql_type,
            note: c.note.clone(),
            example: c.example.clone(),
        }));
        self.conn.execute_batch("SAVEPOINT create_sheet_table")?;
        let res = self.create_sheet_table_inner(t, &columns);
        match res {
            Ok(info) => {
                self.conn.execute_batch("RELEASE create_sheet_table")?;
                Ok(info)
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK TO create_sheet_table; RELEASE create_sheet_table");
                Err(e)
            }
        }
    }

    fn create_sheet_table_inner(&self, t: &NewSheetTable, columns: &[SheetColumnInfo]) -> Result<SheetTableInfo> {
        let n: i64 = self.conn.query_row("SELECT COUNT(*) FROM sheet_tables WHERE document_id = ?1", [t.document_id], |r| r.get(0))?;
        let mut k = n + 1;
        let table = loop {
            let name = format!("sheet_{}_{k}", t.document_id);
            let taken: Option<i64> = self.conn.query_row("SELECT 1 FROM sqlite_master WHERE name = ?1", [&name], |r| r.get(0)).optional()?;
            if taken.is_none() {
                break name;
            }
            k += 1;
        };
        let defs: Vec<String> = columns.iter().map(|c| format!("\"{}\" {}", c.sql, c.sql_type.sql())).collect();
        self.conn.execute_batch(&format!("CREATE TABLE \"{table}\" ({})", defs.join(", ")))?;
        let marks = vec!["?"; columns.len()].join(", ");
        {
            let mut stmt = self.conn.prepare(&format!("INSERT INTO \"{table}\" VALUES ({marks})"))?;
            for (row_no, row) in t.sheet_rows.iter().zip(&t.rows) {
                let values = std::iter::once(rusqlite::types::Value::Integer(*row_no)).chain(row.iter().map(to_sql_value));
                stmt.execute(params_from_iter(values))?;
            }
        }
        self.conn.execute(
            "INSERT INTO sheet_tables (context_id, document_id, sheet_name, region_index, table_name, columns_json, row_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![t.context_id, t.document_id, t.sheet_name, t.region_index, table, serde_json::to_string(columns)?, t.rows.len() as i64],
        )?;
        let id = self.conn.last_insert_rowid();
        self.sheet_table(id)?.ok_or_else(|| CoreError::NotFound(format!("sheet table {id}")))
    }

    /// One registry row by id.
    pub fn sheet_table(&self, id: i64) -> Result<Option<SheetTableInfo>> {
        let found = self.conn.query_row(&format!("{SELECT_INFO} WHERE t.id = ?1"), [id], row_to_info).optional()?;
        found.map(finish_info).transpose()
    }

    /// The tables a chat over `context_ids` may read: every table of those
    /// contexts, or only of `doc_ids` when a file selection is given
    /// (`Some(&[])` = none). Ordered by document, sheet order, region.
    pub fn sheet_tables_for_scope(&self, context_ids: &[i64], doc_ids: Option<&[i64]>) -> Result<Vec<SheetTableInfo>> {
        if context_ids.is_empty() || doc_ids.is_some_and(|d| d.is_empty()) {
            return Ok(Vec::new());
        }
        let ctx_marks = vec!["?"; context_ids.len()].join(",");
        let mut sql = format!("{SELECT_INFO} WHERE t.context_id IN ({ctx_marks})");
        let mut args: Vec<i64> = context_ids.to_vec();
        if let Some(docs) = doc_ids {
            sql.push_str(&format!(" AND t.document_id IN ({})", vec!["?"; docs.len()].join(",")));
            args.extend_from_slice(docs);
        }
        sql.push_str(" ORDER BY t.document_id, t.id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args), row_to_info)?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(finish_info).collect()
    }

    /// The rows of a registered sheet table as stored: `(sheet_row, values)`
    /// with one value per column after `sheet_row` (context transfer export).
    pub fn sheet_table_rows(&self, info: &SheetTableInfo) -> Result<Vec<(i64, Vec<SheetValue>)>> {
        let registered: Option<i64> = self.conn.query_row("SELECT id FROM sheet_tables WHERE table_name = ?1", [&info.table_name], |r| r.get(0)).optional()?;
        if registered.is_none() || !is_sheet_table_name(&info.table_name) {
            return Err(CoreError::NotFound(format!("sheet table {}", info.table_name)));
        }
        let mut stmt = self.conn.prepare(&format!("SELECT * FROM \"{}\" ORDER BY rowid", info.table_name))?;
        let n = stmt.column_count();
        let rows = stmt.query_map([], |r| {
            let values = (1..n)
                .map(|i| {
                    Ok(match r.get_ref(i)? {
                        rusqlite::types::ValueRef::Integer(v) => SheetValue::Int(v),
                        rusqlite::types::ValueRef::Real(v) => SheetValue::Real(v),
                        rusqlite::types::ValueRef::Text(t) => SheetValue::Text(String::from_utf8_lossy(t).into_owned()),
                        _ => SheetValue::Null,
                    })
                })
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((r.get::<_, i64>(0)?, values))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Drop the physical tables of the registry rows matching `filter`
    /// (`"document_id = ?1"` or `"context_id = ?1"`); the rows themselves go by CASCADE.
    pub(crate) fn drop_sheet_tables_where(&self, filter: &str, id: i64) -> Result<usize> {
        let names: Vec<String> = {
            let mut stmt = self.conn.prepare(&format!("SELECT table_name FROM sheet_tables WHERE {filter}"))?;
            let rows = stmt.query_map([id], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for name in names.iter().filter(|n| is_sheet_table_name(n)) {
            self.conn.execute_batch(&format!("DROP TABLE IF EXISTS \"{name}\""))?;
        }
        Ok(names.len())
    }

    /// Drop physical sheet tables that no registry row names (left behind by a
    /// path that removed rows without dropping). Returns how many were dropped.
    pub fn drop_orphan_sheet_tables(&self) -> Result<usize> {
        let orphans: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'sheet\\_%' ESCAPE '\\'
                   AND name NOT IN (SELECT table_name FROM sheet_tables)",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().filter(|n| is_sheet_table_name(n)).collect()
        };
        for name in &orphans {
            self.conn.execute_batch(&format!("DROP TABLE IF EXISTS \"{name}\""))?;
        }
        Ok(orphans.len())
    }

    /// Run `f` (a delete) together with dropping the sheet tables it orphans, in one savepoint.
    pub(crate) fn with_sheet_drop<T>(&self, filter: &str, id: i64, f: impl FnOnce() -> Result<T>) -> Result<T> {
        self.conn.execute_batch("SAVEPOINT drop_sheets")?;
        let res = self.drop_sheet_tables_where(filter, id).and_then(|_| f());
        match res {
            Ok(v) => {
                self.conn.execute_batch("RELEASE drop_sheets")?;
                Ok(v)
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK TO drop_sheets; RELEASE drop_sheets");
                Err(e)
            }
        }
    }
}

fn finish_info((mut info, json): (SheetTableInfo, String)) -> Result<SheetTableInfo> {
    info.columns = serde_json::from_str(&json)?;
    Ok(info)
}

#[cfg(test)]
#[path = "sheet_tables_tests.rs"]
pub(crate) mod sheet_tables_tests;
