//! Read-only SQL over the spreadsheet tables (`sheet_tables.rs`) — the engine
//! behind the chat tool and the MCP tool `query_table`. The SQL comes from a
//! model, so it is treated as hostile; five independent fences:
//!
//! 1. **Own connection**, opened `SQLITE_OPEN_READ_ONLY` on the database file
//!    (never the app's connection), plus `PRAGMA query_only = ON`.
//! 2. **Authorizer** (installed after that pragma): allows `SELECT`, reads of
//!    the tables in `allowed` (the registered sheet tables of the caller's
//!    scope; a CTE read arrives without schema name and passes unless its name is
//!    a stored object), recursive CTEs, and the
//!    functions in [`ALLOWED_FUNCTIONS`]. Everything else is denied — other
//!    tables (`chunks`, `llm_endpoints` with its keys, `sqlite_master`,
//!    `pragma_*` table functions), PRAGMA, ATTACH, every write and DDL,
//!    `load_extension` and any function not listed. The first denial is kept
//!    and becomes the error text, so the model learns what to change.
//! 3. **One statement**, and SQLite must call it read-only (`sqlite3_stmt_readonly`).
//! 4. **Time limit**: a progress handler aborts after `timeout` (a recursive
//!    CTE without end, a cross join over big tables).
//! 5. **Size limits**: strings/blobs ≤ [`MAX_VALUE_BYTES`] inside SQLite
//!    (stops `printf('%.*c', …)`/`replace` blow-ups); the result stops after
//!    `max_rows` rows or `max_chars` characters (truncation is reported).
//!
//! The result is CSV with a header line — compact for a model, exact numbers.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::limits::Limit;
use rusqlite::types::ValueRef;
use rusqlite::{Batch, Connection, OpenFlags};

use super::{CoreError, Result};

/// Largest string or blob SQLite builds inside a query (bytes).
pub const MAX_VALUE_BYTES: i32 = 1_000_000;
/// Longest SQL text accepted (characters).
pub const MAX_SQL_CHARS: usize = 8_000;
/// Longest single value written into the CSV (characters).
const MAX_CELL_CHARS: usize = 500;

/// SQL functions a query may call: aggregates, window functions, text, math and date helpers.
pub const ALLOWED_FUNCTIONS: &[&str] = &[
    "abs", "avg", "ceil", "ceiling", "char", "coalesce", "concat", "concat_ws", "count", "cume_dist", "date",
    "datetime", "dense_rank", "exp", "first_value", "floor", "format", "glob", "group_concat", "ifnull", "iif",
    "instr", "julianday", "lag", "last_value", "lead", "length", "like", "likely", "ln", "log", "log10", "log2",
    "lower", "ltrim", "max", "min", "mod", "nth_value", "ntile", "nullif", "percent_rank", "pi", "pow", "power",
    "printf", "rank", "replace", "round", "row_number", "rtrim", "sign", "sqrt", "strftime", "string_agg",
    "substr", "substring", "sum", "time", "timediff", "total", "trim", "trunc", "typeof", "unicode",
    "unixepoch", "unlikely", "upper",
];

/// Caps of one query.
#[derive(Debug, Clone)]
pub struct SheetQueryLimits {
    pub max_rows: usize,
    pub max_chars: usize,
    pub timeout: Duration,
}

impl Default for SheetQueryLimits {
    fn default() -> Self {
        Self { max_rows: 200, max_chars: 20_000, timeout: Duration::from_secs(2) }
    }
}

/// What a query returned.
#[derive(Debug, Clone, PartialEq)]
pub struct SheetQueryResult {
    /// Header line plus one line per returned row.
    pub csv: String,
    /// Rows in `csv`.
    pub rows: usize,
    /// More rows existed (or `max_chars` was hit) than `csv` holds.
    pub truncated: bool,
    /// The sheet tables the query read (lower case), for naming the sources.
    pub tables_read: Vec<String>,
}

/// What the authorizer saw: the first denial and the tables read.
#[derive(Default)]
struct AuthLog {
    denied: Option<String>,
    read: BTreeSet<String>,
}

fn deny(log: &Mutex<AuthLog>, why: String) -> Authorization {
    let mut l = log.lock().unwrap_or_else(|e| e.into_inner());
    l.denied.get_or_insert(why);
    Authorization::Deny
}

fn authorize(ctx: AuthContext<'_>, allowed: &HashSet<String>, stored: &HashSet<String>, log: &Mutex<AuthLog>) -> Authorization {
    match ctx.action {
        AuthAction::Select | AuthAction::Recursive => Authorization::Allow,
        AuthAction::Read { table_name, .. } => {
            let t = table_name.to_lowercase();
            // A read without a schema name that is no stored object is a CTE
            // (SQLite reports `COUNT(*)` over a CTE like that). A stored table
            // can come without schema name too (`COUNT(*)` over it), hence the name check.
            if ctx.database_name.is_none() && !stored.contains(&t) && !t.starts_with("sqlite_") && !t.starts_with("pragma_") {
                return Authorization::Allow;
            }
            if allowed.contains(&t) {
                log.lock().unwrap_or_else(|e| e.into_inner()).read.insert(t);
                Authorization::Allow
            } else {
                deny(log, format!("table '{table_name}' is not one of the spreadsheet tables available here"))
            }
        }
        AuthAction::Function { function_name } => {
            if ALLOWED_FUNCTIONS.contains(&function_name.to_lowercase().as_str()) {
                Authorization::Allow
            } else {
                deny(log, format!("function '{function_name}' is not allowed"))
            }
        }
        AuthAction::Pragma { pragma_name, .. } => deny(log, format!("PRAGMA {pragma_name} is not allowed")),
        AuthAction::Attach { .. } | AuthAction::Detach { .. } => deny(log, "ATTACH/DETACH is not allowed".into()),
        other => deny(log, format!("only SELECT queries are allowed (denied: {})", action_word(&other))),
    }
}

