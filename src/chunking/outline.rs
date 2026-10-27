//! Outline of a document read back from its stored chunks (LLM-free, no re-chunking): which
//! headings a passage sits under ("KAPITEL III › ABSCHNITT 3 › Artikel 18 – Aufbewahrung der
//! Dokumentation › (1)") and which chunks make up its whole article or section.
//!
//! Input are the chunks of ONE document in reading order (`chunk_index`); the outline is rebuilt
//! from their lines each time (a few ms for 1,400 chunks), so it works for every existing
//! collection without a new field. **Precision over recall:** a line only counts as a heading when
//! it stands alone and matches a closed form; where nothing matches, a chunk has no path (the
//! caller leaves it out rather than inventing one). Recognised:
//! - Markdown ATX headings (`#`…`######`) in chunks of the Markdown chunker (not in the
//!   synthetic "Text on page N (OCR)" sections; the caller sets `markdown`).
//! - Legal acts (a document with at least [`MIN_ARTICLES`] article lines): part / title /
//!   chapter / section / subsection / annex lines (DE/EN/FR/ES/IT/NL words + Roman, Arabic or one
//!   capital letter), article lines (`Artikel 18`, `Article 18`, `Artículo 18`, `Art. 18`, `§ 18`)
//!   with the next line as title, and inside an article its paragraphs `(1)` or `1.` — strictly
//!   numbered 1, 2, 3 …, one style per article, so dates and list items do not count.
//! - Other documents: numbered sections (`2.1 Title`, `3.1. Title`, `5. Title`) that continue the
//!   numbering seen so far (a table of contents is skipped by its trailing page numbers), control
//!   lines of control catalogues (`AC-7 UNSUCCESSFUL LOGON ATTEMPTS`) and their numbered
//!   enhancements `(1)`.
//!
//! A section ("unit") runs from its heading to the next heading of the same or a higher rank;
//! paragraphs are never units. A chunk belongs to the unit of its first non-heading line; the unit's
//! chunks are all chunks with a line inside that span (a chunk ending in the next article's heading
//! does not pull that article in). Labels keep the document's own wording.

use regex::Regex;
use std::sync::LazyLock;

/// Separator of the path (the same as the Markdown chunker's `heading_path`).
pub const PATH_SEP: &str = " › ";
/// Article lines a document needs before it is read as a legal act.
pub const MIN_ARTICLES: usize = 3;
const TITLE_MAX_CHARS: usize = 80;

/// One chunk of the document, in reading order.
pub struct OutlineChunk<'a> {
    pub id: i64,
    pub text: &'a str,
    /// The chunk comes from the Markdown chunker (ATX headings are real headings).
    pub markdown: bool,
}

/// The chunks of one section, for "fetch the whole article".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineSection {
    /// Path of the section itself (without a paragraph).
    pub path: String,
    /// Its chunks in reading order.
    pub chunk_ids: Vec<i64>,
}

