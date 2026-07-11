//! `chunk_refs` CRUD + ref derivation + retrieval expansion (schema_v49,
//! RETRIEVAL_QUALITY_PLAN.md AP2).
//!
//! One derivation routine (`derive_refs_for_chunk` / `insert_refs_for_chunk`)
//! is shared by the chunking-run finalization hook and the idempotent
//! `rebuild_chunk_refs(context_id)`. Resolution + expansion follows a hit's
//! outgoing refs to a target chunk, preferring the *definition site*.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Row};

use super::models::*;
use super::{Database, Result};
use crate::refs::parse_refs;

fn row_to_chunk_ref(row: &Row<'_>) -> rusqlite::Result<ChunkRef> {
    Ok(ChunkRef {
        id: row.get("id")?,
        chunk_id: row.get("chunk_id")?,
        context_id: row.get("context_id")?,
        ref_key: row.get("ref_key")?,
    })
}

impl Database {
    /// Replace a single chunk's outgoing refs with those freshly derived from
    /// its `text`. Idempotent: deletes the chunk's existing rows first, so
    /// re-running never duplicates. `context_id` is stored alongside so
    /// (context, ref_key) lookups don't need a join.
    pub fn set_chunk_refs(&self, chunk_id: i64, context_id: i64, text: &str) -> Result<usize> {
        self.conn
            .execute("DELETE FROM chunk_refs WHERE chunk_id = ?1", [chunk_id])?;
        // De-dup within a chunk (parse_refs already de-dups, but be defensive).
        let mut seen: HashSet<String> = HashSet::new();
        let mut n = 0usize;
        for r in parse_refs(text) {
            if !seen.insert(r.ref_key.clone()) {
                continue;
            }
            self.conn.execute(
                "INSERT INTO chunk_refs (chunk_id, context_id, ref_key) VALUES (?1, ?2, ?3)",
                params![chunk_id, context_id, r.ref_key],
            )?;
            n += 1;
        }
        Ok(n)
    }

    /// Derive + persist refs for every non-omitted chunk of `context_id`,
    /// replacing any existing rows for the context. Idempotent — a second run
    /// yields the same rows. Returns the number of ref rows inserted.
    pub fn rebuild_chunk_refs(&self, context_id: i64) -> Result<usize> {
        self.conn
            .execute("DELETE FROM chunk_refs WHERE context_id = ?1", [context_id])?;
        // Non-omitted only: omitted chunks are never retrievable (not embedded,
        // excluded from the FTS leg), so their outgoing refs would be dead rows.
        let chunks = self.list_chunks(context_id, false)?;
        let mut total = 0usize;
        for c in chunks {
            // set_chunk_refs deletes-then-inserts per chunk; the context-wide
            // delete above already cleared everything, so this just inserts.
            total += self.set_chunk_refs(c.id, context_id, &c.text)?;
        }
        Ok(total)
    }

