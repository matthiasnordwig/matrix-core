//! Which part of the previous calls the local chat engine (`gguf.rs`) can keep. A multi-round
//! tool loop sends the same conversation again each round, only longer; a Grid run, an
//! extraction or a DeDup pass sends the same instructions (system prompt, schema, type lists)
//! before every row or chunk. Reading that shared beginning again was most of each call's
//! time. The engine keeps one context with the tokens of its last call in memory and, per
//! call, [`plan`]s how much of that to keep.
//!
//! Attention memory can be cut back to any position. Models with a recurrent part (Qwen3.5/3.6,
//! Mamba-like layers) or a sliding attention window cannot: their state only exists for the
//! last position. For them the engine saves that state while reading a prompt
//! ([`checkpoints_for`]): a little before its end (the reply header and a forced-tool hint at
//! the end change from round to round — the next round restores this one), and at every
//! [`CHECKPOINT_EVERY`] tokens (a new question in the same chat, or earlier rounds compacted by
//! the app, still share the long system prompt and tool list). A call restores the latest
//! saved state its prompt still shares; at most [`MAX_CHECKPOINTS`] are kept ([`retain`]).
//! A chat's next turn repeats the conversation up to this turn's question (stored without its
//! sources), so the state is also saved where the last message begins (`turn_start`).
//!
//! Beyond that the engine keeps whole saved beginnings (snapshots: memory of every layer up to
//! a position, in host RAM). Where a prompt stops sharing the previous call ([`snapshot_at`]) is
//! where the next row or chunk will diverge too, so the state there is saved; a later call that
//! starts with it restores it ([`best_snapshot`]) — also after calls of another job came in
//! between (a chat while a Grid run waits). Bounded by count and bytes ([`snapshots_to_drop`]).
//! This only pays when the shared part comes first: prompts put static text (instructions,
//! schema, tool list, the source text several calls ask about) before the variable part.
//! Pure; tests in `inference/tests.rs`.

/// How far before the end of a prompt the state is saved (tokens).
pub const CHECKPOINT_BACK: usize = 128;
/// Spacing of the other saved states (tokens).
pub const CHECKPOINT_EVERY: usize = 4096;
/// Saved states kept per engine (each holds the recurrent part: ~63 MiB for Qwen3.6 35B-A3B).
pub const MAX_CHECKPOINTS: usize = 4;
/// A snapshot is saved only this many tokens past what the call kept anyway (below, reading
/// again costs less than a second).
pub const SNAPSHOT_MIN_GAIN: usize = 256;
/// Snapshots kept per engine.
pub const MAX_SNAPSHOTS: usize = 4;
/// Their total size (Qwen3.6 35B-A3B, KV Q8_0: ~63 MiB recurrent part + ~11 KiB per token);
/// a single larger one is not saved. Less on iOS, where the app's memory budget is small.
pub const SNAPSHOT_BYTES: usize = if cfg!(target_os = "ios") { 128 << 20 } else { 512 << 20 };

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
    let shared = shared_len(cached, next);
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
/// `turn_start` = where the last message begins: the next turn of a chat sends the same
/// conversation up to there (system prompt, earlier turns) but this turn's question without
/// its sources, so it diverges right there.
pub fn checkpoints_for(from: usize, n: usize, turn_start: Option<usize>) -> Vec<usize> {
    let mut at: Vec<usize> = turn_start.filter(|&t| t > from && t < n).into_iter().collect();
    if let Some(last) = n.checked_sub(CHECKPOINT_BACK).filter(|&l| l > from) {
        at.extend((1..).map(|k| k * CHECKPOINT_EVERY).skip_while(|&p| p <= from).take_while(|&p| p < last));
        at.push(last);
    }
    at.sort_unstable();
    at.dedup();
    at
}

/// Which saved positions to keep (sorted): the earliest (the shared beginning of every turn),
/// `pinned` (this call's turn start) and the latest ones, at most [`MAX_CHECKPOINTS`].
pub fn retain(mut positions: Vec<usize>, pinned: Option<usize>) -> Vec<usize> {
    positions.sort_unstable();
    positions.dedup();
    while positions.len() > MAX_CHECKPOINTS {
        let drop = (1..positions.len()).find(|&i| Some(positions[i]) != pinned).expect("more than one unpinned");
        positions.remove(drop);
    }
    positions
}

/// How many tokens `a` and `b` share from the start.
pub fn shared_len<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// Where to save a snapshot while reading a prompt of `n` tokens from `keep` on: at `shared`,
/// the end of what it shares with the previous call — if that lies inside the prompt and at
/// least [`SNAPSHOT_MIN_GAIN`] tokens past what was kept anyway.
pub fn snapshot_at(shared: usize, keep: usize, n: usize) -> Option<usize> {
    (shared >= keep + SNAPSHOT_MIN_GAIN && shared < n).then_some(shared)
}

/// The snapshot to restore for `next`: the longest one `next` starts with, if it is longer than
/// `keep` (what the context keeps anyway) and leaves at least one prompt token to read.
pub fn best_snapshot<T: PartialEq>(snapshots: &[&[T]], next: &[T], keep: usize) -> Option<usize> {
    snapshots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.len() > keep && s.len() < next.len() && next.starts_with(s))
        .max_by_key(|(_, s)| s.len())
        .map(|(i, _)| i)
}

/// How many of the snapshots (sizes in bytes, least recently used first) to drop so that at
/// most [`MAX_SNAPSHOTS`] remain within [`SNAPSHOT_BYTES`].
pub fn snapshots_to_drop(sizes: &[usize]) -> usize {
    let mut drop = sizes.len().saturating_sub(MAX_SNAPSHOTS);
    while sizes[drop..].iter().sum::<usize>() > SNAPSHOT_BYTES {
        drop += 1;
    }
    drop
}
