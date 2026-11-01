//! Which small differences keep repeated mail passages apart: groups bodies by their first and by
//! their last 200 characters and counts what differs. Measurement only, no writes.
//!   cargo run --release --example chunk_doppel -- <matrix.db> <context_id> <top_n>

use std::collections::{HashMap, HashSet};

use app::chunking::mail::{chunk_mail, passage_body};
use rusqlite::{Connection, OpenFlags};

const EDGE: usize = 200;
const SHOW: usize = 70;

fn head(s: &str) -> String {
    s.chars().take(SHOW).collect()
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let conn = Connection::open_with_flags(&a[0], OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open");
    let top: usize = a[2].parse().unwrap();
    let mut stmt = conn.prepare("SELECT extracted_text FROM documents WHERE context_id = ?1").unwrap();
    let mut rows = stmt.query([a[1].parse::<i64>().unwrap()]).unwrap();
    let mut seen: HashSet<u64> = HashSet::new();
    let mut by_start: HashMap<String, String> = HashMap::new();
    let mut by_end: HashMap<String, String> = HashMap::new();
    let (mut tails, mut starts): (HashMap<String, (usize, u64)>, HashMap<String, (usize, u64)>) = Default::default();
    let (mut bodies, mut bytes, mut unique, mut unique_bytes, mut near_start, mut near_end, mut near_bytes) = (0usize, 0u64, 0usize, 0u64, 0usize, 0usize, 0u64);
    while let Some(r) = rows.next().unwrap() {
        let Some(text): Option<String> = r.get(0).unwrap() else { continue };
        for c in chunk_mail(&text, 1, 1) {
            let body = passage_body(&c.text);
            let chars: Vec<char> = body.chars().collect();
            if chars.len() < EDGE {
                continue;
            }
            bodies += 1;
            bytes += body.len() as u64;
            let id = fxhash(&body);
            if !seen.insert(id) {
                continue;
            }
            unique += 1;
            unique_bytes += body.len() as u64;
            let start: String = chars[..EDGE].iter().collect();
            let end: String = chars[chars.len() - EDGE..].iter().collect();
            if let Some(other) = by_start.get(&start) {
                let same = chars.iter().zip(other.chars()).take_while(|(x, y)| *x == y).count();
                let rest: String = if chars.len() >= other.chars().count() { chars[same..].iter().collect() } else { other.chars().skip(same).collect() };
                let e = tails.entry(head(&rest)).or_default();
                e.0 += 1;
                e.1 += rest.len() as u64;
                near_start += 1;
                near_bytes += body.len().min(other.len()) as u64;
            } else if let Some(other) = by_end.get(&end) {
                let (rx, ro): (Vec<char>, Vec<char>) = (chars.iter().rev().copied().collect(), other.chars().rev().collect());
                let same = rx.iter().zip(&ro).take_while(|(x, y)| x == y).count();
                let rest: String = if rx.len() >= ro.len() { rx[same..].iter().rev().collect() } else { ro[same..].iter().rev().collect() };
                let e = starts.entry(head(&rest)).or_default();
                e.0 += 1;
                e.1 += rest.len() as u64;
                near_end += 1;
                near_bytes += body.len().min(other.len()) as u64;
            }
            by_start.entry(start).or_insert_with(|| body.clone());
            by_end.entry(end).or_insert(body);
        }
    }
    println!("bodies {bodies} bytes {bytes} unique {unique} unique_bytes {unique_bytes} near_same_start {near_start} near_same_end {near_end} near_bytes {near_bytes}");
    for (name, map) in [("ENDE WEICHT AB", tails), ("ANFANG WEICHT AB", starts)] {
        println!("== {name}");
        let mut v: Vec<_> = map.into_iter().collect();
        v.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
        for (k, (n, b)) in v.into_iter().take(top) {
            println!("{n}\t{}\t{k}", b / n as u64);
        }
    }
}

fn fxhash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
