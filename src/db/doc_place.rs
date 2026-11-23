//! Where a document was made and with what (schema_v86): `documents.lat`, `lon`, `place_id`, `device` as one
//! [`DocPlace`]. Written by whoever reads the file's own metadata (QuickTime/MP4 location of a video, EXIF
//! GPS of a photo; services `ingest/meta/`); `place_id` (GeoNames id of the nearest place, core
//! `timeline/place.rs`) is filled in once the place index is at hand, possibly later than the position
//! ([`Database::unplaced_documents`]).
//!
//! The search filter on places and scene labels ([`Database::narrow_scope_with`]) narrows like the sender/date/
//! folder filter (`doc_meta.rs`): to document ids the retrieval's `doc_ids` scope takes. Both read only
//! indexes (`idx_documents_place`, `idx_scene_labels_doc`) and never the text of a document.

use std::collections::HashSet;

use rusqlite::{params, OptionalExtension};

use super::models::{DocFilter, DocPlace, PictureFilter};
use super::{json_ids, Database, Result};

impl Database {
    /// Stores position, place and device of document `id` (all four columns; `None` clears one).
    pub fn set_document_place(&self, id: i64, p: &DocPlace) -> Result<()> {
        self.conn.execute(
            "UPDATE documents SET lat = ?2, lon = ?3, place_id = ?4, device = ?5 WHERE id = ?1",
            params![id, p.lat, p.lon, p.place_id, p.device],
        )?;
        Ok(())
    }

    /// Position, place and device of document `id`; `None` when there is no such document.
    pub fn document_place(&self, id: i64) -> Result<Option<DocPlace>> {
        Ok(self
            .conn
            .query_row("SELECT lat, lon, place_id, device FROM documents WHERE id = ?1", [id], |r| {
                Ok(DocPlace { lat: r.get(0)?, lon: r.get(1)?, place_id: r.get(2)?, device: r.get(3)? })
            })
            .optional()?)
    }

    /// Sets the nearest place of document `id` (its position stays).
    pub fn set_document_place_id(&self, id: i64, place_id: Option<i64>) -> Result<()> {
        self.conn.execute("UPDATE documents SET place_id = ?2 WHERE id = ?1", params![id, place_id])?;
        Ok(())
    }

    /// Up to `limit` documents with a position but no place yet: `(id, lat, lon)`, oldest first.
    pub fn unplaced_documents(&self, limit: usize) -> Result<Vec<(i64, f64, f64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, lat, lon FROM documents INDEXED BY idx_documents_unplaced
              WHERE lat IS NOT NULL AND place_id IS NULL ORDER BY id LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The places documents of `context_ids` were made at (GeoNames ids, each once, ascending).
    pub fn context_place_ids(&self, context_ids: &[i64]) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT place_id FROM documents INDEXED BY idx_documents_place
              WHERE context_id IN (SELECT value FROM json_each(?1)) AND place_id IS NOT NULL ORDER BY place_id",
        )?;
        let rows = stmt.query_map([json_ids(context_ids)], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Documents of `context_ids` made at one of `place_ids`.
    pub fn documents_at_places(&self, context_ids: &[i64], place_ids: &[i64]) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM documents INDEXED BY idx_documents_place
              WHERE context_id IN (SELECT value FROM json_each(?1)) AND place_id IN (SELECT value FROM json_each(?2))
              ORDER BY id",
        )?;
        let rows = stmt.query_map([json_ids(context_ids), json_ids(place_ids)], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The search scope with both filters applied: [`Database::narrow_scope`] for `filter`, then only the
    /// documents `pictures` lets through (at one of its places, with a picture of one of its labels).
    /// `doc_ids` unchanged when both filters are empty.
    pub fn narrow_scope_with(
        &self,
        context_ids: &[i64],
        doc_ids: Option<Vec<i64>>,
        filter: &DocFilter,
        pictures: &PictureFilter,
    ) -> Result<Option<Vec<i64>>> {
        let mut scope = self.narrow_scope(context_ids, doc_ids, filter)?;
        let mut keep = |allowed: Vec<i64>| {
            let allowed: HashSet<i64> = allowed.into_iter().collect();
            scope = Some(match scope.take() {
                Some(mut ids) => {
                    ids.retain(|id| allowed.contains(id));
                    ids
                }
                None => {
                    let mut ids: Vec<i64> = allowed.into_iter().collect();
                    ids.sort_unstable();
                    ids
                }
            });
        };
        if let Some(places) = &pictures.place_ids {
            keep(self.documents_at_places(context_ids, places)?);
        }
        if !pictures.scene.is_empty() {
            keep(self.documents_with_scene(context_ids, &pictures.scene)?);
        }
        Ok(scope)
    }
}
