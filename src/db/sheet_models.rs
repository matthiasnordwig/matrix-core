//! Row types of the spreadsheet tables (schema_v63, `sheet_tables.rs`,
//! `sheet_query.rs`). Re-exported from `models.rs` (so they are
//! `models::NewSheetTable` etc. like every other domain type); a file of their
//! own only because `models.rs` sits at its line cap.
//!
//! The ingest side (services `ingest/sheet/tables.rs`) fills [`NewSheetTable`]
//! with typed values; the database side names the columns and creates the
//! table. [`SheetTableInfo`] is a registry row as read back, with the column
//! list the query tool shows the model.

use serde::{Deserialize, Serialize};

/// SQL storage class of a sheet column (the column's declared type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum SheetColumnType {
    Integer,
    Real,
    Text,
}

impl SheetColumnType {
    pub fn sql(self) -> &'static str {
        match self {
            SheetColumnType::Integer => "INTEGER",
            SheetColumnType::Real => "REAL",
            SheetColumnType::Text => "TEXT",
        }
    }
}

/// One cell value as stored (empty cells and values of another type than the
/// column's are `Null`).
#[derive(Debug, Clone, PartialEq)]
pub enum SheetValue {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

/// A column of a table to create: the header as detected, its type, and an
/// optional note for the model (percent stored as fraction, ISO dates, …).
#[derive(Debug, Clone, PartialEq)]
pub struct NewSheetColumn {
    pub name: String,
    pub sql_type: SheetColumnType,
    /// What the values mean beyond the type, shown in the table description.
    pub note: Option<String>,
    /// One value as displayed in the sheet (e.g. `12%`, `1.234,50 €`).
    pub example: Option<String>,
}

/// A table region to store: rows hold one value per column, in column order;
/// `sheet_rows` (same length as `rows`) are the spreadsheet row numbers
/// (1-based, as in Excel) stored in the extra column `sheet_row`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewSheetTable {
    pub context_id: i64,
    pub document_id: i64,
    /// Sheet label as in the chunk metadata (`sheet`).
    pub sheet_name: String,
    /// Region number as in the chunk metadata (`region`, 0 = only region of the sheet).
    pub region_index: i64,
    pub columns: Vec<NewSheetColumn>,
    pub sheet_rows: Vec<i64>,
    pub rows: Vec<Vec<SheetValue>>,
}

/// One column of a stored table, as kept in `sheet_tables.columns_json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetColumnInfo {
    /// The header as detected in the sheet.
    pub name: String,
    /// The column name in SQL (sanitised, unique within the table).
    pub sql: String,
    #[serde(rename = "type")]
    pub sql_type: SheetColumnType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
}

/// A registry row of `sheet_tables` plus the document name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetTableInfo {
    pub id: i64,
    pub context_id: i64,
    pub document_id: i64,
    pub document_name: String,
    pub sheet_name: String,
    pub region_index: i64,
    pub table_name: String,
    /// The columns in table order, `sheet_row` first.
    pub columns: Vec<SheetColumnInfo>,
    pub row_count: i64,
}
