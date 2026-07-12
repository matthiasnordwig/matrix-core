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
    /// BM25 keyword search over a context's **non-omitted** chunks. Returns up
    /// to `n` `(chunk_id, rank)` pairs, best match first, `rank` 1-based.
    ///
    /// `is_omitted = 0` mirrors the vector path (omitted chunks are never
    /// embedded, so vector search can't return them) and every other
    /// retrieval-facing query — without it, omitted rows (the FTS triggers index
    /// *all* chunks) could leak into hybrid results and be handed to the LLM.
    ///
    /// `doc_ids = Some(&[…])` restricts to those documents (the file-level scope,
    /// AP8) — the same filter that the vector path applies, so both lists are
    /// scoped identically **before** the caller's RRF fusion. An empty slice
    /// means "no documents in scope" → empty result.
    pub fn keyword_search_context(
        &self,
        context_id: i64,
        query: &str,
        n: usize,
        doc_ids: Option<&[i64]>,
    ) -> Result<Vec<(i64, usize)>> {
        if matches!(doc_ids, Some(d) if d.is_empty()) {
            return Ok(Vec::new());
        }
        let Some(match_expr) = escape_fts_query(query) else {
            return Ok(Vec::new());
        };
        fts_match_scoped(&self.conn, &match_expr, context_id, n, doc_ids)
    }

    /// Exact-phrase FTS5 search over a context's **non-omitted** chunks (AP8,
    /// the `lookup_exact` tool). Unlike `keyword_search_context` (which OR-fuses
    /// the terms bag-of-words style), this quotes the WHOLE query as a single
    /// FTS5 phrase so the terms must appear adjacent, in order — precisely what
    /// the model needs when following a cited norm ("Art. 33", "2016/679") whose
    /// exact string vector search blurs. Returns up to `n` `(chunk_id, rank)`
    /// pairs, best match first. `None` phrase (no usable tokens) → empty result.
    /// Same `doc_ids` file scope and `is_omitted = 0` guard as the keyword path.
    pub fn phrase_search_context(
        &self,
        context_id: i64,
        phrase: &str,
        n: usize,
        doc_ids: Option<&[i64]>,
    ) -> Result<Vec<(i64, usize)>> {
        if matches!(doc_ids, Some(d) if d.is_empty()) {
            return Ok(Vec::new());
        }
        let Some(match_expr) = escape_fts_phrase(phrase) else {
            return Ok(Vec::new());
        };
        fts_match_scoped(&self.conn, &match_expr, context_id, n, doc_ids)
    }
}

/// Run a prepared FTS5 MATCH (either the OR-fused keyword expr or a quoted
/// phrase) scoped to one context, non-omitted, best-first, optionally restricted
/// to a document set. Uses **anonymous** `?` placeholders throughout (mixing
/// `?N` with a bare `?` collapses SQLite's parameter numbering and undercounts
/// the binds), bound in textual order.
fn fts_match_scoped(
    conn: &rusqlite::Connection,
    match_expr: &str,
    context_id: i64,
    n: usize,
    doc_ids: Option<&[i64]>,
) -> Result<Vec<(i64, usize)>> {
    let mut sql = String::from(
        "SELECT c.id \
           FROM chunks_fts \
           JOIN chunks c ON c.id = chunks_fts.rowid \
          WHERE chunks_fts MATCH ? AND c.context_id = ? AND c.is_omitted = 0",
    );
    if let Some(allowed) = doc_ids {
        let placeholders = std::iter::repeat("?")
            .take(allowed.len())
            .collect::<Vec<_>>()
            .join(",");
        sql.push_str(&format!(" AND c.document_id IN ({placeholders})"));
    }
    sql.push_str(" ORDER BY bm25(chunks_fts) LIMIT ?");
    let mut stmt = conn.prepare(&sql)?;
    let n_i64 = n as i64;
    let mut binds: Vec<&dyn rusqlite::ToSql> = Vec::new();
    binds.push(&match_expr);
    binds.push(&context_id);
    // Doc-id binds come BEFORE the LIMIT bind, matching their textual position
    // (the IN clause precedes ORDER BY / LIMIT in the SQL above).
    if let Some(allowed) = doc_ids {
        for id in allowed {
            binds.push(id);
        }
    }
    binds.push(&n_i64);
    let rows = stmt.query_map(rusqlite::params_from_iter(binds), |row| row.get::<_, i64>(0))?;
    let mut out = Vec::new();
    for (i, id) in rows.enumerate() {
        out.push((id?, i + 1));
    }
    Ok(out)
}

/// Quote the ENTIRE query as one FTS5 phrase (terms must be adjacent/in order),
/// as opposed to `escape_fts_query`'s per-term OR fusion. Collapses internal
/// whitespace to single spaces and doubles embedded quotes. Returns `None` when
/// the query has no alphanumeric content (a lone "§" → an empty `""` phrase,
/// which FTS5 treats as match-all).
pub(crate) fn escape_fts_phrase(raw: &str) -> Option<String> {
    if !raw.chars().any(|c| c.is_alphanumeric()) {
        return None;
    }
    // Rebuild from whitespace-split tokens so tabs/newlines/runs collapse to one
    // space inside the phrase; double any embedded quote to keep it one string.
    let joined = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let escaped = joined.replace('"', "\"\"");
    Some(format!("\"{escaped}\""))
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
