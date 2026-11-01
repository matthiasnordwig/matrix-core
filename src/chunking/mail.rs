//! Structural chunking for mails (LLM-free): a mail's own text and every mail embedded in it
//! (reply, forward, Lotus header block) are separate passages, so a hit belongs to one author.
//!
//! Blocks are found by their form, so any language works: a run of `Label: value` lines with a date,
//! a line ending in a colon before `>` quotes, a ruled banner before such a run. That covers Outlook,
//! Gmail, Apple Mail and Thunderbird, also as the bold lines an HTML mail turns into; the English
//! words below are the banners of older Outlook and Lotus Notes, which stand alone. The text is the
//! Markdown of `ingest/mail` (subject heading, `**From:**` lines, body). Offsets are
//! byte offsets and chunks are verbatim slices, as in `markdown.rs`. A block stays whole up to
//! [`MAIL_MAX_CHARS`], the most one vector can stand for; above that it is cut at paragraphs, then
//! as `markdown.rs` cuts a long line.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

use super::markdown::{pack_at, split_long_text, trimmed_end};
use crate::db::models::NewChunk;

/// Longest block in characters. Among 8 000, 16 000 and 30 000 this found most on the mail
/// measurement, at a third less memory than 30 000 (`handbuch/messen.md`).
pub const MAIL_MAX_CHARS: usize = 16_000;

const HEADER_SEP: &str = " › ";
/// Body lines between two markers that still count as one block's header (a wrapped banner line).
const MAX_LINES_BETWEEN_MARKERS: usize = 1;
/// How far a marker looks ahead for the rest of its header block.
const HEADER_LOOKAHEAD: usize = 5;
/// A Lotus block starts at the sender and date lines above its `To:`.
const MAX_SENDER_LINES: usize = 3;
const MAX_GAP_ABOVE_TO: usize = 2;
/// Bold marks, escapes and direction marks of Arabic and Hebrew mails, and the full-width colon of
/// Japanese and Chinese ones (it becomes a plain colon).
const MARKS: [char; 12] = ['*', '\\', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2069}'];
/// Fewest `Label: value` lines in a row that make a mail header.
const MIN_HEADER_LINES: usize = 3;
/// Lines of a wrapped recipient list inside such a run.
const MAX_WRAPPED_LINES: usize = 2;
/// A line shorter than this is the date as a whole, with its time.
const MAX_DATE_LINE_CHARS: usize = 40;
/// Longest line that can announce a block ("On … wrote:", a banner).
const MAX_INTRO_CHARS: usize = 240;
/// Longest text behind a rule that still counts as a footer.
const MAX_FOOTER_CHARS: usize = 2_000;
/// Shorter passages are not tracked as repeats.
const MIN_DUPLICATE_CHARS: usize = 200;

static ORIGINAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^-{2,}\s*original message\s*-{2,}").unwrap());
static FORWARD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^-{2,}\s*forwarded by\s+(.+?)(\s+on\s|$)").unwrap());
/// `Label: value` in any language; the value may be empty (`cc:`). A space before the colon is French.
static HEADER_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\p{L}[\p{L} -]{0,18}?\s?:(\s+(.*))?$|(?i)^please respond to\b|^[ap]m -{3,}").unwrap());
/// A ruled line with a few words: `-----Ursprüngliche Nachricht-----`, `---- Forwarded message ----`.
static BANNER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-_=]{2,}\s*\p{L}.{0,50}?\s*[-_=]{2,}$").unwrap());
/// A date or time in any language.
static DATED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d{1,2}[./-]\d{1,2}[./-]\d{2,4}|\d{1,2}:\d{2}|\b\d{1,2}\.?\s\p{L}{3,}\.?\s\d{4}\b|\p{L}{3,}\.?\s\d{1,2},?\s\d{4}\b|\d{4}\s?年\s?\d{1,2}\s?月\s?\d{1,2}\s?日").unwrap()
});
static TRAILING_NOISE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^([-_=]{2,}\s*\p{L}.{0,50}?\s*[-_=]{2,}|<embedded .*>|-+\s*inline attachment follows\s*-+|\(revision: \d+\))$").unwrap());
/// Where a gateway's or list's footer starts: a ruled line, or its first words.
static FOOTER_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^([*_=-]{20,}|confidentiality notice|this e-?mail is the property|this message (and any attachments )?(is|are|may) .*confidential|to unsubscribe\b|get your free download|sent from my |von meinem .* gesendet|diese e-?mail .*(vertraulich|enthält)|gesendet von )").unwrap()
});

static DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d{1,2}/\d{1,2}/\d{2,4}(\s+\d{1,2}:\d{2}(:\d{2})?\s*(AM|PM)?)?").unwrap());

struct Line<'a> {
    start: usize,
    end: usize,
    /// Without leading spaces and quote marks.
    text: &'a str,
    /// `text` without the bold marks and escapes of a converted HTML mail, for finding markers.
    head: Cow<'a, str>,
    /// Starts with a `>` quote mark.
    raw_quoted: bool,
}

fn scan(text: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for raw in text.split_inclusive('\n') {
        let end = start + raw.len();
        let text = raw.trim_start_matches([' ', '\t', '>']).trim_end();
        let head = if text.contains(MARKS) || text.contains('：') { Cow::Owned(text.replace(MARKS, "").replace('：', ":")) } else { Cow::Borrowed(text) };
        let raw_quoted = raw.trim_start_matches([' ', '\t']).starts_with('>');
        lines.push(Line { start, end, text, head, raw_quoted });
        start = end;
    }
    lines
}

/// First line of the body: past the subject heading and the `**From:**` lines.
fn body_start(lines: &[Line]) -> usize {
    let mut i = 0;
    while i < lines.len() && (lines[i].text.is_empty() || lines[i].text.starts_with("# ")) {
        i += 1;
    }
    while i < lines.len() && lines[i].text.starts_with("**") {
        i += 1;
    }
    i
}

/// Number of lines of the `Label: value` run from `i`, wrapped lines included; 0 if it is too short.
fn header_run(lines: &[Line], i: usize) -> usize {
    let (mut labels, mut wrapped, mut j) = (0, 0, i);
    while let Some(l) = lines.get(j).filter(|l| !l.text.is_empty()) {
        if HEADER_LINE.is_match(&l.head) {
            labels += 1;
            wrapped = 0;
        } else {
            wrapped += 1;
            if labels == 0 || wrapped > MAX_WRAPPED_LINES {
                break;
            }
        }
        j += 1;
    }
    if labels >= MIN_HEADER_LINES { j - wrapped - i } else { 0 }
}

/// A line announcing the quote below it: ends in a colon, names a time or an address, `>` follows.
fn is_reply_line(lines: &[Line], i: usize) -> bool {
    let t = lines[i].head.as_ref();
    let next = lines[i + 1..].iter().find(|l| !l.text.is_empty());
    t.ends_with(':') && (DATED.is_match(t) || t.contains('@')) && next.is_some_and(|l| l.raw_quoted)
}

/// Start of a header block at line `i`: the banner or announcing line just above it, if any.
fn with_banner(lines: &[Line], i: usize, floor: usize) -> usize {
    let above = (floor..i).rev().take(MAX_GAP_ABOVE_TO + 1).find(|&k| !lines[k].text.is_empty());
    above.filter(|&k| BANNER.is_match(&lines[k].head) || lines[k].head.ends_with(':') && lines[k].head.len() < 60).unwrap_or(i)
}

/// The Lotus sender and date lines above a `To:` line, if the first of them looks like one. An
/// empty line may stand between them and the `To:` (Lotus often writes one with only tabs).
fn lotus_start(lines: &[Line], to: usize, floor: usize) -> usize {
    let mut top = to;
    while top > floor && to - top < MAX_GAP_ABOVE_TO && lines[top - 1].text.is_empty() {
        top -= 1;
    }
    let mut s = top;
    while s > floor && top - s < MAX_SENDER_LINES && !lines[s - 1].text.is_empty() {
        s -= 1;
    }
    let above = &lines[s..top];
    if above.iter().any(|l| DATE.is_match(l.text) || l.text.contains('@')) { s } else { to }
}

