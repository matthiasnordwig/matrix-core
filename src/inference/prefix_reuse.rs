//! Which part of the previous call the local chat engine (`gguf.rs`) can keep. A multi-round
//! tool loop sends the same conversation again each round, only longer; reading the shared
//! beginning of the prompt again was most of each round's time. The engine keeps one context
//! with the tokens of its last call in memory and, per call, [`plan`]s how much of that to keep.
//!
//! Attention memory can be cut back to any position. Models with a recurrent part (Qwen3.5/3.6,
//! Mamba-like layers) or a sliding attention window cannot: their state only exists for the
//! last position. For them the engine saves that state while reading a prompt
//! ([`checkpoints_for`]): a little before its end (the reply header and a forced-tool hint at
//! the end change from round to round — the next round restores this one), and at every
//! [`CHECKPOINT_EVERY`] tokens (a new question in the same chat, or earlier rounds compacted by
//! the app, still share the long system prompt and tool list). A call restores the latest
//! saved state its prompt still shares; at most [`MAX_CHECKPOINTS`] are kept ([`retain`]).
//! Pure; tests in `inference/tests.rs`.

/// How far before the end of a prompt the state is saved (tokens).
pub const CHECKPOINT_BACK: usize = 128;
/// Spacing of the other saved states (tokens).
pub const CHECKPOINT_EVERY: usize = 4096;
/// Saved states kept per engine (each holds the recurrent part: ~63 MiB for Qwen3.6 35B-A3B).
pub const MAX_CHECKPOINTS: usize = 4;

/// How the next call starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reuse {
    /// Keep the first `keep` tokens in memory, drop the rest.
    Truncate { keep: usize },
    /// Restore the state saved at `keep`, then drop everything from `keep` on.
    Restore { keep: usize },
    /// Start with empty memory.
    Fresh,
}

/// The plan for prompt `next` when memory holds `cached` (the last prompt plus its decoded
/// answer). `partial` = the model has memory that cannot be cut back (recurrent part or sliding
/// window); `checkpoints` = positions of its saved states. At least one prompt token is always
/// read again, so the call has fresh output to sample from.
pub fn plan<T: PartialEq>(cached: &[T], next: &[T], partial: bool, checkpoints: &[usize]) -> Reuse {
    let shared = cached.iter().zip(next).take_while(|(a, b)| a == b).count();
    let keep = shared.min(next.len().saturating_sub(1));
    if keep == 0 {
        return Reuse::Fresh;
    }
    if !partial || keep == cached.len() {
        // Attention-only memory is cut at `keep`; a pure continuation cuts nothing.
        return Reuse::Truncate { keep };
    }
    match checkpoints.iter().copied().filter(|&at| at > 0 && at <= keep).max() {
        Some(at) => Reuse::Restore { keep: at },
        None => Reuse::Fresh,
    }
}

/// Where to save the state while reading a prompt of `n` tokens from position `from` on.
pub fn checkpoints_for(from: usize, n: usize) -> Vec<usize> {
    let Some(last) = n.checked_sub(CHECKPOINT_BACK).filter(|&l| l > from) else { return Vec::new() };
    let mut at: Vec<usize> = (1..).map(|k| k * CHECKPOINT_EVERY).skip_while(|&p| p <= from).take_while(|&p| p < last).collect();
    at.push(last);
    at
}

/// Which saved positions to keep (sorted): the earliest (the shared beginning of every turn)
/// and the latest ones, at most [`MAX_CHECKPOINTS`].
pub fn retain(mut positions: Vec<usize>) -> Vec<usize> {
    positions.sort_unstable();
    positions.dedup();
    while positions.len() > MAX_CHECKPOINTS {
        positions.remove(1);
    }
    positions
}
