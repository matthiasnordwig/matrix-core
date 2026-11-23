//! `media.rs`: media rows with their JSON columns, segments in time order, resuming without doubles,
//! cascades, the still → passage range (`frame_range`) and that it follows every re-cut
//! (`replace_media_document_text`) while the times of stills stay as they were; context copy.

use super::*;
use crate::db::Database;
use crate::embedding::page_select::{chunks_for_page, ChunkSpan};

fn setup() -> (Database, i64, i64) {
    let db = Database::open_in_memory().unwrap();
    let ctx = db.create_context(&NewContext { name: "media".into(), chunking_strategy: "structural".into(), ..Default::default() }).unwrap().id;
    let doc = db
        .create_document(&NewDocument { context_id: ctx, name: "talk.mp4".into(), zip_entry: None, byte_size: None, page_count: None, content_hash: None, extracted_text: Some(String::new()) })
        .unwrap()
        .id;
    (db, ctx, doc)
}

fn media(doc: i64) -> MediaDocument {
    MediaDocument { document_id: doc, kind: MediaKind::Video, duration_ms: 60_000, width: Some(1280), height: Some(720), language: Some("de".into()), ..Default::default() }
}

fn seg(doc: i64, kind: SegmentKind, t0: i64, t1: i64, text: &str) -> MediaSegment {
    MediaSegment { document_id: doc, kind, t_start_ms: t0, t_end_ms: t1, text: text.into(), ..Default::default() }
}

fn frame(db: &Database, ctx: i64, doc: i64, page: i64, t0: i64, t1: i64) -> i64 {
    let p = NewPageImage { context_id: ctx, document_id: doc, page, char_start: None, char_end: None, width: 4, height: 3, image: vec![page as u8] };
    db.insert_frame(&p, t0, t1).unwrap().id
}

/// Passages `(t0, t1, text)` laid out one after another, as a chunker would.
fn cut(ctx: i64, doc: i64, parts: &[(i64, i64, &str)]) -> (String, Vec<NewChunk>) {
    let mut text = String::new();
    let mut chunks = Vec::new();
    for (i, (t0, t1, body)) in parts.iter().enumerate() {
        text.push_str("## heading\n");
        let start = text.len();
        text.push_str(body);
        chunks.push(NewChunk {
            context_id: ctx,
            document_id: doc,
            chunk_index: i as i64,
            char_start: start as i64,
            char_end: text.len() as i64,
            text: body.to_string(),
            signature: None,
            is_omitted: false,
            metadata: serde_json::json!({ "media": { "t0": t0, "t1": t1, "kind": "speech" } }).to_string(),
        });
        text.push_str("\n\n");
    }
    (text, chunks)
}

