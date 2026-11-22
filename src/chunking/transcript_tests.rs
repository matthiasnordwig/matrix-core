//! `transcript.rs`: speaker change inside one recognition window, pauses relative to the file, sentence
//! ends (never inside, an over-long sentence whole, an unpunctuated stretch at its pauses), language
//! changes, subtitles as their own stream, slides/scenes/sounds with their flags, exact byte ranges
//! without times, and that passage and still times stay put over re-cuts while image hits keep landing
//! on the passage of the same moment.

use super::*;
use crate::db::models::*;
use crate::db::Database;
use crate::embedding::page_select::{chunks_for_page, ChunkSpan};

const DOC: i64 = 1;

/// A speech segment from `t0`: each word 300 ms long, 100 ms apart, all of `speaker`.
fn talk(t0: i64, speaker: Option<i64>, text: &str) -> MediaSegment {
    let mut t = t0;
    let words: Vec<MediaWord> = text
        .split_whitespace()
        .map(|w| {
            let word = MediaWord { text: w.into(), t0_ms: t, t1_ms: t + 300, speaker: None };
            t += 400;
            word
        })
        .collect();
    MediaSegment { document_id: DOC, kind: SegmentKind::Speech, t_start_ms: t0, t_end_ms: t - 100, speaker, text: text.into(), words, ..Default::default() }
}

fn other(kind: SegmentKind, t0: i64, t1: i64, text: &str) -> MediaSegment {
    MediaSegment { document_id: DOC, kind, t_start_ms: t0, t_end_ms: t1, text: text.into(), ..Default::default() }
}

fn doc() -> MediaDocument {
    MediaDocument { document_id: DOC, duration_ms: 600_000, ..Default::default() }
}

fn meta(c: &NewChunk) -> serde_json::Value {
    serde_json::from_str(&c.metadata).unwrap()
}

fn times(c: &NewChunk) -> (i64, i64) {
    let m = meta(c);
    (m["media"]["t0"].as_i64().unwrap(), m["media"]["t1"].as_i64().unwrap())
}

fn texts(cut: &TranscriptCut) -> Vec<&str> {
    cut.chunks.iter().map(|c| c.text.as_str()).collect()
}

/// `n` one-segment sentences of 20 bytes, `gap(i)` ms of silence before sentence `i`.
fn sentences(n: usize, gap: impl Fn(usize) -> i64) -> Vec<MediaSegment> {
    let mut t = 0;
    (0..n)
        .map(|i| {
            t += gap(i);
            let s = talk(t, None, &format!("Satz nummer {i:02} hier."));
            t = s.t_end_ms;
            s
        })
        .collect()
}

#[test]
fn a_speaker_change_inside_one_window_splits_the_turn() {
    let mut window = talk(0, Some(0), "Wie war es? Gut, danke.");
    for w in &mut window.words[3..] {
        w.speaker = Some(1);
    }
    let mut m = doc();
    m.speakers = vec![MediaSpeaker { id: 0, label: "Speaker 1".into(), name: Some("Anna".into()) }];
    let whole = chunk_transcript("talk.m4a", &m, std::slice::from_ref(&window), 0, DOC, 0);
    assert_eq!(texts(&whole), ["Anna: Wie war es?\nSpeaker 2: Gut, danke."]);
    assert_eq!(meta(&whole.chunks[0])["media"]["speakers"], serde_json::json!([0, 1]));
    assert!(whole.text.contains("## 0:00–0:01 · Anna, Speaker 2\n"), "{}", whole.text);

    let cut = chunk_transcript("talk.m4a", &m, &[window], 0, DOC, 20);
    assert_eq!(texts(&cut), ["Anna: Wie war es?", "Speaker 2: Gut, danke."]);
    assert_eq!(times(&cut.chunks[0]), (0, 1_100), "ends with the word \"es?\"");
    assert_eq!(times(&cut.chunks[1]), (1_200, 1_900), "starts with the word \"Gut,\"");
}

