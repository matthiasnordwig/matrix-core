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
    /// section name), text start, or `metadata.section` (structural chunker's
    /// heading field) carries the ref key's norm identity, else the earliest /
    /// most ref-dense mention. Returns `None` if nothing carries it.
    ///
    /// EU-bound article keys (`EU:2013/575:Art.395`) get a second candidate
    /// source: the regulation's own text writes just "Artikel 395" (no Kürzel,
    /// no long form), so its definition chunk never *carries* the compound key.
    /// `eu_article_def_candidates` adds definition-shaped chunks from the
    /// regulation's own document(s) — see there for the precision constraints.
    pub fn resolve_ref_target(&self, context_ids: &[i64], ref_key: &str) -> Result<Option<Chunk>> {
        let mut candidates = self.chunks_with_ref(context_ids, ref_key)?;
        if let Some((base_key, art)) = split_eu_article_key(ref_key) {
            let reg = self.eu_article_def_candidates(context_ids, &base_key, &art)?;
            if !reg.is_empty() {
                // The regulation itself is ingested: its definition-shaped
                // chunks are strictly better targets than any citing-law
                // mention (whose first-line signature may also carry
                // "Artikel N", and whose expanded range refs inflate density)
                // → pick exclusively among the regulation's own chunks.
                candidates = reg;
            }
        }
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

    /// Definition-shaped chunks for an EU-bound article ref, taken from the
    /// regulation's own document(s). Two precision gates:
    ///   1. **Document identity:** only documents whose *early* chunks
    ///      (chunk_index ≤ 2, i.e. the title page) carry the base regulation
    ///      ref (`EU:2013/575`) count as "the regulation itself" — a law that
    ///      merely cites the regulation somewhere in its body does not qualify.
    ///   2. **Definition shape:** within those documents only chunks whose
    ///      signature carries `Art./Artikel N` at a word boundary, whose text
    ///      *begins* with it, or whose `metadata.section` carries it (the
    ///      structural chunker's article heading, e.g. `"Artikel 395 (1)"` —
    ///      the CRR PDF puts it there and nowhere else, found 2026-07-12) are
    ///      returned — mere in-body mentions are not, so this can never flood
    ///      the candidate set.
    fn eu_article_def_candidates(
        &self,
        context_ids: &[i64],
        base_key: &str,
        art: &str,
    ) -> Result<Vec<Chunk>> {
        if context_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat("?")
            .take(context_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        // SQL pre-filter: regulation documents (gate 1) + a cheap `LIKE` on the
        // article number; the exact word-boundary/definition-shape check (gate
        // 2) happens in Rust below (LIKE can't collapse whitespace). `metadata`
        // is included alongside `text`/`signature` because structural chunkers
        // (e.g. the CRR PDF) put the article heading in `metadata.section`
        // only — a chunk whose article number lives solely there would
        // otherwise never survive this prefilter (found 2026-07-12).
        let sql = format!(
            "SELECT c.* FROM chunks c \
              WHERE c.context_id IN ({placeholders}) \
                AND c.is_omitted = 0 \
                AND c.document_id IN ( \
                      SELECT DISTINCT c2.document_id \
                        FROM chunk_refs r JOIN chunks c2 ON c2.id = r.chunk_id \
                       WHERE r.ref_key = ? AND c2.chunk_index <= 2) \
                AND (c.text LIKE '%' || ? || '%' \
                     OR c.signature LIKE '%' || ? || '%' \
                     OR c.metadata LIKE '%' || ? || '%') \
              ORDER BY c.document_id, c.chunk_index"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut binds: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(context_ids.len() + 4);
        for cid in context_ids {
            binds.push(cid);
        }
        binds.push(&base_key);
        binds.push(&art);
        binds.push(&art);
        binds.push(&art);
        let rows = stmt.query_map(rusqlite::params_from_iter(binds), |row| {
            super::chunks::row_to_chunk(row)
        })?;
        let surfaces = [norm(&format!("art. {art}")), norm(&format!("artikel {art}"))];
        let out = rows
            .collect::<rusqlite::Result<Vec<Chunk>>>()?
            .into_iter()
            .filter(|c| {
                let sig_hit = c.signature.as_deref().is_some_and(|sig| {
                    let n = norm(sig);
                    surfaces.iter().any(|s| contains_bounded(&n, s))
                });
                let text_start_hit = {
                    let n = norm(&c.text);
                    surfaces
                        .iter()
                        .any(|s| n.starts_with(s.as_str()) && contains_bounded(&n, s))
                };
                // Structural chunkers (e.g. the CRR PDF) put the article
                // heading in `metadata.section`, not in `signature` or at the
                // text start (a section-final chunk starts mid-sentence, e.g.
                // "(1) Ein Institut hält ..."). Without this, EU-bound
                // articles from structurally chunked regulations never
                // qualify as a definition site — found 2026-07-12.
                let section_hit = super::chunks::chunk_section(c).is_some_and(|section| {
                    let n = norm(&section);
                    surfaces.iter().any(|s| contains_bounded(&n, s))
                });
                sig_hit || text_start_hit || section_hit
            })
            .collect();
        Ok(out)
    }
}

/// Split an EU-bound article key (`EU:2013/575:Art.395`) into the base
/// regulation key (`EU:2013/575`) and the article part (`395`). `None` for all
/// other key shapes.
fn split_eu_article_key(ref_key: &str) -> Option<(String, String)> {
    let rest = ref_key.strip_prefix("EU:")?;
    let (reg, art) = rest.split_once(":Art.")?;
    if reg.is_empty() || art.is_empty() {
        return None;
    }
    Some((format!("EU:{reg}"), art.to_string()))
}

/// The literal surface(s) the ref_key denotes in prose (e.g. `KWG:§25a` →
/// `§ 25a`, `MARISK:AT4.3.2` → `AT 4.3.2`, `DORA:Art.28` → `Art. 28`). Used to
/// test whether a candidate chunk's signature/text-start is the *definition
/// site* of the norm rather than a mere in-body mention. Returns all normalized
/// surfaces that should count as a match — most refs have exactly one, but an
/// EU regulation has two: the normalized `YYYY/NNNN` key **and** the legacy
/// `NNN/YYYY` order ("Nr. 575/2013"), so pre-2015 regulations resolve too.
fn ref_key_surfaces(ref_key: &str) -> Vec<String> {
    let Some((law, rest)) = ref_key.split_once(':') else { return Vec::new() };
    let out = if let Some(par) = rest.strip_prefix('§') {
        // "§25a" → "§ 25a"; the law is usually in the signature too.
        vec![format!("§ {par}")]
    } else if let Some(art) = rest.strip_prefix("Art.") {
        vec![format!("Art. {art}")]
    } else if law == "MARISK" {
        // "AT4.3.2" → "AT 4.3.2" (split the module letters from the clause).
        rest.find(|c: char| c.is_ascii_digit())
            .map(|i| vec![format!("{} {}", &rest[..i], &rest[i..])])
            .unwrap_or_default()
    } else if law == "EU" {
        if let Some((_reg, art)) = rest.split_once(":Art.") {
            // EU-bound article (`EU:2013/575:Art.395`): the regulation's own
            // text writes just "Artikel 395" / "Art. 395" (no Kürzel) — both
            // prose forms are surfaces.
            vec![format!("Art. {art}"), format!("Artikel {art}")]
        } else {
            // `2013/575` → also the legacy source order `575/2013` (as in
            // "Verordnung (EU) Nr. 575/2013"), so pre-2015 regs are not a no-op.
            let mut v = vec![rest.to_string()];
            if let Some((a, b)) = rest.split_once('/') {
                let legacy = format!("{b}/{a}");
                if legacy != rest {
                    v.push(legacy);
                }
            }
            v
        }
    } else {
        Vec::new()
    };
    out.into_iter().map(|s| norm(&s)).collect()
}

/// Normalize for a loose "starts-with / signature-carries" check: collapse
/// whitespace and lowercase.
fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// True if `needle` occurs in `haystack` at a **word boundary on the right**:
/// the character right after the match must not continue the norm token. This
/// stops `§ 25` from matching `§ 25a` and `at 4.3` from matching `at 4.3.2` —
/// a following ASCII-alphanumeric extends the identity, and `.`/`/`/`-` extend
/// it when themselves followed by an alphanumeric (`4.3.2`, `25-30`). A bare
/// trailing sentence period ("… Artikel 395.") does NOT disqualify.
/// Both strings are assumed already `norm`-alized (lowercased, single spaces).
fn contains_bounded(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let hb = haystack.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = haystack[from..].find(needle) {
        let start = from + rel;
        // Surfaces end in ASCII (digit/letter), so `after` is a char boundary.
        let after = start + needle.len();
        let ok_right = match hb.get(after) {
            None => true,
            Some(&c) if c.is_ascii_alphanumeric() => false,
            Some(&c) if c == b'.' || c == b'/' || c == b'-' => {
                !hb.get(after + 1).is_some_and(|d| d.is_ascii_alphanumeric())
            }
            _ => true,
        };
        // Left side: the surfaces start with `§`/`art.`/module letters/digits and
        // are preceded by whitespace or start-of-string in practice; we only need
        // the right boundary to reject prefix collisions like 25 vs 25a.
        if ok_right {
            return true;
        }
        // Advance past this match (a char boundary) to keep scanning.
        from = after;
        if from >= haystack.len() {
            break;
        }
    }
    false
}

/// Extract a candidate's law abbreviation from a `§ N`-style ref_key
/// (`KWG:§25a` → `kwg`). Only paragraph/article keys carry a meaningful Kürzel;
/// EU/MaRisk keys return `None` (their identity is the number/module itself).
fn ref_kuerzel(ref_key: &str) -> Option<String> {
    let (law, rest) = ref_key.split_once(':')?;
    if rest.starts_with('§') || rest.starts_with("Art.") {
        Some(law.to_lowercase())
    } else {
        None
    }
}

/// True if a *conflicting* known law Kürzel sits right after `surf` in `text`.
/// The Kürzel of a paragraph/article ref (e.g. `kwg`) must not be contradicted
/// by an explicit different act name in the candidate (a `§ 25a VAG` signature
/// must NOT satisfy `KWG:§25a`). Semantics: only an *explicitly present,
/// different* known Kürzel disqualifies — chunks of the same law that just write
/// "§ 25a" without repeating the Kürzel (the common case inside a law's own
/// document) stay valid. `text` is `norm`-alized; `own` is the ref's own Kürzel.
fn kuerzel_conflicts(text: &str, surf: &str, own: &str) -> bool {
    // Look at the first word following the surface occurrence; if it is a known
    // law abbrev different from `own`, this is a different act → conflict.
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(surf) {
        let after = from + rel + surf.len();
        let tail = text[after..].trim_start();
        // First whitespace-delimited token after the norm.
        let word: String = tail
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        if !word.is_empty()
            && word != own
            && crate::refs::is_known_law_abbrev_public(&word)
        {
            return true;
        }
        from = after; // past the matched surface (a char boundary)
        if from >= text.len() {
            break;
        }
    }
    false
}

/// Pure ranking: choose the best target chunk for a ref among `candidates`.
/// The EARLIEST definition site wins (signature, text-start, or
/// `metadata.section` carries the ref surface; candidates arrive ordered by
/// document/chunk_index, so the first def site is the article's opening
/// chunk, not a citation-heavy later Absatz). Without any def site: the most
/// ref-dense mention, ties to the earliest. `density[chunk_id]` is the number
/// of outgoing refs of that chunk. `candidates` is non-empty.
pub(crate) fn pick_definition_site(
    candidates: Vec<Chunk>,
    ref_key: &str,
    density: &HashMap<i64, usize>,
) -> Chunk {
    let surfaces = ref_key_surfaces(ref_key);
    let own_kuerzel = ref_kuerzel(ref_key);

    // A chunk is a "definition site" if its signature carries the ref surface at
    // a word boundary (so `§ 25` does not match `§ 25a`), or its text *begins*
    // with it, or its `metadata.section` carries it — structural chunkers
    // (e.g. the CRR PDF) put the article heading there instead of in
    // signature/text-start, since a section-final chunk can start mid-sentence
    // (found 2026-07-12) — AND no conflicting different law Kürzel sits next to
    // it (a `§ 25a VAG` signature must not satisfy a `KWG:§25a` ref). A mere
    // mid-body mention does not count — that would let a dense mention chunk
    // masquerade as the definition.
    let carries = |hay: &str, at_start: bool| -> bool {
        surfaces.iter().any(|surf| {
            let matched = if at_start {
                hay.starts_with(surf.as_str())
                    // start-anchored still needs a right boundary (§ 25 vs § 25a).
                    && contains_bounded(hay, surf)
            } else {
                contains_bounded(hay, surf)
            };
            if !matched {
                return false;
            }
            match &own_kuerzel {
                Some(k) => !kuerzel_conflicts(hay, surf, k),
                None => true,
            }
        })
    };
    let is_def_site = |c: &Chunk| -> bool {
        if surfaces.is_empty() {
            return false;
        }
        if let Some(sig) = &c.signature {
            if carries(&norm(sig), false) {
                return true;
            }
        }
        if carries(&norm(&c.text), true) {
            return true;
        }
        if let Some(section) = super::chunks::chunk_section(c) {
            if carries(&norm(&section), false) {
                return true;
            }
        }
        false
    };

    // Among DEFINITION SITES the earliest wins outright — since the
    // metadata.section check (2026-07-12) a multi-chunk article yields several
    // def-shaped candidates (one per Absatz chunk, all sharing "Artikel N …"
    // sections); ref-density must not pick a citation-heavy later Absatz over
    // the article's opening chunk (measured: EU:2013/575:Art.395 resolved to
    // the "(2)" chunk because it cites EBA regulations, instead of the "(1)"
    // opening the section-completion consumer needs). Candidates arrive
    // ordered by document/chunk_index, so the first def site IS the earliest.
    // Without any def site, fall back to the most ref-dense mention, ties to
    // the earliest (unchanged).
    if let Some(pos) = candidates.iter().position(is_def_site) {
        return candidates.into_iter().nth(pos).expect("pos in range");
    }
    let mut best_idx = 0usize;
    let mut best_density = density.get(&candidates[0].id).copied().unwrap_or(0);
    for (i, c) in candidates.iter().enumerate().skip(1) {
        let d = density.get(&c.id).copied().unwrap_or(0);
        if d > best_density {
            best_density = d;
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
