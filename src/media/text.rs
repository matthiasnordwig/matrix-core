//! The stored Markdown text of a media document: `# name`, then one section per passage in time order —
//! `## 12:30–14:02 · Speaker 2`, `## 0:00–0:42 · Subtitles (en)`, `### On screen 12:28–13:40 (slide, OCR)`,
//! `### Scene 03:10–03:45 (AI description)`, `### Sounds 05:00–05:20: applause (AI)`. Times stand only in
//! headings, never in the passage text (`Speaker 2: …`). Clock `m:ss`, from one hour of duration on
//! `h:mm:ss` throughout. [`render`] reports each section's body byte range — a passage is exactly its body,
//! so `text[start..end] == body` — with its time range, kind and whether a model generated it; the
//! passages (`chunking/transcript.rs`) and the stills' text ranges (`db/media.rs`) build on that.
//! Flow: `handbuch/medien.md`.

use crate::db::models::SegmentKind;

/// One passage to write: `tags` follow the time in the heading of speech and subtitles (speaker names,
/// "Subtitles (en)"); `body` is the passage text.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Section {
    pub kind: SegmentKind,
    pub t0_ms: i64,
    pub t1_ms: i64,
    pub tags: Vec<String>,
    pub body: String,
    pub generated: bool,
}

/// Where a section landed: its heading (without `#`) and the byte range of its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub heading: String,
    pub start: usize,
    pub end: usize,
}

/// The whole text and, in the order of the given sections, where each landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    pub placed: Vec<Placed>,
}

/// `m:ss`, or `h:mm:ss` when `long` (a file of an hour or more), from milliseconds (rounded down).
pub fn clock(ms: i64, long: bool) -> String {
    let s = ms.max(0) / 1000;
    if long {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// The heading of `s` without its `#` marks.
pub fn heading(s: &Section, long: bool) -> String {
    let span = format!("{}–{}", clock(s.t0_ms, long), clock(s.t1_ms, long));
    let one_line = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
    match s.kind {
        SegmentKind::Speech | SegmentKind::Subtitle if s.tags.is_empty() => span,
        SegmentKind::Speech | SegmentKind::Subtitle => format!("{span} · {}", s.tags.join(", ")),
        SegmentKind::Slide if s.generated => format!("On screen {span} (slide, AI description)"),
        SegmentKind::Slide => format!("On screen {span} (slide, OCR)"),
        SegmentKind::Scene => format!("Scene {span} (AI description)"),
        SegmentKind::Sound => format!("Sounds {span}: {} (AI)", one_line(&s.body)),
    }
}

/// Writes the document text: the name, then every section in the given order.
pub fn render(name: &str, duration_ms: i64, sections: &[Section]) -> Rendered {
    let long = duration_ms >= 3_600_000;
    let mut text = format!("# {}\n", name.split_whitespace().collect::<Vec<_>>().join(" "));
    let mut placed = Vec::with_capacity(sections.len());
    for s in sections {
        let level = if matches!(s.kind, SegmentKind::Speech | SegmentKind::Subtitle) { "##" } else { "###" };
        let h = heading(s, long);
        text.push_str(&format!("\n{level} {h}\n"));
        let start = text.len();
        text.push_str(s.body.trim());
        placed.push(Placed { heading: h, start, end: text.len() });
        text.push('\n');
    }
    Rendered { text, placed }
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod text_tests;