/// Lines where an embedded mail starts, in order.
fn block_starts(lines: &[Line], body: usize) -> Vec<usize> {
    let mut starts: Vec<usize> = Vec::new();
    for i in body..lines.len() {
        let t = lines[i].head.as_ref();
        let floor = starts.last().map_or(body, |&s| s + 1);
        let run = header_run(lines, i);
        let start = if ORIGINAL.is_match(t) || FORWARD.is_match(t) {
            Some(i)
        } else if run > 0 && lines[i..i + run].iter().any(|l| DATED.is_match(&l.head)) {
            Some(with_banner(lines, i, floor))
        } else if run > 0 {
            Some(lotus_start(lines, i, floor)).filter(|&s| s < i)
        } else if t.len() < MAX_INTRO_CHARS && (t.ends_with("wrote:") || is_reply_line(lines, i)) {
            Some(i)
        } else {
            None
        };
        let Some(s) = start else { continue };
        let same_block = starts.last().is_some_and(|&prev| {
            let body_lines = lines[prev + 1..s]
                .iter()
                .filter(|l| !l.text.is_empty() && !HEADER_LINE.is_match(&l.head) && !DATE.is_match(&l.head))
                .count();
            body_lines <= MAX_LINES_BETWEEN_MARKERS
        });
        if !same_block {
            starts.push(s);
        }
    }
    starts
}

/// Sender and date of an embedded mail, read from its first lines.
fn quoted_author(lines: &[Line]) -> (Option<String>, Option<String>) {
    let head: Vec<&str> = lines.iter().map(|l| l.head.as_ref()).filter(|t| !t.is_empty()).take(HEADER_LOOKAHEAD + 1).collect();
    let first = head.iter().position(|t| !BANNER.is_match(t) && !t.ends_with(':')).unwrap_or(0);
    let value = |t: &str| t.split_once(':').map(|(_, v)| v.trim().to_string()).filter(|v| !v.is_empty());
    let dated = head.iter().position(|t| DATED.is_match(t));
    let date = dated.map(|i| {
        let line = head[i];
        let short = line.len() < MAX_DATE_LINE_CHARS;
        value(line).filter(|_| HEADER_LINE.is_match(line)).unwrap_or_else(|| {
            if short { line.trim().to_string() } else { DATED.find(line).map_or(String::new(), |m| m.as_str().to_string()) }
        })
    });
    // Sender first, then the date: the order of every mail program, in every language. A banner
    // says as much; a Lotus block starts with the recipient and has neither.
    let banner = first > 0 && !FORWARD.is_match(head[0]);
    let leads = |t: &&&str| HEADER_LINE.is_match(t) && (banner || dated.is_some_and(|d| d <= first + 1));
    let named = head.get(first).filter(leads).and_then(|t| value(t));
    let from = named
        .or_else(|| head.iter().find_map(|t| FORWARD.captures(t).map(|c| c[1].trim().to_string())))
        .or_else(|| head.get(first).filter(|t| !t.starts_with("--") && !HEADER_LINE.is_match(t)).map(|t| t.to_string()));
    (from, date)
}

/// One block as ranges of at most [`MAIL_MAX_CHARS`]: at paragraphs from line `cut_from` on, then like a long line.
fn split_block(text: &str, lines: &[Line], first: usize, cut_from: usize, end: usize) -> Vec<(usize, usize)> {
    let start = lines[first].start;
    let stop = trimmed_end(text, start, lines[end - 1].end);
    let paragraphs: Vec<usize> =
        (cut_from..end).filter(|&i| lines[i - 1].text.is_empty() && !lines[i].text.is_empty()).map(|i| lines[i].start).collect();
    pack_at(text, start, stop, &paragraphs, MAIL_MAX_CHARS)
        .into_iter()
        .flat_map(|(s, e)| split_long_text(text, s, e, MAIL_MAX_CHARS))
        .collect()
}

