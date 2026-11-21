//! The quick measurement behind "how many table rows at once" (app `local_places.rs` decides and keeps
//! the result per device, model and app version): rows per second of the loaded chat model with 1, 2,
//! 4 … places, on a context like the worker's (`gguf_places.rs`: `kv_unified`, sequence 0 holds a
//! shared beginning that is copied onto the places). Every candidate answers the same [`ROWS`] small
//! rows — a fixed text, [`SHARED`] tokens shared, [`READ`] own tokens read and [`WRITE`] written per
//! row, greedily and without stopping at an end token, so every run does the same work. Reading gains
//! little from more places, writing much; real table rows read far more than they write (classic rows
//! ~2.5–4.5k tokens read, 100–150 written), so the measurement reads four times what it writes —
//! more than the feasibility measurement (`handbuch/core.md` "Mehrere Sequenzen": ~380 : 150), whose
//! gains it would otherwise overstate, and still short. A first short run warms up the kernels (Metal compiles them on first use)
//! and is discarded. Takes a few seconds; the weights are the engine's, the context is its own.
use std::time::Instant;

use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::AddBos;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use super::gguf::{context_params, GgufEngine, CHUNK};

/// Tokens every row shares (read once).
pub const SHARED: usize = 256;
/// Own tokens every row reads.
pub const READ: usize = 160;
/// Tokens every row writes.
pub const WRITE: usize = 40;
/// Rows per candidate (the largest candidate answers them in one go).
pub const ROWS: usize = 4;

/// Fixed text the rows are cut from.
const TEXT: &str = "The committee met on a Tuesday morning to review the quarterly report. Each member read the \
    figures for their own department, compared them with the plan and noted where costs had risen. The \
    warehouse had moved to a new building in spring, which delayed two deliveries and raised the rent. \
    Sales in the northern region grew faster than expected, while the southern office lost a large customer \
    after a dispute about a late invoice. The board asked for a short summary of every item, the decisions \
    taken, who is responsible and the date by which each task must be done. ";

/// Rows per second for each of `candidates` places (ascending, each at most [`ROWS`]).
pub fn rows_per_second(engine: &GgufEngine, kv_quantization: Option<&str>, cpu_threads: Option<i64>, candidates: &[usize]) -> Result<Vec<(usize, f64)>, String> {
    let model = engine.model();
    let text: Vec<LlamaToken> = model.str_to_token(&TEXT.repeat(12), AddBos::Always).map_err(|e| format!("tokenize: {e}"))?;
    let need = SHARED + ROWS * READ;
    if text.len() < need {
        return Err(format!("probe text too short: {} tokens", text.len()));
    }
    let rows: Vec<&[LlamaToken]> = (0..ROWS).map(|r| &text[SHARED + r * READ..SHARED + (r + 1) * READ]).collect();
    let most = candidates.iter().copied().max().unwrap_or(1).max(1);
    let n_ctx = ((SHARED + ROWS * (READ + WRITE)) as u32).next_multiple_of(256) + 256;
    let key = GgufEngine::key_for(kv_quantization, cpu_threads);
    let params = context_params(n_ctx, &key).with_n_seq_max(most as u32 + 1).with_kv_unified(true);
    let mut ctx = model.new_context(crate::llama_backend::backend()?, params).map_err(|e| format!("context create: {e}"))?;
    run(&mut ctx, &text[..SHARED], &rows[..1], 1)?;
    let mut rates = Vec::new();
    for &n in candidates {
        let secs = run(&mut ctx, &text[..SHARED], &rows, n.clamp(1, most))?;
        rates.push((n, rows.len() as f64 / secs.max(1e-6)));
    }
    Ok(rates)
}

/// Answers `rows` after `shared` with `n` places at once; returns the seconds taken.
fn run(ctx: &mut LlamaContext<'_>, shared: &[LlamaToken], rows: &[&[LlamaToken]], n: usize) -> Result<f64, String> {
    ctx.clear_kv_cache();
    let started = Instant::now();
    let mut batch = LlamaBatch::new(CHUNK, 1);
    for (i, &tok) in shared.iter().enumerate() {
        batch.add(tok, i as i32, &[0], false).map_err(|e| format!("batch: {e}"))?;
    }
    ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
    let mut sampler = LlamaSampler::greedy();
    // Per place: (row, tokens read, tokens written, next token, logits index).
    let mut places: Vec<Option<(usize, usize, usize, Option<LlamaToken>)>> = vec![None; n];
    let mut queue = 0..rows.len();
    loop {
        for (p, place) in places.iter_mut().enumerate().filter(|(_, pl)| pl.is_none()) {
            let Some(r) = queue.next() else { break };
            let s = p as i32 + 1;
            let _ = ctx.clear_kv_cache_seq(Some(s as u32), None, None);
            ctx.copy_kv_cache_seq(0, s, None, None).map_err(|e| format!("copy: {e}"))?;
            *place = Some((r, 0, 0, None));
        }
        if places.iter().all(Option::is_none) {
            return Ok(started.elapsed().as_secs_f64());
        }
        batch.clear();
        let mut logits = vec![-1; n];
        for (p, place) in places.iter().enumerate() {
            if let Some((_, read, written, Some(tok))) = place {
                batch.add(*tok, (shared.len() + read + written - 1) as i32, &[p as i32 + 1], true).map_err(|e| format!("batch: {e}"))?;
                logits[p] = batch.n_tokens() - 1;
            }
        }
        for (p, place) in places.iter_mut().enumerate() {
            let Some((r, read, _, None)) = place else { continue };
            let end = (*read + CHUNK.saturating_sub(batch.n_tokens() as usize)).min(rows[*r].len());
            for i in *read..end {
                batch.add(rows[*r][i], (shared.len() + i) as i32, &[p as i32 + 1], i == rows[*r].len() - 1).map_err(|e| format!("batch: {e}"))?;
            }
            *read = end;
            if end == rows[*r].len() {
                logits[p] = batch.n_tokens() - 1;
            }
        }
        ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
        for (p, place) in places.iter_mut().enumerate() {
            let Some((_, _, written, next)) = place.as_mut() else { continue };
            if logits[p] < 0 {
                continue;
            }
            if *written == WRITE {
                *place = None;
                continue;
            }
            *next = Some(sampler.sample(ctx, logits[p]));
            *written += 1;
        }
    }
}
