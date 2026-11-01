//! Which kind of document a text is, read from its lines (LLM-free, the same on every platform).
//!
//! `detect_kind` counts line markers in a sample of the text and takes the kind with the clearest
//! lead; without a lead the document is `Other` and gets the paragraph chunker. Mails and
//! spreadsheets are known from their file format and never reach this code. `is_plausible` checks
//! a chunker's result afterwards, so a wrong kind costs no more than the paragraph cut.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::db::models::NewChunk;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocKind {
    Mail,
    Legal,
    Report,
    Manual,
    Minutes,
    Transcript,
    Slides,
    Book,
    Chat,
    Code,
    Form,
    Sheet,
    Other,
}

impl DocKind {
    pub const ALL: [DocKind; 13] = [
        DocKind::Mail,
        DocKind::Legal,
        DocKind::Report,
        DocKind::Manual,
        DocKind::Minutes,
        DocKind::Transcript,
        DocKind::Slides,
        DocKind::Book,
        DocKind::Chat,
        DocKind::Code,
        DocKind::Form,
        DocKind::Sheet,
        DocKind::Other,
    ];

    /// The stored name, in `documents.chunker` and the `chunker` metadata of a passage.
    pub fn name(self) -> &'static str {
        match self {
            DocKind::Mail => "mail",
            DocKind::Legal => "legal",
            DocKind::Report => "report",
            DocKind::Manual => "manual",
            DocKind::Minutes => "minutes",
            DocKind::Transcript => "transcript",
            DocKind::Slides => "slides",
            DocKind::Book => "book",
            DocKind::Chat => "chat",
            DocKind::Code => "code",
            DocKind::Form => "form",
            DocKind::Sheet => "sheet",
            DocKind::Other => "other",
        }
    }

    pub fn from_name(name: &str) -> Option<DocKind> {
        DocKind::ALL.into_iter().find(|k| k.name() == name)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    pub kind: DocKind,
    /// The winning score and its nearest rival, for the log.
    pub reason: String,
}

/// File name endings that are source code or configuration.
pub const CODE_EXTENSIONS: &[&str] = &[
    "rs", "py", "ts", "tsx", "js", "jsx", "go", "java", "kt", "swift", "c", "h", "cpp", "hpp", "cs", "rb", "php",
    "sh", "sql", "toml", "css", "scss",
];

/// Characters read from the start of a text, and from its middle and end each.
const SAMPLE_HEAD: usize = 30_000;
const SAMPLE_TAIL: usize = 15_000;
/// A score from here on names a kind.
const MIN_SCORE: f64 = 1.0;
/// Lead over the next kind. Below it the two read alike and the paragraph cut is the safe choice.
const MIN_LEAD: f64 = 0.25;
const FORM_MAX_CHARS: usize = 6_000;
const BOOK_MIN_CHARS: usize = 40_000;
const BOOK_MIN_PARAGRAPH: f64 = 250.0;
/// A real speaker list stays short; more names means ordinary `Label: text` lines.
const MAX_SPEAKERS: usize = 40;

macro_rules! rx {
    ($name:ident, $pattern:literal) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).unwrap());
    };
}

rx!(LEGAL, r"(?i)^(?:#{1,6}\s+|\*\*)?(?:§§?\s*\d|art\.\s*\d|artikel\s+\d|article\s+\d|artículo\s+\d|section\s+\d+[.:]|ziffer\s+\d)");
rx!(AGENDA, r"(?i)^(?:#{1,6}\s+|\*\*)?(?:top\s*\d|tagesordnungspunkt\s*\d|agenda item\s*\d|item\s+\d+[.:]|point\s+\d+[.:]|punto\s+\d+[.:])");
rx!(MINUTES_WORD, r"(?i)\b(protokoll|niederschrift|minutes of|anwesend|attendees|beschluss|procès-verbal|acta de)\b");
rx!(SPEAKER, r"^(?:\*\*)?([A-ZÄÖÜÉ][\p{L}.'\- ]{1,38})(?:\*\*)?:\s+\S");
rx!(TIMESTAMP, r"^\[?\d{1,2}:\d{2}(:\d{2})?\]?\s|-->");
rx!(CHAT, r"^\[?\d{1,4}[./-]\d{1,2}[./-]\d{1,4},?\s+\d{1,2}:\d{2}");
rx!(CHAPTER, r"(?i)^(?:#{1,6}\s+|\*\*)?(?:kapitel|chapter|chapitre|capítulo)\s+(?:\d+|[ivxlc]+|[\p{L}]+)\b");
rx!(HEADING, r"^(?:#{1,6}\s+\S|\d+(?:\.\d+)*\.?\s+\p{Lu})");
rx!(STEP, r"^(?:\s*\d{1,2}[.)]\s+\p{Lu}|(?:schritt|step|étape|paso)\s+\d)");
rx!(WARNING, r"(?i)^(?:\*\*)?(?:warnung|achtung|vorsicht|hinweis|warning|caution|note|attention|advertencia)\b");
rx!(SLIDE, r"^(?:#{1,6}\s*)?Slide \d+");
rx!(LABEL, r"^(?:\*\*)?[\p{L}][\p{L} ./-]{1,30}(?:\*\*)?:\s*\S.{0,60}$");
rx!(FORM_WORD, r"(?i)\b(rechnung|invoice|facture|factura|iban|betrag|formular|antrag|total)\b");

/// Whole text if short, else its start, middle and end: enough lines to tell kinds apart.
fn sample(text: &str) -> String {
    let n = text.len();
    if n <= SAMPLE_HEAD + 2 * SAMPLE_TAIL {
        return text.to_string();
    }
    let at = |p: usize| (p..n).find(|&i| text.is_char_boundary(i)).unwrap_or(n);
    let mid = n / 2;
    [(0, SAMPLE_HEAD), (mid, mid + SAMPLE_TAIL), (n - SAMPLE_TAIL, n)]
        .map(|(a, b)| &text[at(a)..at(b)])
        .join("\n")
}