static GROUP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(TEIL|PART|PARTIE|PARTE|TITEL|TITLE|TITRE|T[IÍ]TULO|TITOLO|KAPITEL|CHAPTER|CHAPITRE|CAP[IÍ]TULO|CAPO|HOOFDSTUK|ABSCHNITT|SECTION|SECCI[OÓ]N|SEZIONE|AFDELING|UNTERABSCHNITT|SUBSECTION|SOUS-SECTION|SUBSECCI[OÓ]N|ANHANG|ANNEX|ANNEXE|ANEXO|ALLEGATO|BIJLAGE)\s+([IVXLC]+|\d{1,3}[a-z]?|[A-Z])$").unwrap()
});
static ARTICLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(Artikel|Article|Art[ií]culo|Articolo|Art\.|§)\s*\d{1,3}[a-z]?$").unwrap());
static PARA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\((\d{1,3})\)|(\d{1,3})\.(?:\s+|[^\d\s.]))").unwrap());
static NUMBERED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{1,2}(?:\.\d{1,2})*)(\.?)\s+(\S.*)$").unwrap());
static CONTROL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z]{2,3}-\d{1,2}\s+[A-Z][A-Z0-9 ,'’/()&—–-]{2,90}$").unwrap());
static ENHANCEMENT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\((\d{1,2})\)\s*[A-Z]").unwrap());
static TRAILING_PAGE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s\d{1,4}$").unwrap());

/// Rank of a heading: lower is higher. Paragraphs are not headings.
fn group_rank(word: &str) -> u8 {
    match word.to_uppercase().as_str() {
        "TEIL" | "PART" | "PARTIE" | "PARTE" | "ANHANG" | "ANNEX" | "ANNEXE" | "ANEXO" | "ALLEGATO" | "BIJLAGE" => 1,
        "TITEL" | "TITLE" | "TITRE" | "TITULO" | "TÍTULO" | "TITOLO" => 2,
        "KAPITEL" | "CHAPTER" | "CHAPITRE" | "CAPITULO" | "CAPÍTULO" | "CAPO" | "HOOFDSTUK" => 3,
        "UNTERABSCHNITT" | "SUBSECTION" | "SOUS-SECTION" | "SUBSECCION" | "SUBSECCIÓN" => 5,
        _ => 4, // ABSCHNITT, SECTION, …
    }
}
const ARTICLE_RANK: u8 = 6;
const CONTROL_RANK: u8 = 9;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Carries its title on the next line (groups, annexes, articles).
    TitledNext,
    /// Title is part of the heading line (Markdown, numbered sections, controls).
    Inline,
}

struct Heading {
    rank: u8,
    label: String,
    title: Option<String>,
    parent: Option<usize>,
    /// Line position of the heading; the span is `(start, end)`.
    start: usize,
    end: usize,
    /// Articles and controls number their paragraphs / enhancements.
    numbers_paragraphs: bool,
}

#[derive(Clone)]
struct ChunkPlace {
    id: i64,
    /// First and last non-heading line (titles included): the chunk's span.
    first_pos: Option<usize>,
    last_pos: Option<usize>,
    /// Innermost heading at the first line that is no title; failing that, at the first title.
    unit: Option<Option<usize>>,
    title_unit: Option<Option<usize>>,
    para_first: Option<String>,
    para_last: Option<String>,
    /// Innermost heading after the chunk (for a chunk of headings only).
    after: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Line {
    Title,
    Paragraph,
    Text,
}

/// Paragraph numbering inside the current article/control.
#[derive(Default)]
struct Paras {
    /// `true` = "(1)", `false` = "1."; set by the first paragraph.
    parens: Option<bool>,
    last: u32,
    current: Option<String>,
}

pub struct Outline {
    headings: Vec<Heading>,
    chunks: Vec<ChunkPlace>,
}

impl Outline {
    pub fn build(chunks: &[OutlineChunk]) -> Outline {
        let legal = chunks
            .iter()
            .flat_map(|c| c.text.lines())
            .filter(|l| ARTICLE_RE.is_match(l.trim()))
            .take(MIN_ARTICLES)
            .count()
            >= MIN_ARTICLES;
        let mut b = Builder { legal, headings: Vec::new(), stack: Vec::new(), numbers: Vec::new(), paras: Paras::default(), expect_title: None };
        let mut places = Vec::with_capacity(chunks.len());
        let mut pos = 0usize;
        for c in chunks {
            let mut place = ChunkPlace { id: c.id, first_pos: None, last_pos: None, unit: None, title_unit: None, para_first: None, para_last: None, after: None };
            for raw in c.text.lines() {
                pos += 1;
                let line = raw.trim();
                if line.is_empty() || b.heading(line, c.markdown, pos) {
                    continue;
                }
                let kind = b.content(line);
                let inner = b.stack.last().copied();
                place.first_pos.get_or_insert(pos);
                place.last_pos = Some(pos);
                if kind == Line::Title {
                    place.title_unit.get_or_insert(inner);
                    continue;
                }
                if *place.unit.get_or_insert(inner) != inner {
                    continue;
                }
                // Short lines and layout lines (page headers with wide gaps) before the first
                // paragraph of the chunk say nothing about which paragraph it starts in.
                let counts = kind == Line::Paragraph || (line.chars().count() >= 30 && !line.contains("    "));
                if counts {
                    if place.para_first.is_none() {
                        place.para_first = b.paras.current.clone();
                    }
                    if b.paras.current.is_some() {
                        place.para_last = b.paras.current.clone();
                    }
                }
            }
            place.after = b.stack.last().copied();
            places.push(place);
        }
        for &h in &b.stack {
            b.headings[h].end = pos + 1;
        }
        Outline { headings: b.headings, chunks: places }
    }

