//! Pure logic of when and where things happened, shared by the scene/place search of media
//! (`handbuch/medien.md` „Ort, Zeit, Gerät“) and the planned media library with its timeline: the offline
//! place index (`place.rs`). No network, no files: the services crate fetches and opens the index file
//! (`services::places`).

pub mod place;

#[cfg(test)]
mod tests_props;
