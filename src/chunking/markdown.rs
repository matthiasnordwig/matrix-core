//! Structural chunking for Markdown (LLM-free): cut at ATX headings
//! (`#` … `######`) outside code fences, so a hit is a self-contained section.
//!
//! **Index basis:** `char_start`/`char_end` are *byte* offsets into the input
//! `text` (same basis as `signatures::assemble` and the PDF structural
//! chunker, despite the field names) and `text[char_start..char_end] ==
//! chunk.text` holds exactly — chunks are verbatim slices, never rebuilt.
//!
//! Rules: a section runs from its heading to the next heading of any level;
//! heading-only sections are merged into the following one (a trailing one into
//! the previous chunk); sections over `max_chars` are split at blank lines
//! outside code fences; a block still over `max_chars` (typically a long list
//! without blank lines) is split before its top-level list items, and failing
//! that at line ends — never inside a code fence (an oversized fence stays
//! whole); every part
//! carries the same `heading_path`. Text before the first heading is its own
//! section with empty `heading_path`. Setext headings (`===`/`---` underline)
//! are not recognised.

use crate::db::models::NewChunk;

/// Fallback section size when the caller has no profile value (~3000 chars ≈
/// 750 tokens: big enough for a whole handbook subsection, small enough for
/// embedding models with 512-token inputs to stay mostly intact).
pub const DEFAULT_MAX_CHARS: usize = 3000;

const PATH_SEP: &str = " › ";

struct Line {
    start: usize,
    /// Exclusive, including the trailing newline.
    end: usize,
    /// Whitespace-only line outside a code fence.
    blank: bool,
    /// Inside a code fence (after the opening line, closing line included):
    /// no cut may fall right before this line.
    in_fence: bool,
    /// Starts a top-level list item (`- `, `* `, `+ `, `1. `, `1) `).
    list_item: bool,
    heading: Option<(u8, String)>,
}

/// Cuts `text` into section chunks. `max_chars` of 0 means [`DEFAULT_MAX_CHARS`].
pub fn chunk_markdown_structurally(
    text: &str,
    context_id: i64,
    document_id: i64,
    max_chars: usize,
) -> Vec<NewChunk> {
    let max_chars = if max_chars == 0 { DEFAULT_MAX_CHARS } else { max_chars };
    let lines = scan_lines(text);
    let sections = build_sections(&lines);

    let mut out: Vec<NewChunk> = Vec::new();
    let mut carry: Option<usize> = None; // first line of merged heading-only sections
    for (i, sec) in sections.iter().enumerate() {
        let first = carry.unwrap_or(sec.first);
        if sec.heading_only(&lines) {
            if i + 1 < sections.len() {
                carry = Some(first);
                continue;
            }
            // Trailing heading-only section: attach to the previous chunk.
            if let Some(prev) = out.last_mut() {
                let end = trimmed_end(text, lines[first].start, lines[sec.last].end);
                prev.char_end = end as i64;
                prev.text = text[prev.char_start as usize..end].to_string();
                prev.signature = Some(signature(&prev.text));
                continue;
            }
        }
        carry = None;
        let parts = split_range(text, &lines, first, sec.last + 1, max_chars);
        let n_parts = parts.len();
        for (part, (a, b)) in parts.into_iter().enumerate() {
            let start = lines[a].start;
            let end = trimmed_end(text, start, lines[b - 1].end);
            let slice = &text[start..end];
            let mut meta = serde_json::json!({
                "chunk_type": "markdown_section",
                "section": sec.title,
                "title": sec.title,
                "heading_path": sec.path,
                "level": sec.level,
            });
            if n_parts > 1 {
                meta["part"] = serde_json::json!(part + 1);
                meta["parts"] = serde_json::json!(n_parts);
            }
            out.push(NewChunk {
                context_id,
                document_id,
                chunk_index: 0,
                char_start: start as i64,
                char_end: end as i64,
                text: slice.to_string(),
                signature: Some(signature(slice)),
                is_omitted: false,
                metadata: meta.to_string(),
            });
        }
    }
    for (i, c) in out.iter_mut().enumerate() {
        c.chunk_index = i as i64;
    }
    out
}

fn signature(text: &str) -> String {
    text.chars().take(80).collect()
}

/// End of `text[start..end]` with trailing whitespace removed.
fn trimmed_end(text: &str, start: usize, end: usize) -> usize {
    start + text[start..end].trim_end().len()
}

/// Classifies every line: blank / heading / inside a fence (fence lines included).
fn scan_lines(text: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut pos = 0;
    for raw in text.split_inclusive('\n') {
        let start = pos;
        pos += raw.len();
        let content = raw.trim_end_matches(['\n', '\r']);
        let indent = content.len() - content.trim_start_matches(' ').len();
        let body = &content[indent..];
        let mut line = Line { start, end: pos, blank: false, in_fence: false, list_item: false, heading: None };

        if let Some((ch, len)) = fence {
            line.in_fence = true;
            let run = body.chars().take_while(|&c| c == ch).count();
            if indent <= 3 && run >= len && body[run..].trim().is_empty() {
                fence = None;
            }
        } else if let Some(open) = fence_open(body).filter(|_| indent <= 3) {
            fence = Some(open);
        } else if content.trim().is_empty() {
            line.blank = true;
        } else if indent <= 3 {
            line.heading = parse_heading(body);
            line.list_item = is_list_item(body);
        }
        lines.push(line);
    }
    lines
}

/// Opening fence: 3+ backticks or tildes (a backtick fence's info string must
/// not contain a backtick).
fn fence_open(body: &str) -> Option<(char, usize)> {
    let ch = body.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let run = body.chars().take_while(|&c| c == ch).count();
    if run < 3 || (ch == '`' && body[run..].contains('`')) {
        return None;
    }
    Some((ch, run))
}