    /// Whether any heading was recognised in the document (else every `path`/`section` is `None`).
    pub fn has_headings(&self) -> bool {
        !self.headings.is_empty()
    }

    /// Path of a chunk ("KAPITEL III › Artikel 18 – Title › (1)–(3)"); `None` when no heading is
    /// known above it.
    pub fn path(&self, chunk_id: i64) -> Option<String> {
        let place = self.chunks.iter().find(|c| c.id == chunk_id)?;
        let unit = self.unit_of(place)?;
        let mut path = self.heading_path(unit);
        match (&place.para_first, &place.para_last) {
            (Some(a), Some(z)) if a != z => path.push_str(&format!("{PATH_SEP}{a}–{z}")),
            (Some(a), _) => path.push_str(&format!("{PATH_SEP}{a}")),
            _ => {}
        }
        Some(path)
    }

    /// The section (article, control, numbered section, Markdown section) the chunk belongs to.
    pub fn section(&self, chunk_id: i64) -> Option<OutlineSection> {
        let place = self.chunks.iter().find(|c| c.id == chunk_id)?;
        let unit = self.unit_of(place)?;
        let (start, end) = (self.headings[unit].start, self.headings[unit].end);
        let chunk_ids = self
            .chunks
            .iter()
            .filter(|c| match (c.first_pos, c.last_pos) {
                (Some(first), Some(last)) => first < end && last > start,
                _ => false,
            })
            .map(|c| c.id)
            .collect();
        Some(OutlineSection { path: self.heading_path(unit), chunk_ids })
    }

    fn unit_of(&self, place: &ChunkPlace) -> Option<usize> {
        place.unit.or(place.title_unit).unwrap_or(place.after)
    }

    fn heading_path(&self, unit: usize) -> String {
        let mut chain = vec![unit];
        while let Some(p) = self.headings[*chain.last().unwrap()].parent {
            chain.push(p);
        }
        let parts: Vec<String> = chain
            .iter()
            .rev()
            .map(|&h| {
                let head = &self.headings[h];
                match (&head.title, h == unit) {
                    (Some(t), true) => format!("{} – {t}", head.label),
                    _ => head.label.clone(),
                }
            })
            .collect();
        parts.join(PATH_SEP)
    }
}

struct Builder {
    legal: bool,
    headings: Vec<Heading>,
    stack: Vec<usize>,
    /// Numbering of the last numbered section ([2, 1] for "2.1").
    numbers: Vec<u32>,
    paras: Paras,
    expect_title: Option<usize>,
}

impl Builder {
    /// Opens a heading when `line` is one; `true` = the line is a heading.
    fn heading(&mut self, line: &str, markdown: bool, pos: usize) -> bool {
        let found = if markdown { markdown_heading(line) } else { None }
            .or_else(|| self.legal_heading(line))
            .or_else(|| if self.legal { None } else { self.numbered_heading(line) });
        let Some((rank, label, kind, numbers_paragraphs)) = found else {
            return false;
        };
        while let Some(&top) = self.stack.last() {
            if self.headings[top].rank < rank {
                break;
            }
            self.headings[top].end = pos;
            self.stack.pop();
        }
        let idx = self.headings.len();
        let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
        self.headings.push(Heading { rank, label, title: None, parent: self.stack.last().copied(), start: pos, end: pos, numbers_paragraphs });
        self.stack.push(idx);
        self.paras = Paras::default();
        self.expect_title = (kind == Kind::TitledNext).then_some(idx);
        true
    }

    fn legal_heading(&self, line: &str) -> Option<(u8, String, Kind, bool)> {
        if let Some(c) = GROUP_RE.captures(line) {
            return Some((group_rank(&c[1]), line.to_string(), Kind::TitledNext, false));
        }
        (self.legal && ARTICLE_RE.is_match(line)).then(|| (ARTICLE_RANK, line.to_string(), Kind::TitledNext, true))
    }

