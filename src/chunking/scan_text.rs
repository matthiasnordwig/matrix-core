//! A run of scanned PDF pages as one continuous text (pure rules, no OCR engine here).
//!
//! The caller (services `ingest/ocr/`) recognises each scanned page as rows with their boxes
//! ([`ScanRow`], TOP-left coordinates; the engines' bottom-left boxes are converted there) and
//! hands them over page by page:
//! 1. [`page_lines`] / [`lines_from_rows`]: a paragraph starts after an empty band of more than
//!    [`PARAGRAPH_GAP_LINES`] line heights or at an indented line; [`is_heading`] marks a row
//!    much taller than the body text with space above it, an outline number ([`outline_start`])
//!    opening a short line (with a full stop only when centred), or a short line without a
//!    sentence end with gaps above and below.
//! 2. [`clean_pages`]: lone page numbers ([`is_page_number`]) and running heads/feet (the same
//!    line, digits ignored, at a page edge on at least a quarter of the pages, at least 3, and
//!    misread copies of it, [`misread_of`]) go; recognition noise in place of punctuation is
//!    replaced (`scan_noise.rs`).
//! 3. [`assemble`]: lines of a paragraph are joined ([`join_kind`]: end-of-line hyphen removed
//!    or kept; Chinese/Japanese lines meet without a space, [`joins_directly`]), a paragraph
//!    runs on over a page break unless the next page starts one; headings become `## …` lines, paragraphs are separated by a blank line. The stored text has a
//!    single `\n` only where a page break falls inside a paragraph, so every page start
//!    (`Assembled::page_starts`) is a line start — the offsets [`ChunkPages`] needs.
//!    [`chunk_view`] turns those single newlines into spaces (same byte length): the text the
//!    chunker reads, so its byte offsets index the stored text unchanged.
//! 4. [`pages_of_range`]: the pages of one chunk's byte range, for `page`/`page_breaks`.
//!
//! [`continues_run`] decides whether a scanned page may continue the run of the page before
//! (no text-layer rest beyond a page number, no column rows ` | ` on either page);
//! [`scan_marker`]/[`is_scan_marker`] are the line that opens a run's block in the document
//! text (`## Text on pages 5–9 (OCR)`, one page: `## Text on page 5 (OCR)`). Chunks cut this
//! way carry `scan_rev` = [`SCAN_REV`]. General rules only, nothing per kind of document;
//! measured in services `examples/scan_rechunk.rs` (`handbuch/core.md` „Scans als durchgehender
//! Text“). Tests: `scan_text_tests.rs`.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::pages::ChunkPages;
use crate::token_estimate::is_cjk;

/// Revision of the scan text rules, stamped as `metadata.scan_rev` on chunks cut from an
/// assembled scan run (absent = the old page-by-page OCR blocks).
pub const SCAN_REV: i64 = 2;

/// Empty space between two rows, in line heights, above which a new paragraph starts.
pub const PARAGRAPH_GAP_LINES: f64 = 1.0;

/// One text row of a page in reading order with its box, in TOP-left coordinates (0..1 of the
/// image; a page set in vertical columns arrives turned into its reading frame by `ingest/ocr`:
/// each column is a row, the rightmost on top): `top`/`bottom` from the image top, `left`/`right` from its left edge, `height` the
/// tallest box of the row. Columns of one row are joined with ` | ` in `text`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanRow {
    pub text: String,
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    pub height: f64,
}

/// One line of a page after recognition: its text and what the layout says about it.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLine {
    pub text: String,
    /// A paragraph gap above it on the same page.
    pub gap: bool,
    /// Indented against the page's full lines.
    pub indent: bool,
    pub heading: bool,
}

impl PageLine {
    pub fn para_start(&self) -> bool {
        self.gap || self.indent || self.heading
    }
}

// --- Rows → lines: paragraphs and headings from the boxes ---------------------------------

/// A row this much taller than the body text is a heading.
const HEADING_HEIGHT: f64 = 1.3;
/// Indentation (in body line heights) that starts a paragraph.
const INDENT_LINES: f64 = 0.5;
/// Short-line heading: at most this many characters and this share of the full line width.
const SHORT_CHARS: usize = 60;
const SHORT_WIDTH: f64 = 0.6;