/// Text of a `**Label:** value` header line of the mail.
fn header_value<'a>(lines: &'a [Line], body: usize, label: &str) -> Option<&'a str> {
    let key = format!("**{label}:** ");
    lines[..body].iter().find_map(|l| l.text.strip_prefix(key.as_str())).map(str::trim)
}

/// What a folder name says, across the languages of mail programs.
fn folder_kind(folder: &str) -> &'static str {
    let last = folder.rsplit(['\\', '/']).find(|s| !s.is_empty()).unwrap_or("").to_lowercase();
    let is = |names: &[&str]| names.iter().any(|n| last.contains(n));
    if is(&["inbox", "eingang", "réception", "entrada"]) {
        "inbox"
    } else if is(&["sent", "gesendet", "envoy", "enviad", "postausgang"]) {
        "sent"
    } else if is(&["deleted", "trash", "gelöscht", "papierkorb", "supprim", "eliminad"]) {
        "deleted"
    } else if is(&["draft", "entwürfe", "entwurf", "brouillon", "borrador"]) {
        "drafts"
    } else {
        "other"
    }
}

/// First line after the header of a passage: a mail's `**From:**` block, or the marker and header
/// lines of an embedded mail. The body alone says whether two passages are the same mail: a mail
/// quoted in a reply carries another header than the mail itself.
fn after_header(lines: &[Line]) -> usize {
    let mut i = 0;
    let mut in_header = false;
    while let Some(l) = lines.get(i) {
        let t = l.head.as_ref();
        let marker = t.is_empty() || l.text.starts_with("# ") || l.text.starts_with("**") || ORIGINAL.is_match(t) || FORWARD.is_match(t) || BANNER.is_match(t);
        let head = HEADER_LINE.is_match(t) || t.starts_with("---") || (t.ends_with(':') && t.len() < MAX_INTRO_CHARS);
        if marker || head {
            in_header |= head;
            i += 1;
            continue;
        }
        // Lotus puts sender and date lines above its `To:`.
        let lotus = !in_header && i + MAX_SENDER_LINES < lines.len() && lines[i + 1..=i + MAX_SENDER_LINES].iter().any(|l| HEADER_LINE.is_match(&l.head));
        if !lotus {
            break;
        }
        i += 1;
    }
    i
}

/// Key under which a mail passage counts as a repeat: its body with all white space and quote marks
/// folded, so a wrapped quote matches its original. A short body says too little alone, so it counts
/// together with its header; `None` for passages short even then and for other chunks.
pub fn duplicate_key(chunk: &NewChunk) -> Option<String> {
    if !chunk.metadata.contains("\"mail_message\"") && !chunk.metadata.contains("\"mail_quoted\"") {
        return None;
    }
    let lines = scan(&chunk.text);
    let body = fold(without_footer(&lines[after_header(&lines).min(lines.len())..]));
    let clean = if body.chars().count() >= MIN_DUPLICATE_CHARS { body } else { fold(&lines) };
    (clean.chars().count() >= MIN_DUPLICATE_CHARS).then(|| crate::db::ontology::evidence_carry::text_fingerprint(&clean))
}

/// The text of `lines` without filing lines, white space folded.
fn fold(lines: &[Line]) -> String {
    lines.iter().filter(|l| !is_filing(l.text)).flat_map(|l| l.text.split_whitespace()).collect::<Vec<_>>().join(" ")
}

/// Body of a passage as `duplicate_key` compares it.
pub fn passage_body(text: &str) -> String {
    let lines = scan(text);
    fold(without_footer(&lines[after_header(&lines).min(lines.len())..]))
}