#[test]
fn a_pause_long_for_this_file_is_the_preferred_cut() {
    let usual = |i: usize| 400 + (i as i64 % 3) * 100;
    // Sentence 6 follows a pause of 1.5 s: long next to 0.4–0.6 s, not far out.
    let cut = chunk_transcript("a.m4a", &doc(), &sentences(12, |i| if i == 6 { 1_500 } else { usual(i) }), 0, DOC, 200);
    assert_eq!(cut.chunks.len(), 2);
    assert!(cut.chunks[1].text.starts_with("Satz nummer 06"), "{:?}", texts(&cut));
    // Without it the cut goes to the latest sentence end that fits.
    let plain = chunk_transcript("a.m4a", &doc(), &sentences(12, usual), 0, DOC, 200);
    assert!(plain.chunks[1].text.starts_with("Satz nummer 09"), "{:?}", texts(&plain));
}

#[test]
fn a_far_out_pause_always_cuts_and_uniform_pauses_never_do() {
    let usual = |i: usize| 400 + (i as i64 % 3) * 100;
    let cut = chunk_transcript("a.m4a", &doc(), &sentences(12, |i| if i == 4 { 10_000 } else { usual(i) }), 0, DOC, 0);
    assert_eq!(cut.chunks.len(), 2, "fits one passage, but 10 s of silence cut it");
    assert!(cut.chunks[1].text.starts_with("Satz nummer 04"));
    // In a file that always pauses 10 s, 10 s is no long pause.
    let calm = chunk_transcript("a.m4a", &doc(), &sentences(12, |_| 10_000), 0, DOC, 0);
    assert_eq!(calm.chunks.len(), 1);
}

#[test]
fn never_inside_a_sentence_and_an_over_long_sentence_stays_whole() {
    let segs = vec![talk(0, None, "Ein Satz. Und noch einer, der deutlich zu lang ist für die kleine Grenze hier."), talk(9_000, None, "Kurz.")];
    let cut = chunk_transcript("a.m4a", &doc(), &segs, 0, DOC, 30);
    assert_eq!(texts(&cut), ["Ein Satz.", "Und noch einer, der deutlich zu lang ist für die kleine Grenze hier.", "Kurz."]);
    for c in &cut.chunks {
        assert!(c.text.ends_with('.'));
    }
}

#[test]
fn a_sentence_spanning_two_segments_stays_together() {
    let segs = vec![talk(0, None, "Das ist ein Satz"), talk(1_700, None, "über zwei Fenster. Danach kommt"), talk(4_100, None, "der Rest.")];
    let cut = chunk_transcript("a.m4a", &doc(), &segs, 0, DOC, 40);
    assert_eq!(texts(&cut), ["Das ist ein Satz über zwei Fenster.", "Danach kommt der Rest."]);
    assert_eq!(times(&cut.chunks[1]).0, 2_900, "time of the word \"Danach\"");
}

#[test]
fn a_stretch_without_punctuation_is_split_at_its_longest_pause() {
    let segs = vec![talk(0, None, "ohne punkt und komma geht es los"), talk(3_000, None, "und weiter ohne ende hier"), talk(9_000, None, "nach der langen pause")];
    let cut = chunk_transcript("a.m4a", &doc(), &segs, 0, DOC, 70);
    assert_eq!(texts(&cut), ["ohne punkt und komma geht es los und weiter ohne ende hier", "nach der langen pause"]);
    assert_eq!(times(&cut.chunks[1]).0, 9_000);
}

#[test]
fn a_change_of_language_is_preferred_and_one_language_pays_nothing() {
    let lang = |mut s: MediaSegment, l: &str| {
        s.language = Some(l.into());
        s
    };
    let mixed: Vec<MediaSegment> = sentences(8, |_| 500).into_iter().enumerate().map(|(i, s)| lang(s, if i < 3 { "de" } else { "en" })).collect();
    let cut = chunk_transcript("a.m4a", &doc(), &mixed, 0, DOC, 120);
    assert!(cut.chunks[1].text.starts_with("Satz nummer 03"), "{:?}", texts(&cut));
    let one: Vec<MediaSegment> = sentences(8, |_| 500).into_iter().map(|s| lang(s, "de")).collect();
    let cut = chunk_transcript("a.m4a", &doc(), &one, 0, DOC, 120);
    assert!(cut.chunks[1].text.starts_with("Satz nummer 05"), "{:?}", texts(&cut));
}

