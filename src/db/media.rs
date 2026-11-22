//! CRUD of the time-based documents (schema_v85): `media_documents` (kind, duration, speakers, phase
//! state for resuming) and `media_segments` (speech, subtitles, slides, scenes, sounds with their time
//! range). The segments are the expensive part and survive every re-cut; resuming drops only what an
//! interrupted phase wrote past its `done_ms` ([`Database::discard_media_segments_after`]).
//!
//! **Still → passage:** a still of a video (`page_images` with `t_start_ms`/`t_end_ms`) points with
//! `char_start/char_end` at the text of the passages of its time — the chunks whose `metadata.media`
//! time range overlaps the still's, or the nearest one in time — so an image hit lands on the passage
//! of the same moment (`page_select::chunks_for_page` maps by that range). [`Database::sync_frame_ranges`]
//! recomputes it from the stored chunks; [`Database::replace_media_document_text`] re-cuts a media
//! document and syncs in the same savepoint, so the ranges can never lag behind. Times of segments and
//! stills are never touched by a re-cut. Flow and invariants: `handbuch/medien.md`.

use rusqlite::{params, OptionalExtension, Row};

use super::models::*;
use super::{CoreError, Database, Result};
use crate::db::ontology::evidence_carry::EvidenceCarryReport;

fn json<T: serde::de::DeserializeOwned + Default>(s: String) -> T {
    serde_json::from_str(&s).unwrap_or_default()
}

fn row_to_media_document(r: &Row<'_>) -> rusqlite::Result<MediaDocument> {
    let kind: String = r.get(1)?;
    Ok(MediaDocument {
        document_id: r.get(0)?,
        kind: MediaKind::parse(&kind).unwrap_or_default(),
        duration_ms: r.get(2)?,
        width: r.get(3)?,
        height: r.get(4)?,
        language: r.get(5)?,
        engines: json(r.get(6)?),
        speakers: json(r.get(7)?),
        phases: json(r.get(8)?),
        complete: r.get(9)?,
    })
}

const SEGMENT_COLS: &str =
    "id, document_id, t_start_ms, t_end_ms, kind, speaker, text, words, language, confidence, page_image_id, generated";

fn row_to_segment(r: &Row<'_>) -> rusqlite::Result<MediaSegment> {
    let kind: String = r.get(4)?;
    let words: Option<String> = r.get(7)?;
    Ok(MediaSegment {
        id: r.get(0)?,
        document_id: r.get(1)?,
        t_start_ms: r.get(2)?,
        t_end_ms: r.get(3)?,
        kind: SegmentKind::parse(&kind).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, format!("unknown segment kind {kind}").into())
        })?,
        speaker: r.get(5)?,
        text: r.get(6)?,
        words: words.map(json).unwrap_or_default(),
        language: r.get(8)?,
        confidence: r.get(9)?,
        page_image_id: r.get(10)?,
        generated: r.get(11)?,
    })
}

/// A passage's position for [`frame_range`]: byte range in the document text and time range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedSpan {
    pub char_start: i64,
    pub char_end: i64,
    pub t0_ms: i64,
    pub t1_ms: i64,
}

fn overlaps(a0: i64, a1: i64, b0: i64, b1: i64) -> bool {
    let (lo, hi) = (a0.max(b0), a1.min(b1));
    lo < hi || (lo == hi && (a0 == a1 || b0 == b1))
}

/// The byte range a still from `t0..t1` points at: from the first to the last passage whose time
/// overlaps it, else the passage nearest in time (the earlier on a tie); `None` without passages.
pub fn frame_range(spans: &[TimedSpan], t0: i64, t1: i64) -> Option<(i64, i64)> {
    let hit = spans.iter().filter(|s| overlaps(s.t0_ms, s.t1_ms, t0, t1));
    let range = hit.fold(None, |acc: Option<(i64, i64)>, s| match acc {
        Some((a, b)) => Some((a.min(s.char_start), b.max(s.char_end))),
        None => Some((s.char_start, s.char_end)),
    });
    range.or_else(|| {
        let gap = |s: &TimedSpan| if s.t1_ms <= t0 { t0 - s.t1_ms } else { (s.t0_ms - t1).max(0) };
        spans.iter().min_by_key(|s| (gap(s), s.char_start)).map(|s| (s.char_start, s.char_end))
    })
}

