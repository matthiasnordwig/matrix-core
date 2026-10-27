//! Pages of one chunk: on which PDF page each part of a chunk's text stands.
//!
//! A chunk can run over a page break; its `page` (metadata) is only the start
//! page. The structural chunker (`structural.rs`) records every page change
//! inside a chunk as `"page_breaks": [[byte_offset, page], …]` in the chunk's
//! metadata — offsets are byte offsets into the chunk's own `text`, each at the
//! start of a line, pages 1-based; a chunk on one page gets `"page_breaks": []`.
//! The key itself is the mark of a chunker that records page changes: a PDF
//! document none of whose chunks has it was read before (core
//! `db/page_refresh.rs`). Readers ([`ChunkPages::from_metadata`])
//! mark the changes in the text (`[p. 44]`, [`ChunkPages::mark`]), name the
//! page range and find the page of a quoted passage ([`ChunkPages::locate`]).
//!
//! Chunks without `page_breaks` (other chunkers, or chunked before the key
//! existed) read as lying on their start page only.

use serde_json::{json, Map, Value};

const BREAKS_KEY: &str = "page_breaks";

/// Page changes of one chunk: `(byte offset into the chunk text, 1-based page)`,
/// the first entry at offset 0 with the start page, offsets ascending.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkPages(Vec<(usize, i64)>);

impl ChunkPages {
    /// While a chunk's text is built: the line starting at `offset` stands on `page`.
    pub fn line(&mut self, offset: usize, page: i64) {
        if self.0.last().is_none_or(|(_, p)| *p != page) {
            self.0.push((offset, page));
        }
    }

    /// Cut at byte `at` of the text; the second part's text starts `skip` bytes
    /// later (whitespace trimmed off its start) and starts on the page that
    /// stands there.
    pub fn split_at(&self, at: usize, skip: usize) -> (ChunkPages, ChunkPages) {
        let head = ChunkPages(self.0.iter().copied().filter(|(o, _)| *o < at).collect());
        let mut tail = ChunkPages::default();
        if let Some(p) = self.page_at(at + skip) {
            tail.0.push((0, p));
        }
        for &(o, p) in self.0.iter().filter(|(o, _)| *o > at + skip) {
            tail.line(o - at - skip, p);
        }
        (head, tail)
    }

    /// Another chunk's text was appended at byte `shift` of this one.
    pub fn append(&mut self, other: &ChunkPages, shift: usize) {
        for &(o, p) in &other.0 {
            self.line(o + shift, p);
        }
    }

    /// Write `page` (start page) and `page_breaks` (empty for a chunk on one page).
    pub fn stamp(&self, meta: &mut Map<String, Value>) {
        let Some(&(_, first)) = self.0.first() else { return };
        meta.insert("page".into(), json!(first));
        let breaks: Vec<Value> = self.0[1..].iter().map(|(o, p)| json!([o, p])).collect();
        meta.insert(BREAKS_KEY.into(), Value::Array(breaks));
    }

    /// The pages of a chunk from its metadata JSON; `None` without a page.
    pub fn from_metadata(metadata: &str) -> Option<ChunkPages> {
        let meta: Value = serde_json::from_str(metadata).ok()?;
        let first = meta.get("page")?.as_i64().filter(|p| *p > 0)?;
        let mut pages = ChunkPages(vec![(0, first)]);
        for b in meta.get(BREAKS_KEY).and_then(Value::as_array).into_iter().flatten() {
            let (Some(o), Some(p)) = (b.get(0).and_then(Value::as_u64), b.get(1).and_then(Value::as_i64)) else { continue };
            if o > 0 && pages.0.last().is_some_and(|(lo, _)| (o as usize) > *lo) {
                pages.line(o as usize, p);
            }
        }
        Some(pages)
    }

    /// First and last page.
    pub fn range(&self) -> (i64, i64) {
        let first = self.0.first().map_or(0, |(_, p)| *p);
        (first, self.0.last().map_or(first, |(_, p)| *p))
    }

    /// `p. 43` or `pp. 43–44`.
    pub fn label(&self) -> String {
        match self.range() {
            (a, b) if a == b => format!("p. {a}"),
            (a, b) => format!("pp. {a}\u{2013}{b}"),
        }
    }

    /// The page the byte `offset` of the text stands on.
    pub fn page_at(&self, offset: usize) -> Option<i64> {
        self.0.iter().take_while(|(o, _)| *o <= offset).last().map(|(_, p)| *p)
    }

    /// The text split at the page changes: `(page, start, end)` byte ranges. A
    /// recorded offset that does not start a line of `text` is ignored (the text
    /// is not the one the offsets were taken from).
    fn parts(&self, text: &str) -> Vec<(i64, usize, usize)> {
        let mut out: Vec<(i64, usize, usize)> = Vec::new();
        for &(o, p) in &self.0 {
            let starts_line = o == 0 || (o < text.len() && text.is_char_boundary(o) && text.as_bytes()[o - 1] == b'\n');
            if !starts_line {
                continue;
            }
            if let Some(last) = out.last_mut() {
                last.2 = o;
            }
            out.push((p, o, text.len()));
        }
        out
    }

    /// `text` with `[p. N]` on its own line where page N begins (after the start page).
    pub fn mark(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 10 * self.0.len());
        for (i, (p, s, e)) in self.parts(text).into_iter().enumerate() {
            if i > 0 {
                out.push_str(&format!("[p. {p}]\n"));
            }
            out.push_str(&text[s..e]);
        }
        out
    }

    /// First and last page of the shortest run of consecutive pages whose text
    /// `contains` the passage (a quote over a page break spans two); `None` when
    /// no run does.
    pub fn locate(&self, text: &str, contains: impl Fn(&str) -> bool) -> Option<(i64, i64)> {
        let parts = self.parts(text);
        for len in 1..=parts.len() {
            for w in parts.windows(len) {
                if contains(&text[w[0].1..w[len - 1].2]) {
                    return Some((w[0].0, w[len - 1].0));
                }
            }
        }
        None
    }
}