    fn numbered_heading(&mut self, line: &str) -> Option<(u8, String, Kind, bool)> {
        if CONTROL_RE.is_match(line) {
            return Some((CONTROL_RANK, line.to_string(), Kind::Inline, true));
        }
        let c = NUMBERED_RE.captures(line)?;
        let numbers: Vec<u32> = c[1].split('.').filter_map(|n| n.parse().ok()).collect();
        let title = &c[3];
        let single = numbers.len() == 1;
        let in_control = self.stack.iter().any(|&h| self.headings[h].rank == CONTROL_RANK);
        if (single && (c[2].is_empty() || in_control))
            || line.chars().count() > 100
            || !title.chars().next().is_some_and(char::is_uppercase)
            || title.ends_with(['.', ',', ';', ':'])
            || title.contains('[')
            || title.contains(". ")
            || title.split_whitespace().count() > 12
            || TRAILING_PAGE_RE.is_match(line)
            || !continues(&self.numbers, &numbers)
        {
            return None;
        }
        let rank = numbers.len().min(8) as u8;
        self.numbers = numbers;
        Some((rank, line.to_string(), Kind::Inline, false))
    }

    /// A non-heading line: a title, a paragraph start or plain text.
    fn content(&mut self, line: &str) -> Line {
        if let Some(h) = self.expect_title.take()
            && is_title(line)
        {
            self.headings[h].title = Some(line.chars().take(TITLE_MAX_CHARS).collect());
            return Line::Title;
        }
        let Some(&inner) = self.stack.last() else { return Line::Text };
        if !self.headings[inner].numbers_paragraphs {
            return Line::Text;
        }
        let in_control = self.headings[inner].rank == CONTROL_RANK;
        let (n, parens) = if in_control {
            match ENHANCEMENT_RE.captures(line) {
                Some(c) => (c[1].parse::<u32>().ok(), true),
                None => return Line::Text,
            }
        } else {
            match PARA_RE.captures(line) {
                Some(c) => match (c.get(1), c.get(2)) {
                    (Some(m), _) => (m.as_str().parse().ok(), true),
                    (_, Some(m)) => (m.as_str().parse().ok(), false),
                    _ => return Line::Text,
                },
                None => return Line::Text,
            }
        };
        let Some(n) = n else { return Line::Text };
        if n != self.paras.last + 1 || self.paras.parens.is_some_and(|p| p != parens) {
            return Line::Text;
        }
        self.paras.parens = Some(parens);
        self.paras.last = n;
        self.paras.current = Some(if parens { format!("({n})") } else { format!("{n}.") });
        Line::Paragraph
    }
}

fn markdown_heading(line: &str) -> Option<(u8, String, Kind, bool)> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &line[hashes..];
    if !rest.starts_with(' ') {
        return None;
    }
    let title = rest.trim().trim_end_matches('#').trim();
    (!title.is_empty()).then(|| (hashes as u8, title.to_string(), Kind::Inline, false))
}

/// Whether `next` continues the section numbering `prev`: at the first component where they
/// differ, `next` is one higher and every later component is 1 ("2.4" → "3.1", "2.1" → "2.1.1").
/// The first numbered heading may be any "N" / "N.1" / "N.1.1".
fn continues(prev: &[u32], next: &[u32]) -> bool {
    if prev.is_empty() {
        return next[1..].iter().all(|&n| n == 1);
    }
    let Some(i) = (0..next.len()).find(|&i| prev.get(i) != Some(&next[i])) else {
        return false; // same number again (or a prefix of the current one)
    };
    next[i] == prev.get(i).copied().unwrap_or(0) + 1 && next[i + 1..].iter().all(|&n| n == 1)
}

/// The line after an article/group heading is its title when it reads like one.
fn is_title(line: &str) -> bool {
    let n = line.chars().count();
    (4..=160).contains(&n)
        && line.chars().next().is_some_and(char::is_uppercase)
        && !line.ends_with(['.', ',', ';', ':'])
        && !ARTICLE_RE.is_match(line)
        && !GROUP_RE.is_match(line)
}
