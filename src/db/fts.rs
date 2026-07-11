//! FTS5 keyword search over `chunks(text)` (schema_v48).
//!
//! `keyword_search_context` returns BM25-ranked (chunk_id, 1-based rank) hits
//! scoped to one context. The user query is escaped defensively: FTS5 has its
//! own query grammar (`AND`/`OR`/`NEAR`/`*`/`§`/`.`/quotes all carry meaning),
//! so raw regulatory identifiers like "AT 4.3.2", "§ 25a KWG" or "Art. 28"
//! would otherwise be parsed as operators or raise a syntax error. We split the
//! raw query on whitespace and wrap **each term** as a double-quoted FTS5
//! string (doubling any embedded quote), then `OR`-join the quoted terms
//! (BM25 bag-of-words: any term may match, ranking rewards the rarer/identifier
//! terms). This treats every term as a literal phrase — robust, not a query
//! language. (Implicit AND was tried first and measured to regress hybrid eval;
//! see the `escape_fts_query` body / RETRIEVAL_QUALITY_PLAN AP1.)

use crate::{Database, Result};

/// Turn an arbitrary user query into a safe FTS5 MATCH expression. Returns
/// `None` when the query has no usable tokens (empty / whitespace only), in
/// which case the caller should short-circuit to an empty result.
pub(crate) fn escape_fts_query(raw: &str) -> Option<String> {
    let mut quoted: Vec<String> = Vec::new();
    for term in raw.split_whitespace() {
        // Drop terms that carry no alphanumeric content (e.g. a lone "§"),
        // which would otherwise become an empty "" phrase.
        if !term.chars().any(|c| c.is_alphanumeric()) {
            continue;
        }
        // Double embedded quotes so the term stays a single FTS5 string.
        let escaped = term.replace('"', "\"\"");
        quoted.push(format!("\"{escaped}\""));
    }
    if quoted.is_empty() {
        None
    } else {
        // `OR`-join the quoted phrases: BM25 bag-of-words retrieval (any term may
        // match, ranking rewards chunks carrying the rarer/identifier terms).
        // Implicit AND (space-join) was measured to REGRESS hybrid eval on
        // natural-language questions (it returns near-empty for some contexts and
        // noise for others — MRR 0.776→0.563); OR is the standard keyword-retrieval
        // semantics and is what RRF fusion expects. See RETRIEVAL_QUALITY_PLAN AP1.
        Some(quoted.join(" OR "))
    }
}

impl Database {
    /// BM25 keyword search over a context's chunks. Returns up to `n`
    /// `(chunk_id, rank)` pairs, best match first, `rank` 1-based.
    pub fn keyword_search_context(
        &self,
        context_id: i64,
        query: &str,
        n: usize,
    ) -> Result<Vec<(i64, usize)>> {
        let Some(match_expr) = escape_fts_query(query) else {
            return Ok(Vec::new());
        };
        // Join the FTS rowid to chunks.id and filter by context. bm25() is
        // ascending (lower = better), so ORDER BY bm25 gives best-first.
        let mut stmt = self.conn.prepare(
            "SELECT c.id \
               FROM chunks_fts \
               JOIN chunks c ON c.id = chunks_fts.rowid \
              WHERE chunks_fts MATCH ?1 AND c.context_id = ?2 \
              ORDER BY bm25(chunks_fts) \
              LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![match_expr, context_id, n as i64],
            |row| row.get::<_, i64>(0),
        )?;
        let mut out = Vec::new();
        for (i, id) in rows.enumerate() {
            out.push((id?, i + 1));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::escape_fts_query;

    #[test]
    fn escapes_identifiers_and_quotes() {
        assert_eq!(escape_fts_query("AT 4.3.2").unwrap(), "\"AT\" OR \"4.3.2\"");
        assert_eq!(escape_fts_query("§ 25a KWG").unwrap(), "\"25a\" OR \"KWG\"");
        assert_eq!(escape_fts_query("Art. 28").unwrap(), "\"Art.\" OR \"28\"");
        assert_eq!(
            escape_fts_query("a \"weird\" one").unwrap(),
            "\"a\" OR \"\"\"weird\"\"\" OR \"one\""
        );
    }

    #[test]
    fn empty_or_symbol_only_is_none() {
        assert!(escape_fts_query("").is_none());
        assert!(escape_fts_query("   ").is_none());
        assert!(escape_fts_query("§ — ()").is_none());
    }
}