fn action_word(a: &AuthAction<'_>) -> &'static str {
    match a {
        AuthAction::Insert { .. } => "INSERT",
        AuthAction::Update { .. } => "UPDATE",
        AuthAction::Delete { .. } => "DELETE",
        AuthAction::DropTable { .. } | AuthAction::DropTempTable { .. } => "DROP TABLE",
        AuthAction::CreateTable { .. } | AuthAction::CreateTempTable { .. } => "CREATE TABLE",
        AuthAction::Transaction { .. } | AuthAction::Savepoint { .. } => "transaction control",
        _ => "schema or database change",
    }
}

/// Open the read-only connection with every fence installed.
fn open_fenced(db_path: &Path, allowed: &[String], log: Arc<Mutex<AuthLog>>) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.execute_batch("PRAGMA query_only = ON;")?;
    conn.set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_VALUE_BYTES);
    conn.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0);
    let allowed: HashSet<String> = allowed.iter().map(|t| t.to_lowercase()).collect();
    // Every stored object name, read before the authorizer exists (it would deny this read).
    let stored: HashSet<String> = {
        let mut stmt = conn.prepare("SELECT lower(name) FROM sqlite_master")?;
        let names: rusqlite::Result<HashSet<String>> = stmt.query_map([], |r| r.get::<_, String>(0))?.collect();
        names?
    };
    conn.authorizer(Some(move |ctx: AuthContext<'_>| authorize(ctx, &allowed, &stored, &log)));
    Ok(conn)
}

fn csv_field(s: &str) -> String {
    let s: String = if s.chars().count() > MAX_CELL_CHARS {
        format!("{}…", s.chars().take(MAX_CELL_CHARS).collect::<String>())
    } else {
        s.to_string()
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

fn value_text(v: ValueRef<'_>) -> String {
    match v {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => csv_field(&String::from_utf8_lossy(t)),
        ValueRef::Blob(b) => format!("<blob {} bytes>", b.len()),
    }
}

/// Map an SQLite error to the authorizer's reason or a readable message.
fn query_error(e: rusqlite::Error, log: &Mutex<AuthLog>, started: Instant, limits: &SheetQueryLimits) -> CoreError {
    if let Some(why) = log.lock().unwrap_or_else(|p| p.into_inner()).denied.clone() {
        return CoreError::InvalidState(format!("query rejected: {why}"));
    }
    if e.sqlite_error_code() == Some(rusqlite::ErrorCode::OperationInterrupted) || started.elapsed() >= limits.timeout {
        return CoreError::InvalidState(format!(
            "query stopped after the time limit of {} ms — simplify it (filter, aggregate, avoid unbounded recursion)",
            limits.timeout.as_millis()
        ));
    }
    CoreError::InvalidState(format!("query failed: {e}"))
}

/// Run one read-only `sql` against the tables in `allowed` of the database at `db_path`.
pub fn run_sheet_query(db_path: &Path, allowed: &[String], sql: &str, limits: &SheetQueryLimits) -> Result<SheetQueryResult> {
    let sql = sql.trim();
    if sql.is_empty() {
        return Err(CoreError::InvalidState("empty query".into()));
    }
    if sql.chars().count() > MAX_SQL_CHARS {
        return Err(CoreError::InvalidState(format!("query longer than {MAX_SQL_CHARS} characters")));
    }
    let log = Arc::new(Mutex::new(AuthLog::default()));
    let conn = open_fenced(db_path, allowed, log.clone())?;
    let started = Instant::now();
    let timeout = limits.timeout;
    conn.progress_handler(1_000, Some(move || started.elapsed() > timeout));
    let fail = |e: rusqlite::Error| query_error(e, &log, started, limits);

    let mut batch = Batch::new(&conn, sql);
    let mut stmt = batch.next().map_err(fail)?.ok_or_else(|| CoreError::InvalidState("empty query".into()))?;
    if batch.next().map_err(fail)?.is_some() {
        return Err(CoreError::InvalidState("send exactly one SELECT statement".into()));
    }
    if !stmt.readonly() {
        return Err(CoreError::InvalidState("only read-only SELECT queries are allowed".into()));
    }
    let names: Vec<String> = stmt.column_names().iter().map(|n| csv_field(n)).collect();
    let n_cols = names.len();
    let mut csv = names.join(",");
    let mut rows_out = 0usize;
    let mut truncated = false;
    let mut rows = stmt.query([]).map_err(fail)?;
    while let Some(row) = rows.next().map_err(fail)? {
        if rows_out >= limits.max_rows {
            truncated = true;
            break;
        }
        let line: Vec<String> = (0..n_cols).map(|i| row.get_ref(i).map(value_text).unwrap_or_default()).collect();
        let line = line.join(",");
        if csv.len() + line.len() + 1 > limits.max_chars {
            truncated = true;
            break;
        }
        csv.push('\n');
        csv.push_str(&line);
        rows_out += 1;
    }
    drop(rows);
    let tables_read = log.lock().unwrap_or_else(|p| p.into_inner()).read.iter().cloned().collect();
    Ok(SheetQueryResult { csv, rows: rows_out, truncated, tables_read })
}