fn median(mut xs: Vec<f64>) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.sort_by(f64::total_cmp);
    xs[xs.len() / 2]
}

static OUTLINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:§+\s*\d+[a-z]?\.?|(?:Art\.|Artikel|Kapitel|Abschnitt|Teil)\s*[\dIVXLC]+\.?|[IVXLC]{1,6}\.|\d{1,2}(?:\.\d{1,2})+\.?|\d{1,2}\.)(?:\s|$)").unwrap()
});

/// An outline number at the line start: "§ 5", "§ 5.", "Art. 3", "Artikel 12", "Kapitel 2", "II.", "3.2", "4.".
pub fn outline_start(t: &str) -> bool {
    OUTLINE_RE.is_match(t)
}

/// Whether row `r` stands centred between the edges of the page's full lines (`rows` wider
/// than 0.8 × `full_w`): space of more than two body line heights on both sides, the smaller at
/// least half the larger (scans sit askew on the page). A paragraph's indented first line or
/// its short last line has space on one side only.
fn is_centred(rows: &[ScanRow], r: &ScanRow, body_h: f64, full_w: f64) -> bool {
    let full: Vec<&ScanRow> = rows.iter().filter(|x| x.right - x.left >= 0.8 * full_w).collect();
    let (left, right) = (median(full.iter().map(|x| x.left).collect()), median(full.iter().map(|x| x.right).collect()));
    let (l, rt) = (r.left - left, right - r.right);
    !full.is_empty() && l.min(rt) > 2.0 * body_h && 2.0 * l.min(rt) >= l.max(rt)
}

fn ends_sentence(t: &str) -> bool {
    t.trim_end().ends_with(['.', ',', ';', ':', '!', '?', '«', '»', '"', '“', '”'])
}

/// An empty band of more than [`PARAGRAPH_GAP_LINES`] line heights between rows `a` and `b`.
fn gap(a: &ScanRow, b: &ScanRow) -> bool {
    b.top - a.bottom > PARAGRAPH_GAP_LINES * (a.height + b.height) / 2.0
}

/// Whether row `i` of a page is a heading: much taller than the body (`body_h`) with a gap
/// above; an outline number opening a short line with a gap above or standing centred
/// ([`is_centred`]), without a sentence end — or with a full stop when centred ("§ 5. Titel.";
/// a numbered short sentence starts at the margin or the indent); or a short line without a
/// sentence end with a paragraph gap above and below. `full_w` = width of a full line.
pub fn is_heading(rows: &[ScanRow], i: usize, body_h: f64, full_w: f64) -> bool {
    let r = &rows[i];
    let t = r.text.trim();
    let chars = t.chars().count();
    let above = i == 0 || gap(&rows[i - 1], r);
    // Box heights of body lines scatter up to ~1.5×: a tall row counts only with space above it.
    if r.height >= HEADING_HEIGHT * body_h && above && chars <= 120 {
        return true;
    }
    let below = rows.get(i + 1).is_none_or(|n| gap(r, n));
    let bare_number = outline_start(t) && t.split_whitespace().count() == 1;
    let centred = outline_start(t) && is_centred(rows, r, body_h, full_w);
    if outline_start(t) && chars <= 80 && (above || centred) && (!ends_sentence(t) || bare_number || (centred && t.ends_with('.'))) {
        return true;
    }
    i > 0 && rows.get(i + 1).is_some() && above && below && chars <= SHORT_CHARS && r.right - r.left <= SHORT_WIDTH * full_w && !ends_sentence(t)
}

/// The lines of one page from its rows; `body_h` = median row height of the document.
pub fn lines_from_rows(rows: &[ScanRow], body_h: f64) -> Vec<PageLine> {
    let full_w = rows.iter().map(|r| r.right - r.left).fold(0.0, f64::max);
    let rtl = is_rtl(rows);
    let margin = median(rows.iter().filter(|r| r.right - r.left >= 0.8 * full_w).map(|r| start_edge(r, rtl)).collect());
    rows.iter()
        .enumerate()
        .map(|(i, r)| PageLine {
            text: r.text.trim().to_string(),
            gap: i > 0 && gap(&rows[i - 1], r),
            indent: start_edge(r, rtl) - margin > INDENT_LINES * body_h,
            heading: is_heading(rows, i, body_h, full_w),
        })
        .collect()
}