fn is_list_item(body: &str) -> bool {
    if body.starts_with("- ") || body.starts_with("* ") || body.starts_with("+ ") {
        return true;
    }
    let digits = body.chars().take_while(char::is_ascii_digit).count();
    (1..=9).contains(&digits) && (body[digits..].starts_with(". ") || body[digits..].starts_with(") "))
}

fn parse_heading(body: &str) -> Option<(u8, String)> {
    let level = body.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &body[level..];
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    let mut title = rest.trim();
    let stripped = title.trim_end_matches('#');
    if stripped.len() != title.len() && (stripped.is_empty() || stripped.ends_with([' ', '\t'])) {
        title = stripped.trim_end();
    }
    Some((level as u8, title.to_string()))
}

struct Section {
    first: usize,
    /// Inclusive.
    last: usize,
    title: String,
    path: String,
    level: u8,
    has_heading: bool,
}

impl Section {
    /// Only the heading line carries content.
    fn heading_only(&self, lines: &[Line]) -> bool {
        self.has_heading && lines[self.first + 1..=self.last].iter().all(|l| l.blank)
    }
}

fn build_sections(lines: &[Line]) -> Vec<Section> {
    let heads: Vec<usize> = (0..lines.len()).filter(|&i| lines[i].heading.is_some()).collect();
    let mut sections = Vec::new();

    let pre_end = heads.first().copied().unwrap_or(lines.len());
    if lines[..pre_end].iter().any(|l| !l.blank) {
        sections.push(Section {
            first: 0,
            last: pre_end - 1,
            title: String::new(),
            path: String::new(),
            level: 0,
            has_heading: false,
        });
    }

    let mut stack: Vec<(u8, String)> = Vec::new();
    for (k, &h) in heads.iter().enumerate() {
        let (level, title) = lines[h].heading.clone().unwrap();
        while stack.last().is_some_and(|(l, _)| *l >= level) {
            stack.pop();
        }
        stack.push((level, title.clone()));
        let path = stack
            .iter()
            .map(|(_, t)| t.as_str())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(PATH_SEP);
        let last = heads.get(k + 1).map_or(lines.len(), |&n| n) - 1;
        sections.push(Section { first: h, last, title, path, level, has_heading: true });
    }
    sections
}

/// Splits lines `[a, b)` into line ranges of at most `max_chars` each, at
/// blank-line block boundaries, then (for a still oversized block) before
/// list items, then at line ends. Leading heading-only blocks stick to the
/// following block; an oversized code fence stays whole.
fn split_range(text: &str, lines: &[Line], a: usize, b: usize, max_chars: usize) -> Vec<(usize, usize)> {
    let whole_end = trimmed_end(text, lines[a].start, lines[b - 1].end);
    if text[lines[a].start..whole_end].chars().count() <= max_chars {
        return vec![(a, b)];
    }

    // Blocks: maximal runs of non-blank lines (fence content counts as non-blank).
    let mut blocks: Vec<(usize, usize, bool)> = Vec::new(); // (first, end_excl, heading_only)
    let mut i = a;
    while i < b {
        if lines[i].blank {
            i += 1;
            continue;
        }
        let s = i;
        while i < b && !lines[i].blank {
            i += 1;
        }
        blocks.push((s, i, i - s == 1 && lines[s].heading.is_some()));
    }
    // Fuse heading-only blocks with their successor.
    let mut fused: Vec<(usize, usize)> = Vec::new();
    let mut pending: Option<usize> = None;
    for &(s, e, sticky) in &blocks {
        let start = pending.take().unwrap_or(s);
        if sticky {
            pending = Some(start);
        } else {
            fused.push((start, e));
        }
    }
    if let Some(s) = pending {
        let e = blocks.last().unwrap().1;
        fused.push((s, e));
    }

    let size = |s: usize, e: usize| -> usize {
        let end = trimmed_end(text, lines[s].start, lines[e - 1].end);
        text[lines[s].start..end].chars().count()
    };
    let units: Vec<(usize, usize)> = fused
        .into_iter()
        .flat_map(|(s, e)| {
            if size(s, e) <= max_chars {
                return vec![(s, e)];
            }
            cut_before(lines, s, e, |k| lines[k].list_item)
                .into_iter()
                .flat_map(|(us, ue)| {
                    if size(us, ue) <= max_chars {
                        vec![(us, ue)]
                    } else {
                        cut_before(lines, us, ue, |_| true)
                    }
                })
                .collect()
        })
        .collect();

    let mut parts: Vec<(usize, usize)> = Vec::new();
    for (s, e) in units {
        match parts.last_mut() {
            Some(cur) if size(cur.0, e) <= max_chars => cur.1 = e,
            _ => parts.push((s, e)),
        }
    }
    parts
}

/// Cuts lines `[s, e)` before every non-blank line `k > s` with `pred(k)` that
/// is not inside a code fence and does not directly follow a heading. The packer in `split_range` merges the pieces back up
/// to `max_chars`.
fn cut_before(lines: &[Line], s: usize, e: usize, pred: impl Fn(usize) -> bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = s;
    for k in s + 1..e {
        // A heading stays glued to the content that follows it.
        let after_heading = lines[s..k].iter().rev().find(|l| !l.blank).is_some_and(|l| l.heading.is_some());
        if !lines[k].in_fence && !lines[k].blank && !after_heading && pred(k) {
            out.push((start, k));
            start = k;
        }
    }
    out.push((start, e));
    out
}