#[test]
fn subtitles_are_a_stream_of_their_own_and_no_transcript() {
    let mut cue1 = other(SegmentKind::Subtitle, 0, 2_000, "This sentence runs");
    let mut cue2 = other(SegmentKind::Subtitle, 2_000, 5_000, "across two cues. Next one.");
    cue1.language = Some("en".into());
    cue2.language = Some("en".into());
    let cut = chunk_transcript("v.mp4", &doc(), &[cue1, talk(500, None, "Hallo zusammen."), cue2], 0, DOC, 0);
    assert_eq!(texts(&cut), ["This sentence runs across two cues. Next one.", "Hallo zusammen."], "in time order");
    let (subs, speech) = (meta(&cut.chunks[0]), meta(&cut.chunks[1]));
    assert_eq!(speech["asr"], true);
    assert_eq!(subs["media"]["kind"], "subtitle");
    assert!(subs.get("asr").is_none());
    assert_eq!(subs["heading_path"], "0:00–0:05 · Subtitles (en)");
}

#[test]
fn slides_scenes_and_sounds_are_passages_of_their_own() {
    let mut m = doc();
    m.engines = vec![EngineSpan { phase: "scenes".into(), engine: "gemma-4-e2b".into(), t0_ms: 0, t1_ms: 600_000 }];
    let slide = MediaSegment { page_image_id: Some(7), ..other(SegmentKind::Slide, 0, 20_000, "Agenda\nZiele") };
    let scene = MediaSegment { page_image_id: Some(8), generated: true, ..other(SegmentKind::Scene, 5_000, 9_000, "Eine Person am Pult.") };
    let sound = other(SegmentKind::Sound, 9_000, 9_500, "applause");
    let cut = chunk_transcript("v.mp4", &m, &[sound, scene, talk(0, None, "Willkommen."), slide], 0, DOC, 0);
    assert_eq!(texts(&cut), ["Agenda\nZiele", "Willkommen.", "Eine Person am Pult.", "applause"]);
    let all: Vec<serde_json::Value> = cut.chunks.iter().map(meta).collect();
    assert_eq!((all[0]["ocr"].clone(), all[0]["media"]["frame"].clone(), all[0].get("generated")), (true.into(), 7.into(), None));
    assert_eq!((all[2]["generated"].clone(), all[2]["generator"].clone(), all[2]["media"]["kind"].clone()), (true.into(), "gemma-4-e2b".into(), "scene".into()));
    assert_eq!((all[3]["generated"].clone(), all[3].get("generator")), (true.into(), None), "no engine recorded, no name");
    assert!(cut.text.contains("### On screen 0:00–0:20 (slide, OCR)\nAgenda\nZiele\n"), "{}", cut.text);
    assert!(cut.text.contains("### Sounds 0:09–0:09: applause (AI)\napplause\n"), "{}", cut.text);
}

#[test]
fn passages_are_exact_slices_without_times() {
    let segs = vec![talk(0, Some(0), "Erster Satz hier. Zweiter Satz da."), talk(5_000, Some(1), "Dritter Satz."), other(SegmentKind::Slide, 1_000, 4_000, "Folie eins")];
    let cut = chunk_transcript("a.mp4", &doc(), &segs, 0, DOC, 25);
    for (i, c) in cut.chunks.iter().enumerate() {
        assert_eq!(&cut.text[c.char_start as usize..c.char_end as usize], c.text);
        assert_eq!(c.chunk_index, i as i64);
        assert!(!c.text.contains("0:0") && !c.text.contains('–'), "{}", c.text);
    }
    assert!(cut.text.starts_with("# a.mp4\n"));
}