struct Counts {
    lines: usize,
    chars: usize,
    legal: usize,
    agenda: usize,
    speaker: usize,
    speakers: usize,
    timestamp: usize,
    chat: usize,
    chapter: usize,
    heading: usize,
    step: usize,
    warning: usize,
    slide: usize,
    label: usize,
    minutes_word: bool,
    form_word: bool,
    mean_paragraph: f64,
}

fn count(sample: &str) -> Counts {
    let mut c = Counts {
        lines: 0,
        chars: sample.len(),
        legal: 0,
        agenda: 0,
        speaker: 0,
        speakers: 0,
        timestamp: 0,
        chat: 0,
        chapter: 0,
        heading: 0,
        step: 0,
        warning: 0,
        slide: 0,
        label: 0,
        minutes_word: MINUTES_WORD.is_match(sample),
        form_word: FORM_WORD.is_match(sample),
        mean_paragraph: 0.0,
    };
    let mut names = HashSet::new();
    for line in sample.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()) {
        c.lines += 1;
        let hit = |r: &Regex| usize::from(r.is_match(line));
        c.legal += hit(&LEGAL);
        c.agenda += hit(&AGENDA);
        c.timestamp += hit(&TIMESTAMP);
        c.chat += hit(&CHAT);
        c.chapter += hit(&CHAPTER);
        c.heading += hit(&HEADING);
        c.step += hit(&STEP);
        c.warning += hit(&WARNING);
        c.slide += hit(&SLIDE);
        c.label += hit(&LABEL);
        if let Some(m) = SPEAKER.captures(line) {
            c.speaker += 1;
            names.insert(m[1].trim().to_string());
        }
    }
    c.speakers = names.len();
    let paragraphs: Vec<usize> = sample.split("\n\n").map(|p| p.trim().len()).filter(|&n| n > 0).collect();
    c.mean_paragraph = paragraphs.iter().sum::<usize>() as f64 / paragraphs.len().max(1) as f64;
    c
}

fn scores(c: &Counts, total_chars: usize) -> Vec<(DocKind, f64)> {
    let lines = c.lines.max(1) as f64;
    let per_10k = |n: usize| n as f64 * 10_000.0 / c.chars.max(1) as f64;
    let share = |n: usize| n as f64 / lines;
    // Speakers come back; the labels of a form do not.
    let speakers_ok = (2..=MAX_SPEAKERS).contains(&c.speakers) && c.speaker >= 2 * c.speakers;
    let book_length = total_chars >= BOOK_MIN_CHARS && c.mean_paragraph >= BOOK_MIN_PARAGRAPH;
    vec![
        (DocKind::Chat, share(c.chat) * 3.0),
        (DocKind::Slides, if c.slide >= 2 { 1.0 + share(c.slide) * 3.0 } else { 0.0 }),
        (DocKind::Transcript, if speakers_ok { share(c.speaker) * 3.0 } else { 0.0 } + share(c.timestamp) * 2.0),
        (DocKind::Legal, if c.legal >= 3 { per_10k(c.legal).min(3.0) } else { 0.0 }),
        (DocKind::Minutes, if c.agenda >= 3 || (c.agenda >= 2 && c.minutes_word) { 1.4 } else { 0.0 }),
        (DocKind::Book, if book_length && c.chapter >= 2 { 1.6 } else if book_length && c.heading <= 2 { 1.1 } else { 0.0 }),
        (DocKind::Manual, if c.step >= 5 && (c.warning >= 1 || per_10k(c.step) >= 5.0) { 1.3 } else { 0.0 }),
        (DocKind::Form, if total_chars <= FORM_MAX_CHARS && c.form_word && share(c.label) >= 0.3 { 1.2 } else { 0.0 }),
        (DocKind::Report, if c.heading >= 3 { 1.0 } else { 0.0 }),
    ]
}

/// The kind of a text. `file` is the name incl. folders; its ending alone settles source code.
pub fn detect_kind(text: &str, file: &str) -> Detection {
    let ext = file.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if CODE_EXTENSIONS.contains(&ext.as_str()) {
        return Detection { kind: DocKind::Code, reason: format!("file ending .{ext}") };
    }
    let mut ranked = scores(&count(&sample(text)), text.len());
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let (best, score) = ranked[0];
    let (rival, next) = ranked[1];
    let reason = format!("{} {score:.2}, then {} {next:.2}", best.name(), rival.name());
    let kind = if score >= MIN_SCORE && score - next >= MIN_LEAD { best } else { DocKind::Other };
    Detection { kind, reason }
}

/// A result worth keeping: it holds the text and is neither one huge block nor crumbs.
pub fn is_plausible(text: &str, chunks: &[NewChunk], max_chars: usize) -> bool {
    const CRUMB_CHARS: usize = 60;
    const MIN_CRUMB_CHECK: usize = 20;
    const MIN_COVERED: f64 = 0.9;
    if chunks.is_empty() {
        return text.trim().is_empty();
    }
    let lengths: Vec<usize> = chunks.iter().map(|c| c.text.chars().count()).collect();
    let covered = lengths.iter().sum::<usize>() as f64;
    let total = text.chars().filter(|c| !c.is_whitespace()).count() as f64;
    let one_block = chunks.len() == 1 && lengths[0] > max_chars;
    let mut sorted = lengths;
    sorted.sort_unstable();
    let crumbs = sorted.len() >= MIN_CRUMB_CHECK && sorted[sorted.len() / 2] < CRUMB_CHARS;
    !one_block && !crumbs && covered >= total * MIN_COVERED
}