/// A body without what mail systems add at its end, which differs from copy to copy.
fn without_footer<'a, 'b>(lines: &'a [Line<'b>]) -> &'a [Line<'b>] {
    let mut end = lines.len();
    while end > 0 && (lines[end - 1].text.is_empty() || TRAILING_NOISE.is_match(lines[end - 1].text)) {
        end -= 1;
    }
    let mut tail_chars = 0;
    for i in (0..end).rev() {
        tail_chars += lines[i].text.len();
        if tail_chars > MAX_FOOTER_CHARS {
            break;
        }
        if FOOTER_START.is_match(lines[i].text) {
            return without_footer(&lines[..i]);
        }
    }
    &lines[..end]
}

/// What the search model reads of a passage: an embedded mail with the subject and sender it
/// lacks itself; a mail's own passage already starts with them.
pub fn embedding_text<'a>(text: &'a str, metadata: &str) -> Cow<'a, str> {
    let meta: serde_json::Value = serde_json::from_str(metadata).unwrap_or_default();
    match (meta["chunk_type"].as_str(), meta["heading_path"].as_str()) {
        (Some("mail_quoted"), Some(path)) if !path.is_empty() => Cow::Owned(format!("{path}\n\n{text}")),
        _ => Cow::Borrowed(text),
    }
}

/// A header line that says where this copy is filed, not what it says.
fn is_filing(line: &str) -> bool {
    line.starts_with("**Folder:** ") || line.starts_with("**Mailbox:** ")
}

/// Cuts one mail into passages.
pub fn chunk_mail(text: &str, context_id: i64, document_id: i64) -> Vec<NewChunk> {
    let lines = scan(text);
    if lines.is_empty() {
        return Vec::new();
    }
    let subject = lines.iter().find_map(|l| l.text.strip_prefix("# ")).unwrap_or("").trim().to_string();
    let body = body_start(&lines);
    let folder = header_value(&lines, body, "Folder");
    let mail_box = header_value(&lines, body, "Mailbox");
    let mut bounds = block_starts(&lines, body);
    // A forward without own text: the mail's header belongs to the block it forwards.
    let own_text = bounds.first().is_none_or(|&s| lines[body..s].iter().any(|l| !l.text.is_empty()));
    if !own_text {
        bounds.remove(0);
    }
    let mut firsts = vec![0];
    firsts.extend(bounds);
    let mut out = Vec::new();
    for (b, &first) in firsts.iter().enumerate() {
        let end = firsts.get(b + 1).copied().unwrap_or(lines.len());
        if lines[first..end].iter().all(|l| l.text.is_empty()) {
            continue;
        }
        let mut meta = serde_json::json!({ "section": subject, "title": subject });
        if let Some(f) = folder {
            meta["folder"] = f.into();
            meta["folder_kind"] = folder_kind(f).into();
        }
        if let Some(m) = mail_box {
            meta["mailbox"] = m.into();
        }
        if b == 0 {
            meta["chunk_type"] = "mail_message".into();
            meta["heading_path"] = subject.clone().into();
        } else {
            let (from, date) = quoted_author(&lines[first..end]);
            let path = [Some(subject.as_str()), from.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(HEADER_SEP);
            meta["chunk_type"] = "mail_quoted".into();
            meta["heading_path"] = path.into();
            meta["quoted_from"] = from.into();
            meta["quoted_date"] = date.into();
        }
        // The mail's header stays with its first paragraph.
        let cut_from = if b == 0 { (body + 1).max(first + 1) } else { first + 1 };
        let mut parts = split_block(text, &lines, first, cut_from, end);
        if b == 0 && parts.len() > 1 && body < lines.len() && parts[0].1 <= lines[body].start {
            parts[1].0 = parts[0].0; // the sentence split takes the header for a sentence of its own
            parts.remove(0);
        }
        let n_parts = parts.len();
        for (part, (start, stop)) in parts.into_iter().enumerate() {
            let slice = &text[start..stop];
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
                char_start: start as i64,
                char_end: stop as i64,
                text: slice.to_string(),
                signature: Some(slice.chars().take(80).collect()),
                is_omitted: false,
                metadata: m.to_string(),
            });
        }
    }
    out
}