/// Whether the rows are written right to left (see [`is_rtl_text`]).
fn is_rtl(rows: &[ScanRow]) -> bool {
    is_rtl_text(rows.iter().map(|r| r.text.as_str()))
}

/// Whether `texts` are written right to left: more than half of their letters are Hebrew,
/// Arabic or related (no shared script helper exists; the ranges are those of
/// `token_estimate.rs` plus the presentation forms). Also decides the box order inside a row
/// while the rows are built (`ingest/ocr`).
pub fn is_rtl_text<'a>(texts: impl IntoIterator<Item = &'a str>) -> bool {
    let (mut rtl, mut all) = (0usize, 0usize);
    for c in texts.into_iter().flat_map(str::chars).filter(|c| c.is_alphabetic()) {
        all += 1;
        rtl += usize::from(matches!(c as u32, 0x590..=0x8FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF));
    }
    2 * rtl > all
}

/// Distance of a row's reading-start edge from the image side it starts at: the left edge, or
/// the right edge measured from the right for right-to-left text.
fn start_edge(r: &ScanRow, rtl: bool) -> f64 {
    if rtl { 1.0 - r.right } else { r.left }
}

/// A picture counts as a block of running text (letter, book page) only from this many rows on:
/// fewer rows are a sign, a caption or scattered labels, where headings, indents and gaps read
/// from the geometry would be guesses.
pub const TEXT_BLOCK_MIN_ROWS: usize = 8;
/// Start edges within this many row heights count as the same edge (indented first lines of
/// paragraphs stay within it).
const LEFT_EDGE_TOLERANCE: f64 = 1.0;

/// Whether `rows` of one picture form a block of running text: at least [`TEXT_BLOCK_MIN_ROWS`]
/// rows, and more than half of them share one start edge (left; right for right-to-left text). A diagram with labels, a screenshot or a
/// sign fails this, so those keep their rows as they are.
pub fn is_text_block(rows: &[ScanRow]) -> bool {
    if rows.len() < TEXT_BLOCK_MIN_ROWS {
        return false;
    }
    let tol = LEFT_EDGE_TOLERANCE * median(rows.iter().map(|r| r.height).collect());
    let rtl = is_rtl(rows);
    rows.iter().any(|a| 2 * rows.iter().filter(|b| (start_edge(b, rtl) - start_edge(a, rtl)).abs() <= tol).count() > rows.len())
}

/// The lines of every page `(1-based page, rows)`, body height taken over all of them.
pub fn page_lines(pages: &[(i64, Vec<ScanRow>)]) -> Vec<(i64, Vec<PageLine>)> {
    let body_h = median(pages.iter().flat_map(|(_, r)| r.iter().map(|x| x.height)).collect());
    pages.iter().map(|(p, r)| (*p, lines_from_rows(r, body_h))).collect()
}

// --- Runs of scanned pages ----------------------------------------------------------------

/// Two neighbouring rows with a wide horizontal gap (columns joined with ` | `): a table. A single
/// such row (a footer "Titel | 2", a numbered formula) stays running text.
fn has_columns(rows: &[ScanRow]) -> bool {
    rows.windows(2).any(|w| w[0].text.contains(" | ") && w[1].text.contains(" | "))
}

/// Whether a scanned page (`rows`, and `layer_rest` = what its text layer holds besides the
/// recognised text) may continue the run of the scanned page before it (`prev_rows`): the
/// text layer holds at most a page number, and neither page has column rows (two neighbouring ` | ` rows).
pub fn continues_run(prev_rows: &[ScanRow], layer_rest: &str, rows: &[ScanRow]) -> bool {
    let rest = layer_rest.trim();
    (rest.is_empty() || is_page_number(rest)) && !has_columns(prev_rows) && !has_columns(rows)
}

const MARKER_PREFIX: &str = "## Text on page";
const MARKER_SUFFIX: &str = " (OCR)";

/// The line that opens the recognised text of the scanned pages `first..=last` (1-based).
pub fn scan_marker(first: i64, last: i64) -> String {
    if first == last {
        format!("{MARKER_PREFIX} {first}{MARKER_SUFFIX}")
    } else {
        format!("{MARKER_PREFIX}s {first}\u{2013}{last}{MARKER_SUFFIX}")
    }
}