    /// All outgoing ref rows of a chunk.
    pub fn chunk_refs_for_chunk(&self, chunk_id: i64) -> Result<Vec<ChunkRef>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM chunk_refs WHERE chunk_id = ?1 ORDER BY id")?;
        let rows = stmt.query_map([chunk_id], row_to_chunk_ref)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Chunks in the given contexts that *carry* `ref_key` (i.e. mention it in
    /// their own text), ordered by chunk_index so "earliest mention" is a
    /// stable tiebreak. Used by the resolver to find a ref's target chunk.
    pub fn chunks_with_ref(&self, context_ids: &[i64], ref_key: &str) -> Result<Vec<Chunk>> {
        if context_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat("?")
            .take(context_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT c.* FROM chunks c \
               JOIN chunk_refs r ON r.chunk_id = c.id \
              WHERE r.ref_key = ? AND c.context_id IN ({placeholders}) \
                AND c.is_omitted = 0 \
              ORDER BY c.document_id, c.chunk_index"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut binds: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(context_ids.len() + 1);
        binds.push(&ref_key);
        for cid in context_ids {
            binds.push(cid);
        }
        let rows = stmt.query_map(rusqlite::params_from_iter(binds), |row| {
            super::chunks::row_to_chunk(row)
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Resolve `ref_key` to a single best target chunk within `context_ids`,
    /// preferring the *definition site*: a chunk whose `signature` (structural
    /// section name) or text start carries the ref key's norm identity, else the
    /// earliest / most ref-dense mention. Returns `None` if nothing carries it.
    pub fn resolve_ref_target(&self, context_ids: &[i64], ref_key: &str) -> Result<Option<Chunk>> {
        let candidates = self.chunks_with_ref(context_ids, ref_key)?;
        if candidates.is_empty() {
            return Ok(None);
        }
        // Ref-density per candidate (how many outgoing refs it has) — a denser
        // chunk that also carries this ref is a better mention fallback.
        let mut density: HashMap<i64, usize> = HashMap::new();
        for c in &candidates {
            let d = self.chunk_refs_for_chunk(c.id)?.len();
            density.insert(c.id, d);
        }
        Ok(Some(pick_definition_site(candidates, ref_key, &density)))
    }
}

/// The literal token the ref_key denotes in prose (e.g. `KWG:§25a` → `§ 25a`,
/// `MARISK:AT4.3.2` → `AT 4.3.2`, `DORA:Art.28` → `Art. 28`). Used to test
/// whether a candidate chunk's signature/text-start is the *definition site*
/// of the norm rather than a mere in-body mention.
fn ref_key_surface(ref_key: &str) -> Option<String> {
    let (law, rest) = ref_key.split_once(':')?;
    if let Some(par) = rest.strip_prefix('§') {
        // "§25a" → "§ 25a"; the law is usually in the signature too.
        Some(format!("§ {par}"))
    } else if let Some(art) = rest.strip_prefix("Art.") {
        Some(format!("Art. {art}"))
    } else if law == "MARISK" {
        // "AT4.3.2" → "AT 4.3.2" (split the module letters from the clause).
        let split = rest
            .find(|c: char| c.is_ascii_digit())
            .map(|i| (&rest[..i], &rest[i..]));
        split.map(|(m, clause)| format!("{m} {clause}"))
    } else if law == "EU" {
        Some(rest.to_string())
    } else {
        None
    }
}

/// Normalize for a loose "starts-with / signature-carries" check: collapse
/// whitespace and lowercase.
fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Pure ranking: choose the best target chunk for a ref among `candidates`.
/// Prefers a definition site (signature or text-start carries the ref surface),
/// then the most ref-dense, then the earliest (candidates arrive ordered by
/// document/chunk_index, so the first is earliest). `density[chunk_id]` is the
/// number of outgoing refs of that chunk. `candidates` is non-empty.
pub(crate) fn pick_definition_site(
    candidates: Vec<Chunk>,
    ref_key: &str,
    density: &HashMap<i64, usize>,
) -> Chunk {
    let surface = ref_key_surface(ref_key).map(|s| norm(&s));

    // A chunk is a "definition site" if its signature carries the ref surface,
    // or its text *begins* with it (a mere mid-body mention does not count — that
    // would let a dense mention chunk masquerade as the definition).
    let is_def_site = |c: &Chunk| -> bool {
        let Some(surf) = &surface else { return false };
        if let Some(sig) = &c.signature {
            if norm(sig).contains(surf.as_str()) {
                return true;
            }
        }
        norm(&c.text).starts_with(surf.as_str())
    };

    // Score: (definition-site?, ref-density). Higher is better; ties fall to the
    // earliest candidate (stable because we keep the first max we encounter).
    let mut best_idx = 0usize;
    let mut best_key = (
        is_def_site(&candidates[0]),
        density.get(&candidates[0].id).copied().unwrap_or(0),
    );
    for (i, c) in candidates.iter().enumerate().skip(1) {
        let key = (is_def_site(c), density.get(&c.id).copied().unwrap_or(0));
        if key > best_key {
            best_key = key;
            best_idx = i;
        }
    }
    candidates.into_iter().nth(best_idx).expect("best_idx in range")
}

// --- Pure expansion (cap logic) ----------------------------------------------

/// An extra chunk added by ref-following, tagged with the 1-based position of
/// the primary hit that referenced it (provenance "referenced by [n]").
#[derive(Debug, Clone, PartialEq)]
pub struct ReferencedChunk {
    pub chunk_id: i64,
    /// 1-based index of the source hit in the primary top-k list.
    pub referenced_by: i64,
}

/// Pure ref-expansion ranking + capping. Given, per primary hit (in top-k
/// order), the ordered list of target chunk_ids its outgoing refs resolve to,
/// produce the extra chunks to append — obeying the LOCKED caps:
///   - at most **1** extra chunk per top-k hit, and
///   - at most **⌈top_k/2⌉** extra chunks total,
/// never re-adding a chunk already among the primary hits, and never adding the
/// same extra chunk twice. `resolved_per_hit[i]` are the candidate targets for
/// primary hit `i` (best-first); the first not-yet-used one is taken.
pub fn expand_with_refs(
    primary_ids: &[i64],
    resolved_per_hit: &[Vec<i64>],
    top_k: usize,
) -> Vec<ReferencedChunk> {
    let total_cap = top_k.div_ceil(2);
    let mut out: Vec<ReferencedChunk> = Vec::new();
    let mut used: HashSet<i64> = primary_ids.iter().copied().collect();

    for (i, targets) in resolved_per_hit.iter().enumerate() {
        if out.len() >= total_cap {
            break;
        }
        // At most one per hit: take the first target not already used.
        if let Some(&t) = targets.iter().find(|t| !used.contains(t)) {
            used.insert(t);
            out.push(ReferencedChunk { chunk_id: t, referenced_by: (i + 1) as i64 });
        }
    }
    out
}

#[cfg(test)]
#[path = "chunk_refs_tests.rs"]
mod chunk_refs_tests;
