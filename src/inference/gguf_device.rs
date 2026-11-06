//! Which unit the chat model runs on (rule and decision: `crate::compute`): the free query
//! ([`verdict`]) and the quick test ([`probe`]) where the query leaves it open.
//!
//! The test uses the model already loaded on the graphics unit and a CPU twin of the same file
//! (memory-mapped, so its weights come from the page cache, not a second read). Per unit:
//! 1. **Agreement:** a counting prompt, 8 greedy tokens. A driver fault shows as other tokens
//!    ([`agree`]). On the graphics unit this run also warms up (pipelines compile on first use).
//! 2. **Timing:** a [`PROMPT_TOKENS`]-token passage read in one batch (reading the prompt is what
//!    a cited answer spends most on) plus [`GEN_TOKENS`] tokens. The graphics unit runs it once
//!    discarded and once timed, with a context of the real size (memory overflow shows here, not
//!    later); the CPU reads in pieces and stops once it is clearly slower
//!    (`compute::cpu_clearly_slower`).
//!
//! Any error on the graphics unit counts as a failed graphics run (`Probe::gpu = None`).

use std::num::NonZeroU32;
use std::time::Instant;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use super::gguf::{GgufEngine, CHUNK};
use crate::compute::{self, Gpu, Probe, Timing, Unit, Verdict};
use crate::llama_backend::{backend, fit_to_gpu_memory, gpus, model_params_on};

/// Prompt length of the timing run.
pub const PROMPT_TOKENS: usize = 256;
/// Tokens generated after each prompt.
pub const GEN_TOKENS: usize = 8;
/// Equal first tokens for the two ways to agree.
const AGREE_PREFIX: usize = 4;
/// Pieces the CPU reads the timing prompt in (checked for stopping after each).
const CPU_PIECE: usize = 64;

const COUNTING: &str = "Count on, one number per step: 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,";
const PASSAGE: &str = "Section 4.2 of the supply contract sets out that the supplier delivers the \
    ordered goods within fourteen days of the order to the address the customer names, insured \
    and at its own risk. Should a delivery be late by more than seven days, the customer may set \
    a final period of a further seven days and withdraw from the order after it has passed; any \
    claim for damages remains unaffected. Partial deliveries are permitted where the customer can \
    reasonably be expected to accept them, and each is invoiced on its own. The customer inspects \
    the goods on arrival and reports visible defects within five working days in writing; hidden \
    defects are to be reported without delay after their discovery. ";

/// The free query: graphics units, how much of the model plus `n_ctx` fits, the rule's verdict.
pub fn verdict(model_path: &str, n_ctx: u32) -> (Verdict, Vec<Gpu>) {
    let gpus = gpus();
    let metal = cfg!(feature = "gguf-metal") && !cfg!(all(target_os = "ios", target_abi = "sim"));
    let fit = if metal || gpus.is_empty() {
        None
    } else {
        let mut params = std::pin::pin!(model_params_on(Unit::Gpu));
        fit_to_gpu_memory(params.as_mut(), model_path, n_ctx, CHUNK as u32)
    };
    (compute::gguf_verdict(metal, &gpus, fit), gpus)
}

/// Whether two greedy continuations agree (the first [`AGREE_PREFIX`] tokens equal).
pub fn agree(a: &[LlamaToken], b: &[LlamaToken]) -> bool {
    a.len() >= AGREE_PREFIX && b.len() >= AGREE_PREFIX && a[..AGREE_PREFIX] == b[..AGREE_PREFIX]
}