fn doc_chunks(db: &Database, doc: i64) -> crate::db::Result<Vec<Chunk>> {
    let ids: Vec<i64> = db.conn.prepare("SELECT id FROM chunks WHERE document_id = ?1 ORDER BY chunk_index")?.query_map([doc], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    db.chunks_by_ids(&ids)
}

/// The chunk texts an image hit on still `id` can land on.
fn hit_texts(db: &Database, doc: i64, id: i64) -> Vec<String> {
    let f = db.frame_times(doc).unwrap().into_iter().find(|f| f.page_image_id == id).unwrap();
    let chunks = doc_chunks(db, doc).unwrap();
    let spans: Vec<ChunkSpan> =
        chunks.iter().map(|c| ChunkSpan { chunk_id: c.id, char_start: c.char_start, char_end: c.char_end, page: None, last_page: None }).collect();
    let ids = chunks_for_page(&spans, f.page, f.char_start.zip(f.char_end));
    chunks.into_iter().filter(|c| ids.contains(&c.id)).map(|c| c.text).collect()
}

#[test]
fn media_row_round_trips_with_its_json_columns() {
    let (db, _, doc) = setup();
    assert_eq!(db.media_document(doc).unwrap(), None);
    assert!(db.set_media_complete(doc, true).is_err(), "no row yet");
    let mut m = media(doc);
    m.speakers = vec![MediaSpeaker { id: 0, label: "Speaker 1".into(), name: None }];
    db.upsert_media_document(&m).unwrap();
    assert_eq!(db.media_document(doc).unwrap(), Some(m.clone()));

    db.set_media_phase(doc, "transcribing", MediaPhase { state: PhaseState::InProgress, done_ms: 30_000 }).unwrap();
    db.set_media_phase(doc, "frames", MediaPhase { state: PhaseState::Skipped, done_ms: 0 }).unwrap();
    db.add_media_engine(doc, EngineSpan { phase: "scenes".into(), engine: "gemma".into(), t0_ms: 0, t1_ms: 60_000 }).unwrap();
    db.set_media_speakers(doc, &[MediaSpeaker { id: 0, label: "Speaker 1".into(), name: Some("Anna".into()) }]).unwrap();
    db.set_media_complete(doc, true).unwrap();
    let got = db.media_document(doc).unwrap().unwrap();
    assert_eq!(got.phases["transcribing"], MediaPhase { state: PhaseState::InProgress, done_ms: 30_000 });
    assert_eq!(got.phases["frames"].state, PhaseState::Skipped);
    assert_eq!(got.engine_at("scenes", 12_000), Some("gemma"));
    assert_eq!(got.engine_at("sounds", 12_000), None);
    assert_eq!((got.speaker_name(0), got.speaker_name(4)), ("Anna".to_string(), "Speaker 5".to_string()));
    assert!(got.complete);
    let phases: String = db.conn.query_row("SELECT phases FROM media_documents", [], |r| r.get(0)).unwrap();
    assert!(phases.contains("\"in_progress\""), "{phases}");
}

#[test]
fn segments_come_back_in_time_order_with_their_words() {
    let (db, _, doc) = setup();
    let mut talk = seg(doc, SegmentKind::Speech, 5_000, 9_000, "Guten Morgen. Hallo.");
    talk.words = vec![
        MediaWord { text: "Guten".into(), t0_ms: 5_000, t1_ms: 5_400, speaker: Some(0) },
        MediaWord { text: "Hallo.".into(), t0_ms: 8_000, t1_ms: 9_000, speaker: Some(1) },
    ];
    talk.language = Some("de".into());
    talk.confidence = Some(0.93);
    let slide = MediaSegment { generated: false, ..seg(doc, SegmentKind::Slide, 0, 20_000, "Agenda") };
    let ids = db.insert_media_segments(&[talk.clone(), slide.clone()]).unwrap();
    let got = db.media_segments(doc).unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(got[0], MediaSegment { id: ids[1], ..slide });
    assert_eq!(got[1], MediaSegment { id: ids[0], ..talk });
    let words: String = db.conn.query_row("SELECT words FROM media_segments WHERE id = ?1", [ids[0]], |r| r.get(0)).unwrap();
    assert!(words.contains("\"sp\":1") && words.contains("\"w\":\"Guten\""), "short keys: {words}");
    let none: Option<String> = db.conn.query_row("SELECT words FROM media_segments WHERE id = ?1", [ids[1]], |r| r.get(0)).unwrap();
    assert_eq!(none, None, "no words, no JSON");
}

#[test]
fn resuming_drops_only_what_the_interrupted_phase_wrote_past_done_ms() {
    let (db, _, doc) = setup();
    db.upsert_media_document(&media(doc)).unwrap();
    db.insert_media_segments(&[
        seg(doc, SegmentKind::Speech, 0, 28_000, "eins"),
        seg(doc, SegmentKind::Speech, 28_000, 55_000, "zwei"),
        seg(doc, SegmentKind::Speech, 55_000, 61_000, "drei (halb)"),
        seg(doc, SegmentKind::Sound, 56_000, 58_000, "applause"),
    ])
    .unwrap();
    db.set_media_phase(doc, "transcribing", MediaPhase { state: PhaseState::InProgress, done_ms: 55_000 }).unwrap();
    let done = db.media_document(doc).unwrap().unwrap().phases["transcribing"].done_ms;
    assert_eq!(db.discard_media_segments_after(doc, &[SegmentKind::Speech], done).unwrap(), 1);
    db.insert_media_segments(&[seg(doc, SegmentKind::Speech, 55_000, 61_000, "drei")]).unwrap();
    let texts: Vec<String> = db.media_segments(doc).unwrap().into_iter().map(|s| s.text).collect();
    assert_eq!(texts, ["eins", "zwei", "drei", "applause"], "no double, other phases untouched");
}

#[test]
fn deleting_the_document_takes_media_rows_and_stills_along() {
    let (db, ctx, doc) = setup();
    db.upsert_media_document(&media(doc)).unwrap();
    let still = frame(&db, ctx, doc, 1, 0, 9_000);
    db.insert_media_segments(&[MediaSegment { page_image_id: Some(still), ..seg(doc, SegmentKind::Slide, 0, 9_000, "Agenda") }]).unwrap();
    db.delete_document(doc).unwrap();
    for t in ["media_documents", "media_segments", "page_images"] {
        let n: i64 = db.conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "{t}");
    }
}

