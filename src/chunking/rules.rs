//! Structural chunking by line markers (LLM-free), for the document kinds that have no chunker of
//! their own: a table per kind names the lines that start a unit (a paragraph of law, an agenda
//! item, a speaker's turn, a chapter, a chat session, a function), and `chunk_by_rules` cuts there.
//!
//! Offsets are byte offsets and chunks are verbatim slices, as in `markdown.rs`. Units shorter than
//! `min_chars` join the next one, so a bare "Teil 2" stays with its first paragraph. A unit over
//! `max_chars` is cut at the kind's inner marks, at paragraphs, then as `markdown.rs` cuts a long line.

use std::sync::LazyLock;

use regex::Regex;

use super::kind::DocKind;
use super::markdown::{pack_at, split_long_text, trimmed_end};
use crate::db::models::NewChunk;

const PATH_SEP: &str = " › ";
/// Pause in a chat that starts a new conversation.
const CHAT_GAP_MINUTES: i64 = 30;

macro_rules! rx {
    ($name:ident, $pattern:literal) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).unwrap());
    };
}

rx!(LEGAL_PART, r"(?i)^(?:#{1,6}\s+|\*\*)?(?:teil|buch|titel|kapitel|abschnitt|unterabschnitt|part|title|chapter|anlage|anhang|annex|appendix)\s+[\divxlc]+\b");
rx!(LEGAL_UNIT, r"(?i)^(?:#{1,6}\s+|\*\*)?(?:§§?\s*\d|art\.\s*\d|artikel\s+\d|article\s+\d|artículo\s+\d|section\s+\d+[.:]|ziffer\s+\d)");
rx!(LEGAL_INNER, r"^\s*\(\d+[a-z]?\)\s");
rx!(AGENDA, r"(?i)^(?:#{1,6}\s+|\*\*)?(?:top\s*\d|tagesordnungspunkt\s*\d|agenda item\s*\d|item\s+\d+[.:]|point\s+\d+[.:]|punto\s+\d+[.:])");
rx!(SPEAKER, r"^(?:\*\*)?[A-ZÄÖÜÉ][\p{L}.'\- ]{1,38}(?:\*\*)?:\s+\S");
rx!(CHAPTER, r"(?i)^(?:#{1,6}\s+)?(?:kapitel|chapter|chapitre|capítulo)\s+(?:\d+|[ivxlc]+|[\p{L}]+)\b|^\p{L}+s\s+kapitel\s*$|^#{1,3}\s+\S");
rx!(CHAT_LINE, r"^\[?(\d{1,4}[./-]\d{1,2}[./-]\d{1,4}),?\s+(\d{1,2}):(\d{2})");
rx!(CODE_UNIT, r"^(?:pub(?:\([a-z]+\))?\s+)?(?:async\s+|unsafe\s+|const\s+)*(?:fn|struct|enum|impl|trait|mod)\b|^(?:async\s+)?(?:def|class)\s|^(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:function|class|interface|type|const)\s|^func\s|^(?:public|private|protected)\s|^CREATE\s|^\[[\w.-]+\]\s*$");
rx!(CODE_NOTE, r"^(?://|#|/\*|\*|--|@)");

struct Rules {
    /// Line patterns, outermost level first.
    levels: Vec<&'static LazyLock<Regex>>,
    /// Marks inside a unit where a long unit may be cut.
    inner: Option<&'static LazyLock<Regex>>,
    min_chars: usize,
    max_chars: usize,
}

fn rules_for(kind: DocKind) -> Rules {
    let (levels, inner, min_chars, max_chars) = match kind {
        DocKind::Legal => (vec![&LEGAL_PART, &LEGAL_UNIT], Some(&LEGAL_INNER), 120, 8_000),
        DocKind::Minutes => (vec![&AGENDA], None, 200, 8_000),
        DocKind::Transcript => (vec![&SPEAKER], None, 400, 4_000),
        DocKind::Book => (vec![&CHAPTER], None, 200, 4_000),
        DocKind::Chat => (vec![], Some(&CHAT_LINE), 0, 4_000),
        DocKind::Code => (vec![&CODE_UNIT], None, 200, 6_000),
        _ => (vec![], None, 0, 8_000),
    };
    Rules { levels, inner, min_chars, max_chars }
}

/// Longest passage of a kind cut here.
pub fn max_chars(kind: DocKind) -> usize {
    rules_for(kind).max_chars
}

/// Whether `chunk_by_rules` has a table for this kind.
pub fn handles(kind: DocKind) -> bool {
    matches!(kind, DocKind::Legal | DocKind::Minutes | DocKind::Transcript | DocKind::Book | DocKind::Chat | DocKind::Code)
}

struct Line<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

fn scan(text: &str) -> Vec<Line<'_>> {
    let mut start = 0;
    text.split_inclusive('\n')
        .map(|raw| {
            let line = Line { start, end: start + raw.len(), text: raw.trim_end() };
            start = line.end;
            line
        })
        .collect()
}

struct Mark {
    line: usize,
    level: usize,
}