static MARKER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^## Text on (?:page (\d+)|pages (\d+)[–-](\d+)) \(OCR\)$").unwrap());

/// The pages `(first, last)` a marker line names (old form `page N` and run form `pages A–B`).
pub fn parse_scan_marker(line: &str) -> Option<(i64, i64)> {
    let c = MARKER_RE.captures(line.trim())?;
    let n = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<i64>().ok());
    match n(1) {
        Some(p) => Some((p, p)),
        None => Some((n(2)?, n(3)?)),
    }
}

/// Whether `line` is the line opening a block of recognised scan text.
pub fn is_scan_marker(line: &str) -> bool {
    parse_scan_marker(line).is_some()
}

// --- Cleaning: running heads/feet and page numbers ----------------------------------------

/// Lines looked at on each end of a page.
const EDGE_LINES: usize = 2;

static PAGE_NUMBER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\d{1,4}|x{0,3}(?:ix|iv|v?i{0,3})|X{0,3}(?:IX|IV|V?I{0,3}))$").unwrap());
static DIGITS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());

/// Decoration around a page number: dashes, dots, brackets, and what recognition makes of a dash
/// (the Japanese long vowel mark "ー", the CJK "一", full-width "－"/"～", "_").
const PAGE_NUMBER_DECORATION: &str = "-–—‐‑‒―−.·•()[]*_~ー一－～";

/// A line that is only a page number: up to four digits or a small Roman numeral, decorated at
/// most by [`PAGE_NUMBER_DECORATION`] ("— 16 —", "[3]", "xii", "ー8ー").
pub fn is_page_number(t: &str) -> bool {
    let core = t.trim_matches(|c: char| c.is_whitespace() || PAGE_NUMBER_DECORATION.contains(c));
    t.chars().count() <= 12 && !core.is_empty() && PAGE_NUMBER_RE.is_match(core)
}