#[test]
fn frame_range_spans_the_passages_of_its_time_else_the_nearest() {
    let s = |a, b, t0, t1| TimedSpan { char_start: a, char_end: b, t0_ms: t0, t1_ms: t1 };
    let spans = [s(10, 50, 0, 10_000), s(60, 90, 10_000, 20_000), s(100, 140, 30_000, 40_000)];
    assert_eq!(frame_range(&spans, 8_000, 12_000), Some((10, 90)), "across a boundary: both");
    assert_eq!(frame_range(&spans, 10_000, 15_000), Some((60, 90)), "touching end is no overlap");
    assert_eq!(frame_range(&spans, 10_000, 10_000), Some((10, 90)), "a still of one instant on a boundary: both");
    assert_eq!(frame_range(&spans, 22_000, 24_000), Some((60, 90)), "in a silence: the nearer in time");
    assert_eq!(frame_range(&spans, 25_000, 25_000), Some((60, 90)), "tie: the earlier");
    assert_eq!(frame_range(&spans, 50_000, 55_000), Some((100, 140)));
    assert_eq!(frame_range(&[], 0, 1), None);
}

#[test]
fn stills_follow_every_re_cut_and_keep_their_times() {
    let (db, ctx, doc) = setup();
    db.upsert_media_document(&media(doc)).unwrap();
    let a = frame(&db, ctx, doc, 1, 0, 12_000);
    let b = frame(&db, ctx, doc, 2, 25_000, 26_000);
    let c = frame(&db, ctx, doc, 3, 50_000, 60_000);

    // First cut: three passages.
    let (text, chunks) = cut(ctx, doc, &[(0, 10_000, "intro"), (10_000, 30_000, "middle part"), (30_000, 60_000, "the end")]);
    db.replace_media_document_text(doc, &text, &chunks).unwrap();
    assert_eq!(hit_texts(&db, doc, a), ["intro", "middle part"]);
    assert_eq!(hit_texts(&db, doc, b), ["middle part"]);
    assert_eq!(hit_texts(&db, doc, c), ["the end"]);
    let times = |db: &Database| db.frame_times(doc).unwrap().iter().map(|f| (f.t_start_ms, f.t_end_ms)).collect::<Vec<_>>();
    let before = times(&db);

    // Re-cut (other profile, longer passages first): byte ranges shift, the moment stays.
    let (text, chunks) = cut(ctx, doc, &[(0, 20_000, "intro and the first half of the middle"), (20_000, 45_000, "second half"), (45_000, 60_000, "the end")]);
    db.replace_media_document_text(doc, &text, &chunks).unwrap();
    assert_eq!(hit_texts(&db, doc, a), ["intro and the first half of the middle"]);
    assert_eq!(hit_texts(&db, doc, b), ["second half"]);
    assert_eq!(hit_texts(&db, doc, c), ["the end"]);
    assert_eq!(times(&db), before, "times of stills never change on a re-cut");
    let stored: String = db.conn.query_row("SELECT extracted_text FROM documents WHERE id = ?1", [doc], |r| r.get(0)).unwrap();
    assert_eq!(stored, text);

    // A failed re-cut leaves text, chunks and ranges as they were.
    let ranges = db.frame_times(doc).unwrap();
    let bad = NewChunk { document_id: doc + 99, ..chunks[0].clone() };
    assert!(db.replace_media_document_text(doc, "x", &[bad]).is_err());
    assert_eq!(db.frame_times(doc).unwrap(), ranges);
    assert_eq!(doc_chunks(&db, doc).unwrap().len(), 3);
}