impl Database {
    /// Stores the media row of a document (insert or overwrite every column).
    pub fn upsert_media_document(&self, m: &MediaDocument) -> Result<()> {
        self.conn.execute(
            "INSERT INTO media_documents
                (document_id, kind, duration_ms, width, height, language, engines, speakers, phases, complete)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(document_id) DO UPDATE SET kind = ?2, duration_ms = ?3, width = ?4, height = ?5,
                 language = ?6, engines = ?7, speakers = ?8, phases = ?9, complete = ?10",
            params![
                m.document_id,
                m.kind.as_str(),
                m.duration_ms,
                m.width,
                m.height,
                m.language,
                serde_json::to_string(&m.engines)?,
                serde_json::to_string(&m.speakers)?,
                serde_json::to_string(&m.phases)?,
                m.complete,
            ],
        )?;
        Ok(())
    }

    /// The media row of document `id`; `None` for a document that is not audio or video.
    pub fn media_document(&self, id: i64) -> Result<Option<MediaDocument>> {
        Ok(self
            .conn
            .query_row(
                "SELECT document_id, kind, duration_ms, width, height, language, engines, speakers, phases, complete
                   FROM media_documents WHERE document_id = ?1",
                [id],
                row_to_media_document,
            )
            .optional()?)
    }

    fn update_media<T>(&self, id: i64, f: impl FnOnce(&mut MediaDocument) -> T) -> Result<T> {
        self.in_savepoint("media_update", || {
            let mut m = self.media_document(id)?.ok_or_else(|| CoreError::NotFound(format!("media document {id}")))?;
            let out = f(&mut m);
            self.upsert_media_document(&m)?;
            Ok(out)
        })
    }

    /// Records how far phase `name` came (resuming starts at `done_ms`).
    pub fn set_media_phase(&self, id: i64, name: &str, phase: MediaPhase) -> Result<()> {
        self.update_media(id, |m| {
            m.phases.insert(name.to_string(), phase);
        })
    }

    /// Marks the whole file as processed (or not, after a stop).
    pub fn set_media_complete(&self, id: i64, complete: bool) -> Result<()> {
        self.update_media(id, |m| m.complete = complete)
    }

    /// Replaces the speaker list (renaming one; the passages are re-cut by the caller).
    pub fn set_media_speakers(&self, id: i64, speakers: &[MediaSpeaker]) -> Result<()> {
        self.update_media(id, |m| m.speakers = speakers.to_vec())
    }

    /// Records that `span.engine` wrote `span.phase` over its time range.
    pub fn add_media_engine(&self, id: i64, span: EngineSpan) -> Result<()> {
        self.update_media(id, |m| m.engines.push(span))
    }

