//! Passages from the time segments of a media document (`db/media.rs`), within the profile's
//! `max_chunk_chars`, and the stored text they are slices of (`media/text.rs`).
//!
//! **Speech and subtitles** run as streams (speech; subtitles per language). A segment is split where
//! the speaker of its words changes (a recognition window may hold two speakers); consecutive parts of
//! one speaker and language form a run, cut into sentences (`sentences.rs`) — a sentence never spans a
//! change of speaker. Each sentence gets its time from its words, else by position within its segment.
//! Sentences are packed into passages greedily; when the next one would overflow, the cut goes to the
//! best boundary of the passage that leaves it at least half full: a long pause or a change of language
//! before a change of speaker before a plain sentence end, the later one on a tie. A pause is long
//! relative to the file's own pauses (Tukey fence over the gaps between sentences, the scale at least
//! the median gap); one beyond the far fence always cuts. Never inside a sentence, never at a character
//! count: a sentence longer than `max_chars` stays whole — unless it has no closing mark (a recogniser
//! that sets no punctuation leaves one long trailing stretch), which is split at its longest pauses
//! between words or segments.
//! Segments carry differing languages only where the pre-pass (AP5, `speech/windowing.rs`) found the
//! file multilingual, so a one-language file pays nothing for the language rule.
//!
//! **Slides, scenes, sounds** are passages of their own (a long one split at sentence ends). Metadata:
//! `chunk_type = "transcript"`, `heading_path` = the section heading, `media: {t0, t1, kind, speakers?,
//! frame?}`; speech `asr = true` and slides `ocr = true` (source text like OCR), scenes, sounds and
//! anything a model wrote `generated = true` + `generator` (engine of the phase at `t0`, `MediaDocument::
//! engines`). Passage times come from the segments alone, so they stay the same over every re-cut.
//! Flow: `handbuch/medien.md`.

use super::markdown::DEFAULT_MAX_CHARS;
use super::sentences::split_sentences;
use crate::db::models::{MediaDocument, MediaSegment, NewChunk, SegmentKind};
use crate::media::text::{render, Section};

/// The stored text of a media document and its passages (byte ranges into `text`).
#[derive(Debug, Clone)]
pub struct TranscriptCut {
    pub text: String,
    pub chunks: Vec<NewChunk>,
}

/// A boundary that always cuts.
const FORCED: u8 = 4;

/// One sentence (or one stretch of a slide, scene or sound) with its time.
#[derive(Debug, Clone)]
struct Unit {
    t0: i64,
    t1: i64,
    speaker: Option<i64>,
    lang: Option<String>,
    text: String,
    /// Timed stretches inside `text` (words, or whole segments without words): byte start, t0, t1.
    marks: Vec<(usize, i64, i64)>,
    /// Boundary before this unit: 1 sentence end, 2 change of speaker, 3 long pause or change of
    /// language, [`FORCED`].
    level: u8,
}

/// A run of one speaker and language: its text and the timed stretches in it.
struct Run {
    text: String,
    speaker: Option<i64>,
    lang: Option<String>,
    /// Byte range of each segment part in `text` with its time.
    parts: Vec<(usize, usize, i64, i64)>,
    /// Byte range of each located word with its time.
    words: Vec<(usize, usize, i64, i64)>,
}

