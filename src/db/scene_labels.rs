//! Scene labels of pictures (schema_v86): `scene_labels` rows per page image, label and source (`vectors` from
//! the image vector of the collection's model, rule in `media/scene_labels.rs`; `vision` from Apple's image
//! classification), and `scene_label_state`, which says what has been labelled so a picture without any
//! label is not tried again. Labelled are a video's scene stills (segments of kind `scene`,
//! [`Database::scene_still_ids`]) and the pictures of image documents ([`Database::photo_picture_ids`]); never
//! PDF graphics pages or slides.
//! Vectors of another model than the one the labels came from count as unlabelled, so switching the
//! model relabels.

use rusqlite::params;

use super::embeddings::blob_to_vector;
use super::{json_ids, Database, Result};

/// The two sources of labels.
pub const SOURCE_VECTORS: &str = "vectors";
pub const SOURCE_VISION: &str = "vision";

impl Database {
    /// Page images of the scene stills of the videos in `context_id`.
    pub fn scene_still_ids(&self, context_id: i64) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.page_image_id FROM media_segments s JOIN documents d ON d.id = s.document_id
              WHERE d.context_id = ?1 AND s.kind = 'scene' AND s.page_image_id IS NOT NULL ORDER BY s.page_image_id",
        )?;
        let rows = stmt.query_map([context_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Page images of the photos in `context_id`: the picture of an image document (a page image without a time
    /// range of a document without pages — PDFs, whose graphics pages are page images too, have a page count).
    pub fn photo_picture_ids(&self, context_id: i64) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id FROM page_images p JOIN documents d ON d.id = p.document_id
              WHERE p.context_id = ?1 AND p.t_start_ms IS NULL AND d.page_count IS NULL ORDER BY p.id",
        )?;
        let rows = stmt.query_map([context_id], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Of `page_image_ids`, those with a vector of `model_id` not yet labelled from it: `(page image, document,
    /// vector)`.
    pub fn pictures_to_label_by_vectors(&self, page_image_ids: &[i64], model_id: i64) -> Result<Vec<(i64, i64, Vec<f32>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.page_image_id, p.document_id, e.vector FROM page_image_embeddings e
               JOIN page_images p ON p.id = e.page_image_id
               LEFT JOIN scene_label_state s ON s.page_image_id = e.page_image_id
              WHERE e.page_image_id IN (SELECT value FROM json_each(?1)) AND e.embedding_model_id = ?2
                AND (s.vectors_model_id IS NULL OR s.vectors_model_id != ?2)
              ORDER BY e.page_image_id",
        )?;
        let rows = stmt.query_map(params![json_ids(page_image_ids), model_id], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, Vec<u8>>(2)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (id, doc, blob) = row?;
            out.push((id, doc, blob_to_vector(&blob)?));
        }
        Ok(out)
    }

    /// Of `page_image_ids`, those Vision has not looked at yet: `(page image, document)`.
    pub fn pictures_to_label_by_vision(&self, page_image_ids: &[i64]) -> Result<Vec<(i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.document_id FROM page_images p LEFT JOIN scene_label_state s ON s.page_image_id = p.id
              WHERE p.id IN (SELECT value FROM json_each(?1)) AND COALESCE(s.vision, 0) = 0 ORDER BY p.id",
        )?;
        let rows = stmt.query_map([json_ids(page_image_ids)], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Replaces the labels of one picture from `source` ([`SOURCE_VECTORS`] with the `model_id` they came from,
    /// or [`SOURCE_VISION`]) and marks it as labelled from that source, in one savepoint.
    pub fn set_scene_labels(&self, page_image_id: i64, document_id: i64, source: &str, labels: &[&str], model_id: Option<i64>) -> Result<()> {
        self.in_savepoint("scene_labels", || {
            self.conn.execute("DELETE FROM scene_labels WHERE page_image_id = ?1 AND source = ?2", params![page_image_id, source])?;
            for label in labels {
                self.conn.execute(
                    "INSERT OR IGNORE INTO scene_labels (page_image_id, document_id, source, label) VALUES (?1, ?2, ?3, ?4)",
                    params![page_image_id, document_id, source, label],
                )?;
            }
            self.conn.execute("INSERT OR IGNORE INTO scene_label_state (page_image_id) VALUES (?1)", [page_image_id])?;
            if source == SOURCE_VISION {
                self.conn.execute("UPDATE scene_label_state SET vision = 1 WHERE page_image_id = ?1", [page_image_id])?;
            } else {
                self.conn.execute("UPDATE scene_label_state SET vectors_model_id = ?2 WHERE page_image_id = ?1", params![page_image_id, model_id])?;
            }
            Ok(())
        })
    }

    /// The labels of the pictures of document `document_id` as `(page image, source, label)`; with `during`
    /// (ms) only of the stills visible then (a passage of a video), else of all its pictures.
    pub fn scene_labels_of(&self, document_id: i64, during: Option<(i64, i64)>) -> Result<Vec<(i64, String, String)>> {
        let (from, to) = during.unwrap_or((i64::MIN, i64::MAX));
        let mut stmt = self.conn.prepare(
            "SELECT l.page_image_id, l.source, l.label FROM scene_labels l JOIN page_images p ON p.id = l.page_image_id
              WHERE l.document_id = ?1
                AND (?2 = 0 OR (p.t_start_ms IS NOT NULL AND p.t_start_ms < ?4 AND COALESCE(p.t_end_ms, p.t_start_ms) > ?3))
              ORDER BY l.page_image_id, l.source, l.label",
        )?;
        let rows = stmt.query_map(params![document_id, during.is_some(), from, to], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Documents of `context_ids` with a picture labelled with any of `labels` (any source).
    pub fn documents_with_scene(&self, context_ids: &[i64], labels: &[String]) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT l.document_id FROM scene_labels l JOIN documents d ON d.id = l.document_id
              WHERE l.label IN (SELECT value FROM json_each(?2)) AND d.context_id IN (SELECT value FROM json_each(?1))
              ORDER BY l.document_id",
        )?;
        let labels = serde_json::to_string(labels).unwrap_or_else(|_| "[]".into());
        let rows = stmt.query_map([json_ids(context_ids), labels], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}