/// Key of a running line: lower case, digit runs as `#`, whitespace collapsed.
fn running_key(t: &str) -> String {
    DIGITS_RE.replace_all(&t.to_lowercase(), "#").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A running line's key must be this long (without spaces) before misread copies of it count.
const FUZZY_MIN_CHARS: usize = 8;

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push((prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Whether the edge line with key `k` is a misread copy of the running line with key `run`
/// ("Digilized by Google", "Digitiz ed by" for "Digitized by Google"): spaces ignored, `run` at
/// least [`FUZZY_MIN_CHARS`] long, and `k` within a quarter of its length in edit distance or a
/// piece of it of at least half its length.
fn misread_of(k: &str, run: &str) -> bool {
    let (k, run): (Vec<char>, Vec<char>) = (k.chars().filter(|c| !c.is_whitespace()).collect(), run.chars().filter(|c| !c.is_whitespace()).collect());
    if run.len() < FUZZY_MIN_CHARS || k.is_empty() {
        return false;
    }
    let piece = 2 * k.len() >= run.len() && run.windows(k.len()).any(|w| w == k.as_slice());
    piece || (k.len().abs_diff(run.len()) <= run.len() / 4 && levenshtein(&k, &run) <= run.len() / 4)
}

/// What [`clean_pages`] removed.
#[derive(Debug, Default, PartialEq)]
pub struct Removed {
    pub page_numbers: usize,
    pub running_lines: usize,
    /// Up to five of the running lines, as they stood.
    pub running_examples: Vec<String>,
    /// Runs of recognition noise replaced (`scan_noise::repair_noise`).
    pub noise_runs: usize,
}

/// Indices of the edge lines of a page (first and last [`EDGE_LINES`]).
fn edges(n: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..n.min(EDGE_LINES)).chain(n.saturating_sub(EDGE_LINES)..n).collect();
    idx.dedup();
    idx
}

/// Keep the lines of `page` not in `drop`; nothing stands above the first one any more, so a
/// gap there was the one under a dropped head.
fn drop_lines(page: &mut Vec<PageLine>, drop: &HashSet<usize>) {
    *page = page.iter().enumerate().filter(|(i, _)| !drop.contains(i)).map(|(_, l)| l.clone()).collect();
    if let Some(first) = page.first_mut() {
        first.gap = false;
    }
}

/// Drop lone page numbers and running lines (the same key at a page edge on at least a quarter
/// of the pages, at least 3, and misread copies of it, [`misread_of`]) from the edges of every
/// page, then replace recognition noise in place of punctuation (`scan_noise.rs`).
pub fn clean_pages(pages: &mut [(i64, Vec<PageLine>)]) -> Removed {
    let mut removed = Removed::default();
    for (_, page) in pages.iter_mut() {
        let drop: HashSet<usize> = edges(page.len()).into_iter().filter(|&i| is_page_number(&page[i].text)).collect();
        removed.page_numbers += drop.len();
        drop_lines(page, &drop);
    }
    let mut pages_of: HashMap<String, HashSet<usize>> = HashMap::new();
    for (p, (_, page)) in pages.iter().enumerate() {
        for i in edges(page.len()) {
            pages_of.entry(running_key(&page[i].text)).or_default().insert(p);
        }
    }
    let min = (pages.len() / 4).max(3);
    let running: HashSet<String> = pages_of.into_iter().filter(|(k, p)| p.len() >= min && !k.is_empty()).map(|(k, _)| k).collect();
    for (_, page) in pages.iter_mut() {
        let is_running = |k: &str| running.contains(k) || running.iter().any(|r| misread_of(k, r));
        let drop: HashSet<usize> = edges(page.len()).into_iter().filter(|&i| is_running(&running_key(&page[i].text))).collect();
        for &i in &drop {
            if removed.running_examples.len() < 5 && !removed.running_examples.contains(&page[i].text) {
                removed.running_examples.push(page[i].text.clone());
            }
        }
        removed.running_lines += drop.len();
        drop_lines(page, &drop);
    }
    removed.noise_runs = super::scan_noise::repair_noise(pages);
    removed
}

// --- Joining lines and pages --------------------------------------------------------------

/// Words after which a trailing hyphen is a cut-off compound ("Ein- und Ausgang"), not a break.
const KEEP_HYPHEN_BEFORE: [&str; 6] = ["und", "oder", "bis", "sowie", "als", "wie"];

#[derive(Debug, PartialEq, Eq)]
pub enum Join {
    /// Hyphen removed, word glued ("Kündi-" + "gung").
    Dehyphen,
    /// Hyphen kept, glued ("Nord-" + "Amerika").
    Glue,
    /// Chinese/Japanese on both sides: no space (these scripts have none between words).
    Direct,
    Space,
}

/// Whether text ending with `prev` and text starting with `next` meet without a space: both
/// sides are Chinese/Japanese characters ([`is_cjk`]). Also used between the boxes of one row
/// (`ingest/ocr`).
pub fn joins_directly(prev: &str, next: &str) -> bool {
    prev.trim_end().chars().next_back().is_some_and(is_cjk) && next.trim_start().chars().next().is_some_and(is_cjk)
}

/// How `next` continues a paragraph that ends with `prev`.
pub fn join_kind(prev: &str, next: &str) -> Join {
    if joins_directly(prev, next) {
        return Join::Direct;
    }
    let mut tail = prev.chars().rev();
    // '=' and '⸗': the double hyphen of older print.
    let (Some('-' | '¬' | '=' | '⸗'), Some(before)) = (tail.next(), tail.next()) else { return Join::Space };
    if !before.is_alphabetic() {
        return Join::Space;
    }
    let word: String = next.chars().take_while(|c| c.is_alphabetic()).collect();
    match word.chars().next() {
        Some(c) if c.is_lowercase() && !KEEP_HYPHEN_BEFORE.contains(&word.as_str()) => Join::Dehyphen,
        Some(c) if c.is_uppercase() => Join::Glue,
        _ => Join::Space,
    }
}

/// The pages as one text: `stored` (a page break inside a paragraph is a `\n`, everything else
/// a space or a blank line) and where each page's text starts in it, with counts of what the
/// joining did.
#[derive(Debug, Default)]
pub struct Assembled {
    pub stored: String,
    /// (byte offset, 1-based page), ascending; every offset is a line start of `stored`.
    pub page_starts: Vec<(usize, i64)>,
    pub dehyphen: usize,
    pub dehyphen_over_page: usize,
    pub glued: usize,
    pub paragraphs: usize,
    pub headings: usize,
    /// Page breaks inside a paragraph (the page's text continues the previous one).
    pub breaks_in_paragraph: usize,
}

/// Where a page break inside a paragraph becomes a line break: the space before the page's
/// text, else (a word glued over the break) the next space of that line, else its last space
/// before the break. `None` when the line has no space at all.
fn break_at(text: &[u8], at: usize) -> Option<usize> {
    if at > 0 && text[at - 1] == b' ' {
        return Some(at - 1);
    }
    let line_end = text[at..].iter().position(|&b| b == b'\n').map_or(text.len(), |k| at + k);
    if let Some(k) = text[at..line_end].iter().position(|&b| b == b' ') {
        return Some(at + k);
    }
    text[..at].iter().rposition(|&b| b == b' ' || b == b'\n').filter(|&k| text[k] == b' ')
}

/// Join the cleaned pages `(1-based page, lines)` into one text (see the module comment).
pub fn assemble(pages: &[(i64, Vec<PageLine>)]) -> Assembled {
    let mut a = Assembled::default();
    let mut text = String::new();
    // Byte offsets of page breaks inside a paragraph (to become '\n'), with the page.
    let mut mid_breaks: Vec<(usize, i64)> = Vec::new();
    let mut after_heading = false;
    for (page, lines) in pages {
        for (i, line) in lines.iter().enumerate() {
            let body = if line.heading { format!("## {}", line.text) } else { line.text.clone() };
            let new_para = text.is_empty() || line.para_start() || after_heading;
            after_heading = line.heading;
            if new_para {
                if !text.is_empty() {
                    text.push_str("\n\n");
                }
                a.paragraphs += 1;
                a.headings += usize::from(line.heading);
                if i == 0 {
                    a.page_starts.push((text.len(), *page));
                }
                text.push_str(&body);
                continue;
            }
            match join_kind(&text, &body) {
                Join::Dehyphen => {
                    text.pop();
                    a.dehyphen += 1;
                    a.dehyphen_over_page += usize::from(i == 0);
                }
                Join::Glue => a.glued += 1,
                Join::Direct if i > 0 => {}
                // A page break becomes a '\n' in place of a space (`break_at`), also between CJK lines.
                Join::Direct | Join::Space => text.push(' '),
            }
            if i == 0 {
                mid_breaks.push((text.len(), *page));
            }
            text.push_str(&body);
        }
    }
    a.breaks_in_paragraph = mid_breaks.len();
    let mut stored = text.into_bytes();
    for (at, page) in mid_breaks {
        if let Some(s) = break_at(&stored, at) {
            stored[s] = b'\n';
            a.page_starts.push((s + 1, page));
        }
    }
    a.page_starts.sort();
    // Only ASCII spaces were replaced by ASCII newlines.
    a.stored = String::from_utf8(stored).expect("assembled text stays UTF-8");
    a
}

/// The text the chunker reads: every single `\n` (a page break inside a paragraph) as a space,
/// blank lines kept — same byte length as `stored`, so offsets carry over unchanged.
pub fn chunk_view(stored: &str) -> String {
    let b = stored.as_bytes();
    let out: Vec<u8> = b
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let lone = c == b'\n' && (i == 0 || b[i - 1] != b'\n') && b.get(i + 1) != Some(&b'\n');
            if lone { b' ' } else { c }
        })
        .collect();
    // Only ASCII newlines were replaced by ASCII spaces.
    String::from_utf8(out).expect("chunk view stays UTF-8")
}

/// Pages of the byte range `[s, e)` of the assembled text (`starts` = `Assembled::page_starts`,
/// shifted by the caller if the text was placed further into the document); offsets in the
/// result are relative to `s`.
pub fn pages_of_range(starts: &[(usize, i64)], s: usize, e: usize) -> ChunkPages {
    let mut pages = ChunkPages::default();
    let first = starts.iter().take_while(|(o, _)| *o <= s).last().or(starts.first()).map_or(1, |(_, p)| *p);
    pages.line(0, first);
    for &(o, p) in starts.iter().filter(|(o, _)| *o > s && *o < e) {
        pages.line(o - s, p);
    }
    pages
}