/// The quick test for `gpu_engine` (loaded on the graphics unit) against a CPU twin of `model_path`.
pub fn probe(gpu_engine: &GgufEngine, model_path: &str, n_ctx: u32) -> Probe {
    let model = gpu_engine.model();
    let (counting, passage) = match (tokens(model, COUNTING), passage_tokens(model)) {
        (Ok(c), Ok(p)) => (c, p),
        _ => return Probe { gpu: None, cpu: None, agree: false },
    };
    let gpu = (|| -> Result<(Vec<LlamaToken>, f64), String> {
        let mut ctx = context(model, n_ctx)?;
        let continued = run(&mut ctx, &counting, None)?.0;
        run(&mut ctx, &passage, None)?;
        Ok((continued, run(&mut ctx, &passage, None)?.1))
    })();
    let Ok((gpu_tokens, gpu_ms)) = gpu else {
        return Probe { gpu: None, cpu: None, agree: false };
    };
    let cpu = (|| -> Result<(Vec<LlamaToken>, Option<f64>), String> {
        let twin = LlamaModel::load_from_file(backend()?, model_path, &model_params_on(Unit::Cpu))
            .map_err(|e| format!("model load {model_path}: {e}"))?;
        let mut ctx = context(&twin, (PROMPT_TOKENS + 2 * GEN_TOKENS + 64) as u32)?;
        let continued = run(&mut ctx, &counting, None)?.0;
        let (_, ms) = run(&mut ctx, &passage, Some(gpu_ms))?;
        Ok((continued, ms.is_finite().then_some(ms)))
    })();
    match cpu {
        Ok((cpu_tokens, ms)) => Probe {
            gpu: Some(Timing { ms: gpu_ms }),
            cpu: ms.map(|ms| Timing { ms }),
            agree: agree(&gpu_tokens, &cpu_tokens),
        },
        // The CPU twin failing says nothing against the graphics unit: keep the one that ran.
        Err(_) => Probe { gpu: Some(Timing { ms: gpu_ms }), cpu: None, agree: true },
    }
}

fn tokens(model: &LlamaModel, text: &str) -> Result<Vec<LlamaToken>, String> {
    model.str_to_token(text, AddBos::Always).map_err(|e| format!("tokenize: {e}"))
}

/// The passage repeated up to exactly [`PROMPT_TOKENS`] tokens.
fn passage_tokens(model: &LlamaModel) -> Result<Vec<LlamaToken>, String> {
    let mut t = tokens(model, &PASSAGE.repeat(PROMPT_TOKENS / 100 + 2))?;
    t.truncate(PROMPT_TOKENS);
    Ok(t)
}

fn context(model: &LlamaModel, n_ctx: u32) -> Result<LlamaContext<'_>, String> {
    let params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(n_ctx)).with_n_batch(CHUNK as u32);
    model.new_context(backend()?, params).map_err(|e| format!("context create: {e}"))
}

/// Reads `prompt` from an empty context and greedily generates [`GEN_TOKENS`] tokens; returns
/// them and the milliseconds taken. `stop_after`: the graphics unit's time — read in pieces and
/// give up (time `INFINITY`) once projected [`compute::CLEARLY_SLOWER`].
fn run(ctx: &mut LlamaContext<'_>, prompt: &[LlamaToken], stop_after: Option<f64>) -> Result<(Vec<LlamaToken>, f64), String> {
    ctx.clear_kv_cache();
    let started = Instant::now();
    let piece = if stop_after.is_some() { CPU_PIECE } else { prompt.len() };
    let mut batch = LlamaBatch::new(prompt.len().max(1), 1);
    let mut pos = 0;
    while pos < prompt.len() {
        let end = (pos + piece).min(prompt.len());
        batch.clear();
        for (i, &tok) in prompt[pos..end].iter().enumerate() {
            let p = pos + i;
            batch.add(tok, p as i32, &[0], p == prompt.len() - 1).map_err(|e| format!("batch: {e}"))?;
        }
        ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
        pos = end;
        let ms = started.elapsed().as_secs_f64() * 1e3;
        if let Some(gpu_ms) = stop_after {
            if pos < prompt.len() && compute::cpu_clearly_slower(ms, pos, prompt.len(), gpu_ms) {
                return Ok((Vec::new(), f64::INFINITY));
            }
        }
    }
    let mut sampler = LlamaSampler::greedy();
    let mut out = Vec::with_capacity(GEN_TOKENS);
    let mut n_past = prompt.len() as i32;
    for _ in 0..GEN_TOKENS {
        let tok = sampler.sample(ctx, batch.n_tokens() - 1);
        sampler.accept(tok);
        out.push(tok);
        batch.clear();
        batch.add(tok, n_past, &[0], true).map_err(|e| format!("batch: {e}"))?;
        n_past += 1;
        ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
    }
    Ok((out, started.elapsed().as_secs_f64() * 1e3))
}