/// Cuts the segments of `media` into its stored text and passages. `max_chars` 0 = the Markdown default.
pub fn chunk_transcript(
    name: &str,
    media: &MediaDocument,
    segments: &[MediaSegment],
    context_id: i64,
    document_id: i64,
    max_chars: usize,
) -> TranscriptCut {
    let max = if max_chars == 0 { DEFAULT_MAX_CHARS } else { max_chars };
    let mut sorted: Vec<&MediaSegment> = segments.iter().filter(|s| !s.text.trim().is_empty()).collect();
    sorted.sort_by_key(|s| (s.t_start_ms, s.id));

    // Every passage: its section, its speakers, its still.
    let mut out: Vec<(Section, Vec<i64>, Option<i64>)> = Vec::new();
    let mut streams: Vec<(SegmentKind, Option<String>)> = Vec::new();
    for s in &sorted {
        match s.kind {
            SegmentKind::Speech => push_unique(&mut streams, (SegmentKind::Speech, None)),
            SegmentKind::Subtitle => push_unique(&mut streams, (SegmentKind::Subtitle, s.language.clone())),
            kind => {
                let units = vec![Unit { t0: s.t_start_ms, t1: s.t_end_ms, speaker: None, lang: None, text: s.text.trim().to_string(), marks: vec![], level: 1 }];
                let units = if s.text.trim().len() > max { sentence_units(&units[0]) } else { units };
                for p in pack(units, max, joined_len) {
                    let body = p.iter().map(|u| u.text.as_str()).collect::<Vec<_>>().join(" ");
                    let generated = s.generated || matches!(kind, SegmentKind::Scene | SegmentKind::Sound);
                    let section = Section { kind, t0_ms: s.t_start_ms, t1_ms: s.t_end_ms, tags: vec![], body, generated };
                    out.push((section, vec![], s.page_image_id));
                }
            }
        }
    }
    for (kind, lang) in streams {
        let segs: Vec<&MediaSegment> = sorted.iter().copied().filter(|s| s.kind == kind && (kind == SegmentKind::Speech || s.language == lang)).collect();
        let mut units: Vec<Unit> = runs(&segs).iter().flat_map(run_units).flat_map(|u| split_unpunctuated(u, max)).collect();
        set_levels(&mut units);
        let body_of = |p: &[Unit]| speaker_body(media, p);
        for p in pack(units, max, |p| body_of(p).len()) {
            let mut speakers: Vec<i64> = Vec::new();
            p.iter().filter_map(|u| u.speaker).for_each(|id| push_unique(&mut speakers, id));
            let tags = match kind {
                SegmentKind::Subtitle => vec![lang.as_ref().map_or("Subtitles".to_string(), |l| format!("Subtitles ({l})"))],
                _ => speakers.iter().map(|id| media.speaker_name(*id)).collect(),
            };
            let section = Section { kind, t0_ms: p[0].t0, t1_ms: p.iter().map(|u| u.t1).max().unwrap_or(p[0].t1), tags, body: body_of(&p), generated: false };
            out.push((section, speakers, None));
        }
    }
    let rank = |k: SegmentKind| match k {
        SegmentKind::Slide => 0,
        SegmentKind::Scene => 1,
        SegmentKind::Speech => 2,
        SegmentKind::Subtitle => 3,
        SegmentKind::Sound => 4,
    };
    out.sort_by_key(|(s, _, _)| (s.t0_ms, rank(s.kind)));
    let sections: Vec<Section> = out.iter().map(|(s, _, _)| s.clone()).collect();
    let rendered = render(name, media.duration_ms, &sections);
    let chunks = out
        .iter()
        .zip(&rendered.placed)
        .enumerate()
        .map(|(i, ((s, speakers, frame), at))| NewChunk {
            context_id,
            document_id,
            chunk_index: i as i64,
            char_start: at.start as i64,
            char_end: at.end as i64,
            text: rendered.text[at.start..at.end].to_string(),
            signature: None,
            is_omitted: false,
            metadata: metadata(media, s, speakers, *frame, &at.heading).to_string(),
        })
        .collect();
    TranscriptCut { text: rendered.text, chunks }
}

fn push_unique<T: PartialEq>(v: &mut Vec<T>, x: T) {
    if !v.contains(&x) {
        v.push(x);
    }
}

fn metadata(media: &MediaDocument, s: &Section, speakers: &[i64], frame: Option<i64>, heading: &str) -> serde_json::Value {
    let kind = s.kind.as_str();
    let mut m = serde_json::json!({
        "chunk_type": "transcript",
        "heading_path": heading,
        "media": { "t0": s.t0_ms, "t1": s.t1_ms, "kind": kind },
    });
    if !speakers.is_empty() {
        m["media"]["speakers"] = speakers.into();
    }
    if let Some(f) = frame {
        m["media"]["frame"] = f.into();
    }
    if s.generated {
        let phase = match s.kind {
            SegmentKind::Speech => "transcribing",
            SegmentKind::Slide => "reading_slides",
            SegmentKind::Scene => "scenes",
            SegmentKind::Sound => "sounds",
            SegmentKind::Subtitle => "subtitles",
        };
        m["generated"] = true.into();
        if let Some(engine) = media.engine_at(phase, s.t0_ms) {
            m["generator"] = engine.into();
        }
    } else if s.kind == SegmentKind::Speech {
        m["asr"] = true.into();
    } else if s.kind == SegmentKind::Slide {
        m["ocr"] = true.into();
    }
    m
}

/// Length of units joined by spaces (passage text of a slide, scene or sound).
fn joined_len(units: &[Unit]) -> usize {
    units.iter().map(|u| u.text.len()).sum::<usize>() + units.len().saturating_sub(1)
}

