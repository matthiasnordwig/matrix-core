//! Row types of the disk sources of a collection (schema_v71, `source_files.rs`):
//! which files and folders `add_files` of the stand-alone MCP program read, and
//! what each file looked like then. Re-exported from `models.rs` (a file of its
//! own only because `models.rs` sits at its line cap).

use serde::{Deserialize, Serialize};

/// A file or folder given to `add_files` (canonical path).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRoot {
    pub context_id: i64,
    pub path: String,
    pub is_dir: bool,
    pub recursive: bool,
    /// Last comparison with the disk (unix seconds); `None` = never.
    pub checked_at: Option<i64>,
}

/// The file a document was read from, as it was when read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    pub document_id: i64,
    pub context_id: i64,
    /// Canonical path.
    pub path: String,
    pub byte_size: i64,
    /// Modification time in nanoseconds since the Unix epoch.
    pub mtime_ns: i64,
    /// SHA-256 of the file's bytes, lower-case hex.
    pub sha256: String,
    /// Unix seconds of the check that first found the file gone; `None` = present.
    pub missing_since: Option<i64>,
}