#[test]
fn context_copy_carries_media_rows_stills_and_document_time() {
    let (db, ctx, doc) = setup();
    db.upsert_media_document(&media(doc)).unwrap();
    db.set_document_time(doc, &DocTime { taken_at_ms: Some(1_000), time_source: Some("quicktime".into()), ..Default::default() }).unwrap();
    let still = frame(&db, ctx, doc, 1, 1_000, 9_000);
    db.insert_media_segments(&[
        MediaSegment { page_image_id: Some(still), ..seg(doc, SegmentKind::Slide, 1_000, 9_000, "Agenda") },
        seg(doc, SegmentKind::Speech, 0, 9_000, "Hallo."),
    ])
    .unwrap();
    let dst = db.create_context(&NewContext { name: "copy".into(), chunking_strategy: "structural".into(), ..Default::default() }).unwrap().id;
    db.copy_context_chunks(ctx, dst).unwrap();
    let new_doc: i64 = db.conn.query_row("SELECT id FROM documents WHERE context_id = ?1", [dst], |r| r.get(0)).unwrap();
    assert_eq!(db.media_document(new_doc).unwrap(), Some(MediaDocument { document_id: new_doc, ..media(doc) }));
    assert_eq!(db.document_time(new_doc).unwrap().unwrap().time_source.as_deref(), Some("quicktime"));
    let frames = db.frame_times(new_doc).unwrap();
    assert_eq!((frames.len(), frames[0].t_start_ms, frames[0].t_end_ms), (1, 1_000, 9_000));
    let segs = db.media_segments(new_doc).unwrap();
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[1].page_image_id, Some(frames[0].page_image_id), "points at the copied still");
}

#[test]
fn a_re_cut_keeps_the_vectors_of_unchanged_passages() {
    let (db, ctx, doc) = setup();
    let (seed_ctx, _) = crate::db::chunks::chunks_tests::seed(&db);
    let model = db.context(seed_ctx).unwrap().unwrap().embedding_model_id.unwrap();
    db.upsert_media_document(&media(doc)).unwrap();
    let (text, chunks) = cut(ctx, doc, &[(0, 10_000, "intro"), (10_000, 20_000, "middle")]);
    db.replace_media_document_text(doc, &text, &chunks).unwrap();
    for (i, c) in doc_chunks(&db, doc).unwrap().iter().enumerate() {
        let v = vec![i as f32 + 1.0, 0.0, 0.0, 0.0];
        db.insert_embedding(&NewEmbedding { chunk_id: c.id, context_id: ctx, document_id: doc, embedding_model_id: model, dim: 4, vector: v }).unwrap();
    }
    // More speech came in: the first passage is the same, the second grew, a third is new.
    let (text, chunks) = cut(ctx, doc, &[(0, 10_000, "intro"), (10_000, 25_000, "middle and more"), (25_000, 30_000, "end")]);
    db.replace_media_document_text(doc, &text, &chunks).unwrap();
    let now = doc_chunks(&db, doc).unwrap();
    assert_eq!(db.embedding_vector(now[0].id).unwrap(), Some(vec![1.0, 0.0, 0.0, 0.0]));
    assert_eq!(db.embedding_vector(now[1].id).unwrap(), None, "changed text is embedded again");
    assert_eq!(db.embedding_vector(now[2].id).unwrap(), None);
}

#[test]
fn resuming_the_stills_drops_those_past_done_ms() {
    let (db, ctx, doc) = setup();
    db.upsert_media_document(&media(doc)).unwrap();
    let a = frame(&db, ctx, doc, 1, 0, 10_000);
    let b = frame(&db, ctx, doc, 2, 10_000, 30_000);
    db.insert_media_segments(&[MediaSegment { page_image_id: Some(b), ..seg(doc, SegmentKind::Slide, 10_000, 30_000, "slide") }]).unwrap();
    assert_eq!(db.discard_frames_after(doc, 10_000).unwrap(), 1);
    db.set_frame_time(a, 500, 9_000).unwrap();
    assert_eq!((db.frame_times(doc).unwrap()[0].t_start_ms, db.frame_times(doc).unwrap()[0].t_end_ms), (500, 9_000));
    let left: Vec<i64> = db.frame_times(doc).unwrap().iter().map(|f| f.page_image_id).collect();
    assert_eq!(left, [a]);
    assert_eq!(db.media_segments(doc).unwrap()[0].page_image_id, None);
    let id = db.media_segments(doc).unwrap()[0].id;
    db.set_media_segment_text(id, "a described scene", true).unwrap();
    let s = &db.media_segments(doc).unwrap()[0];
    assert_eq!((s.text.as_str(), s.generated), ("a described scene", true));
}