/// Passage text of speech: one line per turn, `Name: …` where the speaker is known.
fn speaker_body(media: &MediaDocument, units: &[Unit]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut last: Option<Option<i64>> = None;
    for u in units {
        if last == Some(u.speaker) {
            let l = lines.last_mut().expect("a line per turn");
            l.push(' ');
            l.push_str(&u.text);
        } else {
            lines.push(match u.speaker {
                Some(id) => format!("{}: {}", media.speaker_name(id), u.text),
                None => u.text.clone(),
            });
            last = Some(u.speaker);
        }
    }
    lines.join("\n")
}

/// Splits each segment where its words change speaker and joins consecutive parts of one speaker and
/// language into runs.
fn runs(segs: &[&MediaSegment]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for s in segs {
        let text = s.text.as_str();
        // Located words: byte range in `text`, time, speaker.
        let mut located: Vec<(usize, usize, i64, i64, Option<i64>)> = Vec::new();
        let mut cursor = 0;
        for w in &s.words {
            let needle = w.text.trim();
            if needle.is_empty() {
                continue;
            }
            if let Some(i) = text[cursor..].find(needle) {
                let at = cursor + i;
                cursor = at + needle.len();
                located.push((at, cursor, w.t0_ms, w.t1_ms.max(w.t0_ms), w.speaker.or(s.speaker)));
            }
        }
        // Part boundaries where the speaker changes.
        let mut cuts = vec![0usize];
        for k in 1..located.len() {
            if located[k].4 != located[k - 1].4 {
                cuts.push(located[k].0);
            }
        }
        cuts.push(text.len());
        for (pi, win) in cuts.windows(2).enumerate() {
            let (a, b) = (win[0], win[1]);
            let words: Vec<_> = located.iter().filter(|w| w.0 >= a && w.1 <= b).collect();
            let first = pi == 0;
            let last = b == text.len();
            let t0 = if first { s.t_start_ms } else { words.first().map_or(s.t_start_ms, |w| w.2) };
            let t1 = if last { s.t_end_ms } else { words.last().map_or(s.t_end_ms, |w| w.3) }.max(t0);
            let speaker = words.first().map_or(s.speaker, |w| w.4);
            let raw = &text[a..b];
            let lead = raw.len() - raw.trim_start().len();
            let part = raw.trim();
            if part.is_empty() {
                continue;
            }
            let joins = out.last().is_some_and(|r| r.speaker == speaker && r.lang == s.language);
            if !joins {
                out.push(Run { text: String::new(), speaker, lang: s.language.clone(), parts: vec![], words: vec![] });
            }
            let run = out.last_mut().expect("pushed above");
            if !run.text.is_empty() {
                run.text.push(' ');
            }
            let base = run.text.len();
            run.text.push_str(part);
            run.parts.push((base, run.text.len(), t0, t1));
            let shift = |p: usize| (base + p.saturating_sub(a + lead)).min(run.text.len());
            run.words.extend(words.iter().map(|w| (shift(w.0), shift(w.1), w.2, w.3)));
        }
    }
    out
}

/// Time at byte `b` of a run, by position within its segment part.
fn time_at(run: &Run, b: usize) -> i64 {
    let p = run.parts.iter().find(|p| b <= p.1).or(run.parts.last()).expect("a run has parts");
    let (a, e, t0, t1) = *p;
    if b <= a || e <= a {
        return t0;
    }
    t0 + ((t1 - t0) as f64 * (b.min(e) - a) as f64 / (e - a) as f64) as i64
}

/// The sentences of a run with their time and timed stretches.
fn run_units(run: &Run) -> Vec<Unit> {
    split_sentences(&run.text)
        .into_iter()
        .map(|s| {
            let (bs, be) = (s.byte_start, s.byte_end);
            let mut marks: Vec<(usize, i64, i64)> = Vec::new();
            for &(a, e, t0, t1) in &run.parts {
                if a >= be || e <= bs {
                    continue;
                }
                let inner: Vec<_> = run.words.iter().filter(|w| w.0 >= a.max(bs) && w.1 <= e.min(be)).collect();
                if inner.is_empty() {
                    let (x, y) = (a.max(bs), e.min(be));
                    marks.push((x - bs, time_at(run, x).max(t0), time_at(run, y).min(t1)));
                } else {
                    marks.extend(inner.iter().map(|w| (w.0 - bs, w.2, w.3)));
                }
            }
            let t0 = marks.first().map_or_else(|| time_at(run, bs), |m| m.1);
            let t1 = marks.last().map_or_else(|| time_at(run, be), |m| m.2).max(t0);
            Unit { t0, t1, speaker: run.speaker, lang: run.lang.clone(), text: s.text, marks, level: 1 }
        })
        .collect()
}