    /// Stores segments (their `id` is ignored) in one savepoint; returns the new ids in order.
    pub fn insert_media_segments(&self, segments: &[MediaSegment]) -> Result<Vec<i64>> {
        self.in_savepoint("media_segments", || {
            let mut stmt = self.conn.prepare_cached(
                "INSERT INTO media_segments
                    (document_id, t_start_ms, t_end_ms, kind, speaker, text, words, language, confidence,
                     page_image_id, generated)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            let mut ids = Vec::with_capacity(segments.len());
            for s in segments {
                let words = if s.words.is_empty() { None } else { Some(serde_json::to_string(&s.words)?) };
                stmt.execute(params![
                    s.document_id,
                    s.t_start_ms,
                    s.t_end_ms,
                    s.kind.as_str(),
                    s.speaker,
                    s.text,
                    words,
                    s.language,
                    s.confidence,
                    s.page_image_id,
                    s.generated,
                ])?;
                ids.push(self.conn.last_insert_rowid());
            }
            Ok(ids)
        })
    }

    /// All segments of a document in time order (start, then id).
    pub fn media_segments(&self, document_id: i64) -> Result<Vec<MediaSegment>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SEGMENT_COLS} FROM media_segments WHERE document_id = ?1 ORDER BY t_start_ms, id"
        ))?;
        let rows = stmt.query_map([document_id], row_to_segment)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Resuming an interrupted phase: drops its segments (`kinds`) that end after `done_ms`, the part
    /// it will write again. Returns how many went.
    pub fn discard_media_segments_after(&self, document_id: i64, kinds: &[SegmentKind], done_ms: i64) -> Result<usize> {
        let mut n = 0;
        for k in kinds {
            n += self.conn.execute(
                "DELETE FROM media_segments WHERE document_id = ?1 AND kind = ?2 AND t_end_ms > ?3",
                params![document_id, k.as_str(), done_ms],
            )?;
        }
        Ok(n)
    }

    /// Points every still of document `id` at the passages of its time ([`frame_range`]) from the
    /// chunks stored now. Returns the number of stills updated.
    pub fn sync_frame_ranges(&self, document_id: i64) -> Result<usize> {
        let spans: Vec<TimedSpan> = {
            let mut stmt = self.conn.prepare(
                "SELECT char_start, char_end,
                        CAST(json_extract(metadata, '$.media.t0') AS INTEGER),
                        CAST(json_extract(metadata, '$.media.t1') AS INTEGER)
                   FROM chunks
                  WHERE document_id = ?1 AND is_omitted = 0 AND json_valid(metadata)
                    AND json_extract(metadata, '$.media.t0') IS NOT NULL
                  ORDER BY chunk_index",
            )?;
            let rows = stmt.query_map([document_id], |r| {
                Ok(TimedSpan { char_start: r.get(0)?, char_end: r.get(1)?, t0_ms: r.get(2)?, t1_ms: r.get::<_, Option<i64>>(3)?.unwrap_or(r.get(2)?) })
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let frames = self.frame_times(document_id)?;
        self.in_savepoint("frame_ranges", || {
            let mut stmt = self.conn.prepare_cached("UPDATE page_images SET char_start = ?2, char_end = ?3 WHERE id = ?1")?;
            for f in &frames {
                let r = frame_range(&spans, f.t_start_ms, f.t_end_ms);
                stmt.execute(params![f.page_image_id, r.map(|r| r.0), r.map(|r| r.1)])?;
            }
            Ok::<_, CoreError>(frames.len())
        })
    }

    /// Re-cut of a media document (new profile, renamed speaker, more segments after resuming): its
    /// text becomes `text`, its chunks `chunks` (graph evidence carried over by
    /// `replace_document_chunks`), and its stills follow ([`Self::sync_frame_ranges`]) — all in one
    /// savepoint. Returns the chunks inserted and the carry-over counts.
    pub fn replace_media_document_text(
        &self,
        document_id: i64,
        text: &str,
        chunks: &[NewChunk],
    ) -> Result<(usize, EvidenceCarryReport)> {
        self.replace_document_chunks(document_id, || {
            let n = self.conn.execute(
                "UPDATE documents SET extracted_text = ?2, byte_size = ?3 WHERE id = ?1",
                params![document_id, text, text.len() as i64],
            )?;
            if n == 0 {
                return Err(CoreError::NotFound(format!("document {document_id}")));
            }
            for c in chunks {
                self.create_chunk(c)?;
            }
            self.sync_frame_ranges(document_id)?;
            Ok(chunks.len())
        })
    }
}

/// Copies the media rows of document `src` to `dst` (context copy), after its page images were
/// copied: a segment's still is found again by its page number.
pub(super) fn copy_media_rows(conn: &rusqlite::Connection, src: i64, dst: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO media_documents
            (document_id, kind, duration_ms, width, height, language, engines, speakers, phases, complete)
         SELECT ?2, kind, duration_ms, width, height, language, engines, speakers, phases, complete
           FROM media_documents WHERE document_id = ?1",
        params![src, dst],
    )?;
    conn.execute(
        "INSERT INTO media_segments
            (document_id, t_start_ms, t_end_ms, kind, speaker, text, words, language, confidence, page_image_id, generated)
         SELECT ?2, s.t_start_ms, s.t_end_ms, s.kind, s.speaker, s.text, s.words, s.language, s.confidence,
                (SELECT n.id FROM page_images o JOIN page_images n ON n.document_id = ?2 AND n.page = o.page
                  WHERE o.id = s.page_image_id),
                s.generated
           FROM media_segments s WHERE s.document_id = ?1 ORDER BY s.t_start_ms, s.id",
        params![src, dst],
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod media_tests;