/// Image-hit chunks of every still, by text, as `chunks_for_page` picks them.
fn hits(db: &Database, ctx: i64, doc: i64) -> Vec<(FrameTime, Vec<(i64, i64)>)> {
    let chunks = db.list_chunks(ctx, true).unwrap();
    let spans: Vec<ChunkSpan> = chunks.iter().map(|c| ChunkSpan { chunk_id: c.id, char_start: c.char_start, char_end: c.char_end, page: None, last_page: None }).collect();
    db.frame_times(doc)
        .unwrap()
        .into_iter()
        .map(|f| {
            let ids = chunks_for_page(&spans, f.page, f.char_start.zip(f.char_end));
            let ts = chunks.iter().filter(|c| ids.contains(&c.id)).map(|c| {
                let m: serde_json::Value = serde_json::from_str(&c.metadata).unwrap();
                (m["media"]["t0"].as_i64().unwrap(), m["media"]["t1"].as_i64().unwrap())
            });
            (f, ts.collect())
        })
        .collect()
}

#[test]
fn times_stay_put_over_re_cuts_and_stills_keep_hitting_the_same_moment() {
    let db = Database::open_in_memory().unwrap();
    let ctx = db.create_context(&NewContext { name: "m".into(), chunking_strategy: "structural".into(), ..Default::default() }).unwrap().id;
    let doc_id = db
        .create_document(&NewDocument { context_id: ctx, name: "v.mp4".into(), zip_entry: None, byte_size: None, page_count: None, content_hash: None, extracted_text: Some(String::new()) })
        .unwrap()
        .id;
    let mut segs = sentences(20, |i| if i % 7 == 0 { 2_500 } else { 450 });
    for (i, s) in segs.iter_mut().enumerate() {
        s.document_id = doc_id;
        s.speaker = Some((i / 5) as i64 % 2);
    }
    let m = MediaDocument { document_id: doc_id, kind: MediaKind::Video, duration_ms: 60_000, ..Default::default() };
    for (page, (t0, t1)) in [(0, 3_000), (14_000, 15_000), (31_000, 40_000), (59_000, 60_000)].into_iter().enumerate() {
        let p = NewPageImage { context_id: ctx, document_id: doc_id, page: page as i64 + 1, char_start: None, char_end: None, width: 2, height: 2, image: vec![1] };
        db.insert_frame(&p, t0, t1).unwrap();
    }
    let finest = chunk_transcript("v.mp4", &m, &segs, ctx, doc_id, 1);
    let (starts, ends): (Vec<i64>, Vec<i64>) = finest.chunks.iter().map(times).unzip();
    let mut frame_times_before = None;
    for max in [120, 400, 120] {
        let cut = chunk_transcript("v.mp4", &m, &segs, ctx, doc_id, max);
        for c in &cut.chunks {
            let (t0, t1) = times(c);
            assert!(starts.contains(&t0) && ends.contains(&t1), "passage times are sentence times");
        }
        assert_eq!((times(&cut.chunks[0]).0, times(cut.chunks.last().unwrap()).1), (starts[0], *ends.last().unwrap()));
        db.replace_media_document_text(doc_id, &cut.text, &cut.chunks).unwrap();
        let got = hits(&db, ctx, doc_id);
        for (f, ts) in &got {
            assert!(!ts.is_empty(), "every still lands on a passage");
            let overlapping = ts.iter().any(|&(a, b)| a < f.t_end_ms && f.t_start_ms < b);
            let nearest_only = ts.len() == 1;
            assert!(overlapping || nearest_only, "still {f:?} hits {ts:?}");
            for &(a, b) in ts {
                assert!(a < f.t_end_ms && f.t_start_ms < b || nearest_only, "no passage of another moment: {f:?} {ts:?}");
            }
        }
        let ft: Vec<(i64, i64)> = got.iter().map(|(f, _)| (f.t_start_ms, f.t_end_ms)).collect();
        assert_eq!(*frame_times_before.get_or_insert(ft.clone()), ft, "still times never change");
    }
    let a = chunk_transcript("v.mp4", &m, &segs, ctx, doc_id, 120);
    let b = chunk_transcript("v.mp4", &m, &segs, ctx, doc_id, 120);
    assert_eq!((a.text, a.chunks.iter().map(|c| (&c.text, &c.metadata)).collect::<Vec<_>>()), (b.text, b.chunks.iter().map(|c| (&c.text, &c.metadata)).collect::<Vec<_>>()));
}
