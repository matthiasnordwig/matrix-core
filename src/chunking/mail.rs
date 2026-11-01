//! Structural chunking for mails (LLM-free): a mail's own text and every mail embedded in it
//! (reply, forward, Lotus header block) are separate passages, so a hit belongs to one author.
//!
//! Blocks are found by their form, so any language works: a run of `Label: value` lines with one
//! date, a line ending in a colon before `>` quotes, a ruled banner before such a run. A run without
//! a date counts with a sender and date above it or with recipients (Lotus Notes); lists, schedules
//! and signatures do not count, whatever their dates. That covers Outlook, Gmail, Apple Mail and
//! Thunderbird, also as the bold lines an HTML mail turns into; the English words below are the
//! banners of older Outlook and Lotus Notes, which stand alone. The text is the
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
/// Lines inside such a run that do not look like a wrapped recipient list.
const MAX_WRAPPED_LINES: usize = 2;
/// Empty lines inside such a run, between two of its lines.
const MAX_HEADER_GAP: usize = 4;
/// Most lines of a run; raw headers come close to this, tables go beyond it.
const MAX_HEADER_LABELS: usize = 24;
/// Without a date and without a sender above it, a run needs this many recipient-like values.
const MIN_ADDRESSED: usize = 2;
/// Longest value of a line that holds the date.
const MAX_DATE_VALUE_CHARS: usize = 60;
/// A value that starts with a digit and has this many, dates aside, is a phone number.
const MIN_PHONE_DIGITS: usize = 7;
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
    LazyLock::new(|| Regex::new(r"^\p{L}[\p{L} -]{0,18}?\s?:([\s?]+(.*))?$|(?i)^please respond to\b|^[ap]m -{3,}").unwrap());
