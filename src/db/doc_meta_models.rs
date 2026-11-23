//! Types of the search filter by sender, date and folder (`doc_meta.rs`, schema_v75) and of a document's
//! time and origin (`doc_time.rs`, schema_v85), of its place and device and the filter on places and scene
//! labels (`doc_place.rs`, `scene_labels.rs`, schema_v86), re-exported
//! by `models.rs`.

use serde::{Deserialize, Serialize};

/// What a search is narrowed to besides collection and file. Every field is optional and they
/// combine with AND; text fields match anywhere, without regard to case. Dates are `YYYY`,
/// `YYYY-MM` or `YYYY-MM-DD` and both ends are included (`date_to: "2001"` = through 31 Dec).
/// Sender and date exist for mails and their attachments only, so a set sender or date leaves
/// every other document out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocFilter {
    #[serde(default)]
    pub sender: Option<String>,
    #[serde(default)]
    pub date_from: Option<String>,
    #[serde(default)]
    pub date_to: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
}

/// The filterable facts of one document as stored (`documents.sender`, `doc_date`, `folder`):
/// lowercased, date as `YYYY-MM-DD`, `None` when unknown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocMeta {
    pub sender: Option<String>,
    pub date: Option<String>,
    pub folder: Option<String>,
}

/// When and how a document came about (schema_v85, `doc_time.rs`): recording/creation time in UTC
/// milliseconds, the local-time offset then in minutes, where the time came from (`quicktime`,
/// `mp4_header`, `file_mtime`, `capture`, `mail`, …) and how it came into Matrix (`captured_in_matrix`,
/// `uploaded`, …). `None` = unknown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocTime {
    pub taken_at_ms: Option<i64>,
    pub tz_offset_min: Option<i32>,
    pub time_source: Option<String>,
    pub origin: Option<String>,
}

/// Where a document was made and with what (schema_v86, `doc_place.rs`): position in WGS 84 degrees
/// from the file's metadata, the GeoNames id of the nearest place once resolved, the camera or phone.
/// `None` = unknown.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DocPlace {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub place_id: Option<i64>,
    pub device: Option<String>,
}

/// The search filter on what pictures show and where documents were made (schema_v86), next to
/// [`DocFilter`]: documents at one of `place_ids` (resolved from a place name by the caller, which holds
/// the place index) and documents with a picture labelled with any of `scene` (`media/scene_labels.rs`
/// keys). Both combine with each other and with the [`DocFilter`] by AND.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PictureFilter {
    pub place_ids: Option<Vec<i64>>,
    pub scene: Vec<String>,
}

impl PictureFilter {
    pub fn is_empty(&self) -> bool {
        self.place_ids.is_none() && self.scene.is_empty()
    }
}