/// The sentences of a slide, scene or sound text over `max`, all with its time.
fn sentence_units(u: &Unit) -> Vec<Unit> {
    split_sentences(&u.text).into_iter().map(|s| Unit { text: s.text, ..u.clone() }).collect()
}

/// A unit over `max` that is no sentence (no closing mark: a stretch the recogniser left without
/// punctuation): split at its longest pause (recursively).
fn split_unpunctuated(u: Unit, max: usize) -> Vec<Unit> {
    // A real sentence ends with its mark; recognised text without punctuation is trailing text.
    let punctuated = u.text.trim_end_matches(['"', '\'', ')', ']', '»', '”', '’']).ends_with(['.', '!', '?', '…']);
    if u.text.len() <= max || punctuated || u.marks.len() < 2 {
        return vec![u];
    }
    let mid = u.text.len() / 2;
    let k = (1..u.marks.len())
        .max_by_key(|&k| ((u.marks[k].1 - u.marks[k - 1].2), std::cmp::Reverse(u.marks[k].0.abs_diff(mid))))
        .expect("two marks or more");
    let cut = u.marks[k].0;
    let (left, right) = (u.text[..cut].trim_end().to_string(), u.text[cut..].trim().to_string());
    let a = Unit { t1: u.marks[k - 1].2, text: left, marks: u.marks[..k].to_vec(), ..u.clone() };
    let b = Unit { t0: u.marks[k].1, text: right, marks: u.marks[k..].iter().map(|m| (m.0 - cut, m.1, m.2)).collect(), level: 1, ..u };
    let mut out = split_unpunctuated(a, max);
    out.extend(split_unpunctuated(b, max));
    out
}

/// Value at quantile `q` of sorted `v` (linear between neighbours).
fn quantile(v: &[i64], q: f64) -> f64 {
    let pos = q * (v.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    v[lo] as f64 + (v[hi] - v[lo]) as f64 * (pos - lo as f64)
}

/// Boundary level before every unit of a stream (see [`Unit::level`]).
fn set_levels(units: &mut [Unit]) {
    let gaps: Vec<i64> = units.windows(2).map(|w| w[1].t0 - w[0].t1).collect();
    let mut positive: Vec<i64> = gaps.iter().copied().filter(|g| *g > 0).collect();
    positive.sort_unstable();
    // Gaps of 0 come from interpolated times, not from listening; too few real ones say nothing.
    let fences = (positive.len() >= 4).then(|| {
        let (q1, q2, q3) = (quantile(&positive, 0.25), quantile(&positive, 0.5), quantile(&positive, 0.75));
        let spread = (q3 - q1).max(q2);
        (q3 + 1.5 * spread, q3 + 3.0 * spread)
    });
    for k in 1..units.len() {
        let (prev, u) = (&units[k - 1], &units[k]);
        let gap = gaps[k - 1] as f64;
        let mut level = 1;
        if prev.speaker != u.speaker {
            level = 2;
        }
        if prev.lang.is_some() && u.lang.is_some() && prev.lang != u.lang {
            level = 3;
        }
        if let Some((near, far)) = fences {
            if gap > far {
                level = FORCED;
            } else if gap > near {
                level = level.max(3);
            }
        }
        units[k].level = level;
    }
}

/// Packs units into passages of at most `max` by `size` (an over-long unit stays alone), cutting at the
/// best boundary that leaves a passage at least half full (module comment).
fn pack(units: Vec<Unit>, max: usize, size: impl Fn(&[Unit]) -> usize) -> Vec<Vec<Unit>> {
    let mut out: Vec<Vec<Unit>> = Vec::new();
    let mut cur: Vec<Unit> = Vec::new();
    for u in units {
        if u.level >= FORCED && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(u);
        while cur.len() > 1 && size(&cur) > max {
            let mut best: Option<(u8, usize)> = None;
            for k in 1..cur.len() {
                if size(&cur[..k]) * 2 >= max && best.is_none_or(|(level, _)| cur[k].level >= level) {
                    best = Some((cur[k].level, k));
                }
            }
            let k = best.map_or(cur.len() - 1, |(_, k)| k);
            out.push(cur.drain(..k).collect());
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod transcript_tests;