/// A ruled line with a few words: `-----Ursprüngliche Nachricht-----`, `---- Forwarded message ----`.
static BANNER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-_=]{2,}\s*\p{L}.{0,50}?\s*[-_=]{2,}$").unwrap());
/// A date or time in any language.
static DATED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d{1,2}[./-]\d{1,2}[./-]\d{2,4}|\d{1,2}:\d{2}|\b\d{1,2}\.?\s\p{L}{3,}\.?\s\d{4}\b|\p{L}{3,}\.?\s\d{1,2},?\s\d{4}\b|\d{4}\s?[年년]\s?\d{1,2}\s?[月월]\s?\d{1,2}\s?[日일]").unwrap()
});
/// The small word before a date behind a sender: `on`, `am`, `le`.
static SENDER_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+\p{Ll}{1,3}$").unwrap());
static NOTES_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{L}/[\p{L}&_ -]+/\p{L}").unwrap());
/// A web address as a value.
static CONTACT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(https?://|www\.)").unwrap());
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

/// The text behind a line's label.
fn label_value(head: &str) -> &str {
    head.split_once(':').map_or("", |(_, v)| v.trim())
}

/// How many dates and how many clock times a line names.
fn moments(t: &str) -> (usize, usize) {
    DATED.find_iter(t).fold((0, 0), |(d, c), m| if m.as_str().contains(':') { (d, c + 1) } else { (d + 1, c) })
}

/// A phone number or web address, the values of a signature and never those of a mail header.
fn is_contact(head: &str) -> bool {
    let value = label_value(head);
    let digits = DATED.replace_all(value, "").chars().filter(char::is_ascii_digit).count();
    let leads = value.starts_with(|c: char| c == '+' || c == '(' || c.is_ascii_digit());
    (leads && digits >= MIN_PHONE_DIGITS) || CONTACT.is_match(value)
}

/// A mail address, or a Notes name such as `Anna Maier/HOU/ECT`.
fn names_address(t: &str) -> bool {
    t.contains('@') || NOTES_NAME.is_match(t)
}

/// A run of `Label: value` lines: where it ends and what it holds.
#[derive(Default)]
struct Run {
    /// The line after its last label.
    end: usize,
    labels: usize,
    /// A line names the moment of sending.
    dated: bool,
    /// A list, not a header: a label comes twice, or moments come in a row or several in a line,
    /// as in `Time: Fri 6/1 at 8:00 PM thru Sat 6/2 at 1:00 AM`.
    listing: bool,
    /// Values that hold an address, as recipients do.
    addressed: usize,
    /// Labels without a value, as `cc:` often is.
    empty: usize,
    contact: bool,
}

/// The run from line `i`. Lotus leaves empty lines inside its headers, and long recipient lists wrap
/// over many lines; a line that looks like neither ends the run after [`MAX_WRAPPED_LINES`].
fn header_run(lines: &[Line], i: usize) -> Run {
    let mut run = Run { end: i, ..Run::default() };
    let mut seen: Vec<String> = Vec::new();
    let (mut j, mut wrapped, mut after_moment) = (i, 0, false);
    while let Some(l) = lines.get(j) {
        let t = l.head.as_ref();
        if l.text.is_empty() {
            let next = (j + 1..lines.len()).take(MAX_HEADER_GAP).find(|&k| !lines[k].text.is_empty());
            let Some(next) = next.filter(|&k| run.labels > 0 && HEADER_LINE.is_match(&lines[k].head)) else { break };
            j = next;
            continue;
        }
        if HEADER_LINE.is_match(t) {
            let label = t.split(':').next().unwrap_or(t).trim().to_lowercase();
            if seen.contains(&label) {
                run.listing = true;
                break;
            }
            if run.labels == MAX_HEADER_LABELS {
                break;
            }
            seen.push(label);
            run.labels += 1;
            let (dates, clocks) = moments(t);
            let moment = dates == 1 && label_value(t).chars().count() <= MAX_DATE_VALUE_CHARS;
            run.listing |= dates > 1 || clocks > 1 || after_moment && dates + clocks > 0;
            run.dated |= moment;
            after_moment = moment;
            run.empty += usize::from(label_value(t).is_empty());
            run.addressed += usize::from(names_address(t));
            run.contact |= is_contact(t);
            wrapped = 0;
            j += 1;
            run.end = j;
        } else {
            wrapped += 1;
            if run.labels == 0 || BANNER.is_match(t) || FORWARD.is_match(t) {
                break;
            }
            run.listing |= after_moment && moments(t).0 > 0;
            run.addressed += usize::from(names_address(t));
            if wrapped > MAX_WRAPPED_LINES && !t.contains(['@', ',', ';', '/']) {
                break;
            }
            j += 1;
        }
    }
    run
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

/// Where the mail starts whose header the run at line `i` is, if it is one. With a date it is one.
/// Without, it needs a sender above it, or recipients: addresses, or an empty `cc:` among filled lines.
fn header_start(lines: &[Line], run: &Run, i: usize, floor: usize) -> Option<usize> {
    if run.contact {
        return None;
    }
    if run.dated && !run.listing {
        return Some(with_banner(lines, i, floor));
    }
    let s = lotus_start(lines, i, floor);
    let recipients = run.addressed >= MIN_ADDRESSED || !run.listing && (s < i || run.empty > 0 && run.empty * 2 < run.labels);
    recipients.then_some(s)
}

/// Lines where an embedded mail starts, in order.
fn block_starts(lines: &[Line], body: usize) -> Vec<usize> {
    let mut starts: Vec<usize> = Vec::new();
    // End of the last marker, and whether it was a header (a banner still waits for one).
    let (mut marker_end, mut after_header) = (body, false);
    let mut i = body;
    while i < lines.len() {
        let t = lines[i].head.as_ref();
        let floor = starts.last().map_or(body, |&s| s + 1);
        let run = header_run(lines, i);
        let found = if ORIGINAL.is_match(t) || FORWARD.is_match(t) {
            Some((i, banner_end(lines, i), false))
        } else if run.labels < MIN_HEADER_LINES {
            (t.len() < MAX_INTRO_CHARS && (t.ends_with("wrote:") || is_reply_line(lines, i))).then_some((i, i + 1, false))
        } else if let Some(s) = header_start(lines, &run, i, floor) {
            Some((s, run.end, true))
        } else {
            // No part of a list or a signature is a header either.
            i = lines[i..run.end].iter().position(|l| l.text.is_empty()).map_or(run.end, |n| i + n);
            continue;
        };
        let Some((s, end, header)) = found else {
            i += 1;
            continue;
        };
        let text_above = lines[marker_end.min(s)..s].iter().any(|l| !l.text.is_empty());
        if starts.is_empty() || text_above {
            starts.push(s);
        } else if after_header {
            // A forward without a word of its own: the text below is by the later header.
            *starts.last_mut().unwrap() = s;
        }
        (marker_end, after_header) = (end, header);
        i = end;
    }
    // A last header with nothing below it says nothing of its own; it stays with the text above.
    if lines[marker_end.min(lines.len())..].iter().all(|l| l.text.is_empty()) {
        starts.pop();
    }
    starts
}

/// First line behind the banner at `i`; a long one wraps, and its last line ends in the rule.
fn banner_end(lines: &[Line], i: usize) -> usize {
    if lines[i].head.ends_with("---") || !lines.get(i + 1).is_some_and(|l| l.head.ends_with("---")) { i + 1 } else { i + 2 }
}

/// Sender and date of an embedded mail, read from its first lines.
fn quoted_author(lines: &[Line], subject: &str) -> (Option<String>, Option<String>) {
    let head: Vec<&str> = lines.iter().map(|l| l.head.as_ref()).filter(|t| !t.is_empty()).take(HEADER_LOOKAHEAD + 1).collect();
    let mut first = head.iter().position(|t| !BANNER.is_match(t) && !FORWARD.is_match(t) && !t.ends_with(':') && !t.ends_with("---")).unwrap_or(0);
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
    let forward = head.first().is_some_and(|t| FORWARD.is_match(t));
    // Thunderbird and cc:Mail start with the subject, whatever its label says.
    if head.get(first).and_then(|t| value(t)).is_some_and(|v| same_subject(&v, subject)) {
        first += 1;
        first += usize::from(head.get(first).is_some_and(|t| DATED.is_match(t)));
    }
    let banner = first > 0 && !forward;
    let leads = |t: &&&str| HEADER_LINE.is_match(t) && (banner || dated.is_some_and(|d| d <= first + 1));
    let named = head.get(first).filter(leads).and_then(|t| value(t)).and_then(|t| sender_only(&t));
    // A sender line with the date on it or below it, also under a forward: the text is the sender's.
    let is_sender = |t: &&&str| (first == 0 || forward) && !HEADER_LINE.is_match(t) && !t.starts_with("--") && head[first..].iter().take(2).any(|l| DATED.is_match(l));
    let from = named
        .or_else(|| head.get(first).filter(is_sender).and_then(|t| sender_only(t)))
        .or_else(|| head.iter().find_map(|t| FORWARD.captures(t).map(|c| c[1].trim().to_string())))
        .or_else(|| head.first().filter(|t| t.ends_with(':')).map(|t| t.trim_end_matches(':').trim().to_string()))
        .or_else(|| head.get(first).filter(|t| !t.starts_with("--") && !HEADER_LINE.is_match(t)).and_then(|t| sender_only(t)));
    (from, date)
}

/// Whether two subjects are the same but for `Re:` and `Fwd:`, in any language.
fn same_subject(a: &str, b: &str) -> bool {
    let bare = |s: &str| s.rsplit([':', '：']).next().unwrap_or("").trim().to_lowercase();
    !bare(a).is_empty() && bare(a) == bare(b)
}

/// A sender without what follows the name: Lotus writes date and recipients behind it. One word
/// before a date is its weekday, not a name.
fn sender_only(line: &str) -> Option<String> {
    let Some(date) = DATED.find(line) else { return Some(line.to_string()) };
    let name = SENDER_LINK.replace(line[..date.start()].trim(), "");
    (name.contains(char::is_whitespace) || names_address(&name)).then(|| name.to_string())
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
            let (from, date) = quoted_author(&lines[first..end], &subject);
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
