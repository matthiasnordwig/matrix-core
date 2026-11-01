//! Structural chunking for mails (LLM-free): a mail's own text and every mail embedded in it
//! (reply, forward, Lotus header block) are separate passages, so a hit belongs to one author.
//!
//! The text is the Markdown of `ingest/mail` (subject heading, `**From:**` lines, body). Offsets are
//! byte offsets and chunks are verbatim slices, as in `markdown.rs`. A block stays whole, whatever
//! its length: search reaches the topics of a long one through `chunking/topics.rs`.

use std::sync::LazyLock;

use regex::Regex;

use super::markdown::trimmed_end;
use crate::db::models::NewChunk;

const HEADER_SEP: &str = " › ";
/// Body lines between two markers that still count as one block's header (a wrapped banner line).
const MAX_LINES_BETWEEN_MARKERS: usize = 1;
/// How far a marker looks ahead for the rest of its header block.
const HEADER_LOOKAHEAD: usize = 5;
/// A Lotus block starts at the sender and date lines above its `To:`.
const MAX_SENDER_LINES: usize = 3;
/// Longest text behind a rule that still counts as a footer.
const MAX_FOOTER_CHARS: usize = 2_000;
/// Shorter passages are not tracked as repeats.
const MIN_DUPLICATE_CHARS: usize = 200;

static ORIGINAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^-{2,}\s*original message\s*-{2,}").unwrap());
static FORWARD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^-{2,}\s*forwarded by\s+(.+?)(\s+on\s|$)").unwrap());
static HEADER_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(from|sent|date|to|cc|bcc|subject|importance|priority|reply-to|sent by):|^please respond to\b|^[ap]m -{3,}").unwrap());
/// Lines that carry nothing of a mail's own and differ between copies, at its end.
static TRAILING_NOISE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(-{2,}\s*original message\s*-{2,}|<embedded .*>|-+\s*inline attachment follows\s*-+|\(revision: \d+\))$").unwrap());
/// Where a gateway's or list's footer starts: a ruled line, or its first words.
static FOOTER_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^([*_=-]{20,}|confidentiality notice|this e-?mail is the property|this message (and any attachments )?(is|are|may) .*confidential|to unsubscribe\b|get your free download)").unwrap()
});

static DATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d{1,2}/\d{1,2}/\d{2,4}(\s+\d{1,2}:\d{2}(:\d{2})?\s*(AM|PM)?)?").unwrap());
static FROM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^from:\s*(.+)$").unwrap());
static SENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(sent|date):\s*(.+)$").unwrap());

struct Line<'a> {
    start: usize,
    end: usize,
    /// Without leading spaces and quote marks.
    text: &'a str,
}

fn scan(text: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for raw in text.split_inclusive('\n') {
        let end = start + raw.len();
        lines.push(Line { start, end, text: raw.trim_start_matches([' ', '\t', '>']).trim_end() });
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

fn starts_with_key(line: &str, key: &str) -> bool {
    line.get(..key.len()).is_some_and(|p| p.eq_ignore_ascii_case(key))
}

fn followed_by(lines: &[Line], i: usize, keys: &[&str]) -> bool {
    lines[i + 1..].iter().take(HEADER_LOOKAHEAD).any(|l| keys.iter().any(|k| starts_with_key(l.text, k)))
}

/// The Lotus sender and date lines directly above a `To:` line, if the first of them looks like one.
fn lotus_start(lines: &[Line], to: usize, floor: usize) -> usize {
    let mut s = to;
    while s > floor && to - s < MAX_SENDER_LINES && !lines[s - 1].text.is_empty() {
        s -= 1;
    }
    let above = &lines[s..to];
    if above.iter().any(|l| DATE.is_match(l.text) || l.text.contains('@')) { s } else { to }
}

/// Lines where an embedded mail starts, in order.
fn block_starts(lines: &[Line], body: usize) -> Vec<usize> {
    let mut starts: Vec<usize> = Vec::new();
    for i in body..lines.len() {
        let t = lines[i].text;
        let start = if ORIGINAL.is_match(t) || FORWARD.is_match(t) {
            Some(i)
        } else if starts_with_key(t, "from:") && followed_by(lines, i, &["sent:", "date:", "to:"]) {
            Some(i)
        } else if starts_with_key(t, "to:") && followed_by(lines, i, &["cc:", "subject:"]) {
            Some(lotus_start(lines, i, starts.last().map_or(body, |&s| s + 1)))
        } else if t.len() < 160 && t.ends_with("wrote:") {
            Some(i)
        } else {
            None
        };
        let Some(s) = start else { continue };
        let same_block = starts.last().is_some_and(|&prev| {
            let body_lines = lines[prev + 1..s]
                .iter()
                .filter(|l| !l.text.is_empty() && !HEADER_LINE.is_match(l.text) && !DATE.is_match(l.text))
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
    let head: Vec<&str> = lines.iter().map(|l| l.text).filter(|t| !t.is_empty()).take(HEADER_LOOKAHEAD + 1).collect();
    let from = head
        .iter()
        .find_map(|t| FROM.captures(t).or_else(|| FORWARD.captures(t)).map(|c| c[1].trim().to_string()))
        .or_else(|| head.first().filter(|t| !t.starts_with("--") && !HEADER_LINE.is_match(t)).map(|t| t.to_string()));
    let date = head
        .iter()
        .find_map(|t| SENT.captures(t).map(|c| c[2].trim().to_string()).or_else(|| DATE.find(t).map(|m| m.as_str().to_string())));
    (from, date)
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
        let t = l.text;
        let marker = t.is_empty() || t.starts_with("# ") || t.starts_with("**") || ORIGINAL.is_match(t) || FORWARD.is_match(t);
        let head = HEADER_LINE.is_match(t) || t.starts_with("---") || t.ends_with("wrote:");
        if marker || head {
            in_header |= head;
            i += 1;
            continue;
        }
        // Lotus puts sender and date lines above its `To:`.
        let lotus = !in_header && i + MAX_SENDER_LINES < lines.len() && lines[i + 1..=i + MAX_SENDER_LINES].iter().any(|l| starts_with_key(l.text, "to:"));
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
        let start = lines[first].start;
        let stop = trimmed_end(text, start, lines[end - 1].end);
        let slice = &text[start..stop];
        out.push(NewChunk {
            context_id,
            document_id,
            chunk_index: out.len() as i64,
            char_start: start as i64,
            char_end: stop as i64,
            text: slice.to_string(),
            signature: Some(slice.chars().take(80).collect()),
            is_omitted: false,
            metadata: meta.to_string(),
        });
    }
    out
}