/// Marks of a chat: a new date, or a pause of more than [`CHAT_GAP_MINUTES`].
fn chat_marks(lines: &[Line]) -> Vec<Mark> {
    let mut marks = Vec::new();
    let mut last: Option<(String, i64)> = None;
    for (i, l) in lines.iter().enumerate() {
        let Some(c) = CHAT_LINE.captures(l.text) else { continue };
        let minute = c[2].parse::<i64>().unwrap_or(0) * 60 + c[3].parse::<i64>().unwrap_or(0);
        let date = c[1].to_string();
        if last.as_ref().is_none_or(|(d, m)| *d != date || minute - m > CHAT_GAP_MINUTES) {
            marks.push(Mark { line: i, level: 0 });
        }
        last = Some((date, minute));
    }
    marks
}

/// A comment or attribute directly above a definition belongs to it.
fn code_start(lines: &[Line], line: usize, floor: usize) -> usize {
    let mut s = line;
    while s > floor && CODE_NOTE.is_match(lines[s - 1].text) {
        s -= 1;
    }
    s
}

fn marks_of(lines: &[Line], kind: DocKind, rules: &Rules) -> Vec<Mark> {
    if kind == DocKind::Chat {
        return chat_marks(lines);
    }
    let mut marks: Vec<Mark> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(level) = rules.levels.iter().position(|r| r.is_match(l.text)) else { continue };
        let floor = marks.last().map_or(0, |m| m.line + 1);
        let line = if kind == DocKind::Code { code_start(lines, i, floor) } else { i };
        marks.push(Mark { line, level });
    }
    marks
}

fn title_of(kind: DocKind, line: &str) -> String {
    let t = line.trim().trim_start_matches('#').trim().trim_matches('*').trim();
    match kind {
        DocKind::Transcript => t.split(':').next().unwrap_or(t).trim_end_matches('*').to_string(),
        _ => t.chars().take(100).collect(),
    }
}

/// Ranges of one unit, each at most `max_chars`.
fn split_unit(text: &str, lines: &[Line], first: usize, end: usize, rules: &Rules) -> Vec<(usize, usize)> {
    let start = lines[first].start;
    let stop = trimmed_end(text, start, lines[end - 1].end);
    let cuts: Vec<usize> = (first + 1..end)
        .filter(|&i| {
            let text_line = !lines[i].text.trim().is_empty();
            let after_blank = lines[i - 1].text.trim().is_empty();
            text_line && (after_blank || rules.inner.is_some_and(|r| r.is_match(lines[i].text)))
        })
        .map(|i| lines[i].start)
        .collect();
    pack_at(text, start, stop, &cuts, rules.max_chars)
        .into_iter()
        .flat_map(|(s, e)| split_long_text(text, s, e, rules.max_chars))
        .collect()
}

/// Cuts `text` by the table of `kind`. A kind without one yields nothing (see [`handles`]).
pub fn chunk_by_rules(text: &str, context_id: i64, document_id: i64, kind: DocKind) -> Vec<NewChunk> {
    if !handles(kind) {
        return Vec::new();
    }
    let rules = rules_for(kind);
    let lines = scan(text);
    let marks = marks_of(&lines, kind, &rules);
    let mut bounds: Vec<(usize, Option<usize>)> = Vec::new(); // (first line, mark)
    if marks.first().is_none_or(|m| m.line > 0) {
        bounds.push((0, None));
    }
    bounds.extend(marks.iter().enumerate().map(|(i, m)| (m.line, Some(i))));

    let n_levels = rules.levels.len().max(1);
    let mut path: Vec<Option<String>> = vec![None; n_levels];
    let mut out: Vec<NewChunk> = Vec::new();
    let mut carry: Option<usize> = None;
    for (b, &(line, mark)) in bounds.iter().enumerate() {
        let end = bounds.get(b + 1).map_or(lines.len(), |x| x.0);
        if line >= end {
            continue;
        }
        if let Some(m) = mark.map(|i| &marks[i]) {
            path[m.level] = Some(title_of(kind, lines[marks_line_of_title(&lines, m.line)].text));
            path[m.level + 1..].iter_mut().for_each(|p| *p = None);
        }
        let first = carry.unwrap_or(line);
        let short = lines[first].start + rules.min_chars > lines[end - 1].end;
        if short && b + 1 < bounds.len() {
            carry = Some(first);
            continue;
        }
        carry = None;
        if lines[first..end].iter().all(|l| l.text.trim().is_empty()) {
            continue;
        }
        let heading = path.iter().flatten().cloned().collect::<Vec<_>>();
        let title = heading.last().cloned().unwrap_or_default();
        let meta = serde_json::json!({
            "chunk_type": kind.name(), "section": title, "title": title, "heading_path": heading.join(PATH_SEP),
        });
        let parts = split_unit(text, &lines, first, end, &rules);
        let n_parts = parts.len();
        for (part, (s, e)) in parts.into_iter().enumerate() {
            let slice = &text[s..e];
            if slice.is_empty() {
                continue;
            }
            let mut m = meta.clone();
            if n_parts > 1 {
                m["part"] = (part + 1).into();
                m["parts"] = n_parts.into();
            }
            out.push(NewChunk {
                context_id,
                document_id,
                chunk_index: out.len() as i64,
                char_start: s as i64,
                char_end: e as i64,
                text: slice.to_string(),
                signature: Some(slice.chars().take(80).collect()),
                is_omitted: false,
                metadata: m.to_string(),
            });
        }
    }
    out
}

/// The line naming a unit: its mark, past a comment block above a code definition.
fn marks_line_of_title(lines: &[Line], line: usize) -> usize {
    (line..lines.len()).find(|&i| !CODE_NOTE.is_match(lines[i].text) || lines[i].text.starts_with('#')).unwrap_or(line)
}
