//! Build-up pages of a slide PDF: a slide exported with its build steps (each step one page, as Keynote's
//! "print each stage of builds" or Beamer overlays write them) becomes one unit over its pages, so a
//! search does not find the same points three times.
//!
//! [`step_runs`] walks the pages in order. A page joins the run before it when
//! - it shows the same slide as the run's latest page by the text rule of video stills (core
//!   `media/slide_track.rs` [`same_words`]: at least three words on both, the shorter text's words to at
//!   least 80 % in the longer one), and
//! - **nothing is lost**: every token (letters or digits, any length, counted with repetition) of every
//!   page left out is on the kept page ([`covers`]). A changed number ("5 %" → "6 %") or a corrected word
//!   splits the run, even where the slide rule calls them the same slide.
//!
//! The kept page is the one with the most words (the fully built slide; on a tie the later one), as for
//! video stills. A page without text never joins (the picture rule of video stills needs a rendering).
//! Consumer: `structural.rs`, which leaves out the lines of the other pages and records the run's first
//! page before the kept page's text (`pages.rs`: a page change without text of its own), so the unit
//! names its page range while a quote is located on the kept page. Rules and interplay with page images:
//! `handbuch/medien.md` „Folien-PDFs mit Aufbau-Schritten“.

use std::collections::HashMap;

use crate::media::slide_track::{same_words, words};

/// Consecutive pages of one built-up slide (0-based page indices, `first < last`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepRun {
    pub first: usize,
    pub last: usize,
    /// The page whose text stands for the run.
    pub kept: usize,
}

/// Tokens of `text` (lower-case runs of letters or digits) with their counts.
fn tokens(text: &str) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    for t in text.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()) {
        *out.entry(t.to_lowercase()).or_insert(0) += 1;
    }
    out
}

/// Every token of `omitted` is in `kept`, at least as often.
pub fn covers(kept: &str, omitted: &str) -> bool {
    let k = tokens(kept);
    tokens(omitted).iter().all(|(t, n)| k.get(t).is_some_and(|m| m >= n))
}

/// The runs of two or more pages (see the module comment); `pages` holds each page's text in order.
pub fn step_runs(pages: &[String]) -> Vec<StepRun> {
    let n_words: Vec<usize> = pages.iter().map(|p| words(p).len()).collect();
    let mut runs = Vec::new();
    let mut run = StepRun { first: 0, last: 0, kept: 0 };
    for j in 1..pages.len() {
        let kept = if n_words[j] >= n_words[run.kept] { j } else { run.kept };
        let joins = same_words(&words(&pages[run.last]), &words(&pages[j])) == Some(true)
            && (run.first..=j).filter(|&p| p != kept).all(|p| covers(&pages[kept], &pages[p]));
        if joins {
            run.last = j;
            run.kept = kept;
        } else {
            if run.last > run.first {
                runs.push(run);
            }
            run = StepRun { first: j, last: j, kept: j };
        }
    }
    if run.last > run.first {
        runs.push(run);
    }
    runs
}

#[cfg(test)]
#[path = "slide_steps_tests.rs"]
mod tests;
