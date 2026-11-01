//! Compares the old and the new mail cuts without vectors: reads the stored mail text of a collection
//! and runs both chunkers over it. Measurement only, no writes.
//!   cargo run --release --example chunk_vergleich -- <matrix.db> <context_id> <samples.txt>
//! Prints JSON with passages and lengths per chunker; writes the cuts of every 20 000th mail with
//! more than one block to `samples.txt`.

use std::fmt::Write as _;

use app::chunking::mail::{chunk_mail, duplicate_key};
use app::chunking::markdown::{chunk_markdown_structurally, embedding_parts, DEFAULT_MAX_CHARS};
use app::db::models::NewChunk;
use rusqlite::{Connection, OpenFlags};

const LIMITS: [usize; 5] = [3_000, 8_000, 16_000, 30_000, 60_000];

#[derive(Default)]
struct Tally {
    mails: usize,
    passages: usize,
    single: usize,
    chars: u64,
    lengths: Vec<usize>,
    quoted: usize,
    quoted_without_author: usize,
    split_blocks: usize,
    longest_ms: u128,
    total_ms: u128,
}

impl Tally {
    fn add(&mut self, cs: &[NewChunk], micros: u128) {
        self.mails += 1;
        self.passages += cs.len();
        self.single += usize::from(cs.len() == 1);
        self.total_ms += micros;
        self.longest_ms = self.longest_ms.max(micros);
        for c in cs {
            let n = c.text.chars().count();
            self.chars += n as u64;
            self.lengths.push(n);
            if c.metadata.contains("\"mail_quoted\"") {
                self.quoted += 1;
                self.quoted_without_author += usize::from(c.metadata.contains("\"quoted_from\":null"));
            }
            self.split_blocks += usize::from(c.metadata.contains("\"parts\":"));
        }
    }

    fn json(&mut self) -> serde_json::Value {
        self.lengths.sort_unstable();
        let at = |p: f64| self.lengths[((self.lengths.len() - 1) as f64 * p) as usize];
        let over: serde_json::Map<_, _> =
            LIMITS.iter().map(|&l| (l.to_string(), self.lengths.iter().filter(|&&n| n > l).count().into())).collect();
        serde_json::json!({
            "mails": self.mails, "passages": self.passages, "single_passage_mails": self.single, "chars": self.chars,
            "median": at(0.5), "p90": at(0.9), "p99": at(0.99), "max": at(1.0),
            "over": over,
            "quoted": self.quoted, "quoted_without_author": self.quoted_without_author, "cut_block_parts": self.split_blocks,
            "seconds": self.total_ms / 1_000_000, "slowest_mail_ms": self.longest_ms / 1000,
        })
    }
}

fn sample(out: &mut String, doc: i64, text: &str, cs: &[NewChunk]) {
    let _ = writeln!(out, "\n=== Dokument {doc}, {} Zeichen, {} Passagen", text.chars().count(), cs.len());
    for c in cs {
        let meta: serde_json::Value = serde_json::from_str(&c.metadata).unwrap_or_default();
        let head: String = c.text.chars().take(90).collect::<String>().replace('\n', " / ");
        let _ = writeln!(out, "[{}] {} {}: {head}", c.chunk_index, meta["chunk_type"], c.text.chars().count());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [db, context, samples] = args.as_slice() else {
        eprintln!("usage: see header of examples/chunk_vergleich.rs");
        std::process::exit(2);
    };
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open");
    let mut stmt = conn.prepare("SELECT id, extracted_text FROM documents WHERE context_id = ?1 ORDER BY id").expect("prepare");
    let mut rows = stmt.query([context.parse::<i64>().expect("context id")]).expect("query");
    let (mut old, mut new) = (Tally::default(), Tally::default());
    let mut out = String::new();
    let mut multi = 0usize;
    let mut seen = std::collections::HashSet::new();
    let (mut kept, mut kept_chars, mut parts, mut small) = (0usize, 0u64, 0usize, 0usize);
    while let Some(r) = rows.next().expect("row") {
        let id: i64 = r.get(0).expect("id");
        let Some(text): Option<String> = r.get(1).expect("text") else { continue };
        let t = std::time::Instant::now();
        let a = chunk_markdown_structurally(&text, 1, id, DEFAULT_MAX_CHARS);
        old.add(&a, t.elapsed().as_micros());
        let t = std::time::Instant::now();
        let b = chunk_mail(&text, 1, id);
        new.add(&b, t.elapsed().as_micros());
        for c in &b {
            if duplicate_key(c).is_none_or(|k| seen.insert(k)) {
                kept += 1;
                kept_chars += c.text.len() as u64;
                parts += embedding_parts(&c.text, 6_000).len();
                small += embedding_parts(&c.text, 2_000).len();
            }
        }
        if b.len() > 1 {
            multi += 1;
            if multi % 20_000 == 1 {
                sample(&mut out, id, &text, &b);
            }
        }
    }
    std::fs::write(samples, out).expect("samples");
    println!("{}", serde_json::json!({ "old": old.json(), "new": new.json(), "embedded_once": { "passages": kept, "bytes": kept_chars, "vectors": parts, "vectors_2000": small } }));
}
