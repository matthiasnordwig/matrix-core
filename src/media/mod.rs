//! Pure logic of time-based documents (audio, video), without network or OS: the stored text of a
//! media document, clustering of voice vectors, and the analysis of video stills (scene changes, slide
//! or scene, the same slide over time, finding and straightening a document quadrilateral).
//! Decoding, models and the job live in the services crate (`media/`, `speech/`). Flow and decisions:
//! `handbuch/medien.md`.

pub mod diarize_cluster;
pub mod frame_kind;
pub mod quad;
pub mod rectify;
pub mod scenes;
pub mod slide_track;
pub mod text;
