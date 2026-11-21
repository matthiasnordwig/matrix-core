//! Several calls at once on one local chat model (`gguf.rs` with more than one place): one llama.cpp
//! context owned by a worker thread, one sequence per place (`kv_unified`, so every place keeps the
//! whole window and a shared beginning lies in memory once). Calls come in over a channel and wait
//! for their answer ([`wait`]: a raised stop flag cancels the call in the worker).
//!
//! - **Places:** sequence 0 holds the shared beginning (only its end state can be copied when the
//!   model has a recurrent part, so it never reads further); sequences 1..=P are the places. Each
//!   place keeps its own memory (tokens, saved partial states) like the one-place engine, and its
//!   own sampler with the same seed. A call takes the free place whose memory shares most of its
//!   prompt (`prefix_reuse::slot_for`) — the next round of a tool loop lands where the last one is.
//! - **Shared beginning:** where a prompt stops sharing the previous call (`prefix_reuse::snapshot_at`)
//!   the place's sequence is copied to sequence 0 and saved to host memory (snapshots, the same bounds
//!   as one place). A later call starting with it copies sequence 0 (`copy_kv_cache_seq`); a saved
//!   snapshot is first restored into sequence 0. Its own memory wins where it keeps more.
//! - **Step:** the next token of every writing place, then pieces of the prompts being read, at
//!   most `CHUNK` tokens, one decode. Reading stops at saved-state positions to save them.
//! - **Full memory:** calls start together only while all fit the window without sharing anything
//!   (`prefix_reuse::admits`). If the shared memory still runs full (`decode` = 1, nothing changed),
//!   idle places and the shared beginning are freed first, then the call started last is stopped,
//!   put back in front of the queue and run again once another one is done.
//!
//! Background: `handbuch/core.md` "Mehrere Sequenzen".
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use llama_cpp_2::context::session::LlamaStateSeqFlags;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use llama_cpp_2::DecodeError;

use super::generation::Generation;
use super::gguf::{context_params, sampler, Memory, SessionKey, Snapshot, CHUNK};
use super::prefix_reuse::{self, Reuse};

/// Error of a call whose worker is gone.
pub(super) const STOPPED: &str = "the local model's worker stopped";

/// Sequence holding the shared beginning.
const HELD: i32 = 0;

/// One call for the worker.
pub(super) struct Request {
    pub tokens: Vec<LlamaToken>,
    pub turn_start: Option<usize>,
    pub max_tokens: u32,
    pub cancel: Arc<AtomicBool>,
    pub reply: Sender<Result<Generation, String>>,
}

pub(super) enum Msg {
    Run(Request),
    /// Drop what idle places and the shared beginning keep (memory pressure, `GgufEngine::drop_saved`).
    DropSaved,
}

/// The engine's handle on its worker thread; dropping it lets the worker answer what it has, then
/// waits for the thread to end.
pub(super) struct Worker {
    key: SessionKey,
    places: usize,
    tx: Option<Sender<Msg>>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    /// Starts a worker with `places` places on a context of `n_ctx` tokens (made on its thread).
    pub(super) fn spawn(model: &'static LlamaModel, n_ctx: u32, key: SessionKey, places: usize, partial: bool) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel();
        let (ready, started) = mpsc::channel();
        let params = key.clone();
        let thread = std::thread::Builder::new()
            .name("gguf-places".into())
            .spawn(move || {
                let made = crate::llama_backend::backend().and_then(|b| {
                    let p = context_params(n_ctx, &params).with_n_seq_max(places as u32 + 1).with_kv_unified(true);
                    model.new_context(b, p).map_err(|e| format!("context create: {e}"))
                });
                match made {
                    Ok(ctx) => {
                        let _ = ready.send(Ok(()));
                        Places::new(model, ctx, places, partial, n_ctx as usize).run(&rx);
                    }
                    Err(e) => {
                        let _ = ready.send(Err(e));
                    }
                }
            })
            .map_err(|e| format!("worker thread: {e}"))?;
        let mut worker = Worker { key, places, tx: Some(tx), thread: Some(thread) };
        match started.recv() {
            Ok(Ok(())) => Ok(worker),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                worker.tx = None;
                Err(STOPPED.to_string())
            }
        }
    }

    pub(super) fn places(&self) -> usize {
        self.places
    }

    /// Whether this worker takes calls with `key` and `places`.
    pub(super) fn serves(&self, key: &SessionKey, places: usize) -> bool {
        self.key == *key && self.places == places && self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    pub(super) fn sender(&self) -> Sender<Msg> {
        self.tx.clone().expect("set until drop")
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.tx = None;
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Waits for the answer to a call; once `stop` is raised the call is cancelled in the worker.
pub(super) fn wait(answer: &Receiver<Result<Generation, String>>, cancel: &AtomicBool, stop: &AtomicBool) -> Result<Generation, String> {
    loop {
        match answer.recv_timeout(Duration::from_millis(20)) {
            Ok(r) => return r,
            Err(RecvTimeoutError::Timeout) => {
                if stop.load(Ordering::Relaxed) {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return Err(STOPPED.to_string()),
        }
    }
}

/// A call in a place.
struct Job {
    req: Request,
    started: Instant,
    /// Admission order: the latest is stopped first when memory runs full.
    order: u64,
    keep: usize,
    /// Prompt tokens in memory.
    read: usize,
    checkpoints: Vec<usize>,
    snapshot_at: Option<usize>,
    read_secs: f64,
    chain: Option<LlamaSampler>,
    /// Sampled, fed with the next step.
    next: Option<LlamaToken>,
    written: u32,
    output: Vec<u8>,
}

#[derive(Default)]
struct Place {
    memory: Memory,
    job: Option<Job>,
}

/// What one place adds to a step: its next token, or prompt tokens up to `end`.
enum Part {
    Write(LlamaToken),
    Read(usize),
}

struct Places<'m> {
    model: &'m LlamaModel,
    ctx: LlamaContext<'m>,
    partial: bool,
    n_ctx: usize,
    /// Tokens in sequence [`HELD`].
    held: Vec<LlamaToken>,
    places: Vec<Place>,
    snapshots: Vec<Snapshot>,
    last_prompt: Vec<LlamaToken>,
    queue: VecDeque<Request>,
    /// Calls that may run at once: lowered after memory ran full, back to all once one is done.
    room: usize,
    admitted: u64,
}

fn seq(p: usize) -> i32 {
    p as i32 + 1
}

impl<'m> Places<'m> {
    fn new(model: &'m LlamaModel, ctx: LlamaContext<'m>, places: usize, partial: bool, n_ctx: usize) -> Self {
        let places: Vec<Place> = (0..places).map(|_| Place::default()).collect();
        let room = places.len();
        Places { model, ctx, partial, n_ctx, held: Vec::new(), places, snapshots: Vec::new(), last_prompt: Vec::new(), queue: VecDeque::new(), room, admitted: 0 }
    }

    fn busy(&self) -> bool {
        !self.queue.is_empty() || self.places.iter().any(|p| p.job.is_some())
    }

    fn run(mut self, rx: &Receiver<Msg>) {
        loop {
            if !self.busy() {
                match rx.recv() {
                    Ok(m) => self.take(m),
                    Err(_) => return,
                }
            }
            while let Ok(m) = rx.try_recv() {
                self.take(m);
            }
            self.drop_cancelled();
            self.admit();
            if self.places.iter().any(|p| p.job.is_some()) {
                self.step();
            }
        }
    }

    fn take(&mut self, m: Msg) {
        match m {
            Msg::Run(r) => self.queue.push_back(r),
            Msg::DropSaved => {
                self.free_idle();
                self.snapshots.clear();
            }
        }
    }

    fn drop_cancelled(&mut self) {
        let cancelled = |r: &Request| r.cancel.load(Ordering::Relaxed);
        for r in self.queue.iter().filter(|r| cancelled(r)) {
            let _ = r.reply.send(Err("cancelled".into()));
        }
        self.queue.retain(|r| !cancelled(r));
        for p in 0..self.places.len() {
            if self.places[p].job.as_ref().is_some_and(|j| cancelled(&j.req)) {
                self.fail(p, "cancelled".into());
            }
        }
    }

    /// Starts queued calls in free places while they fit.
    fn admit(&mut self) {
        while let Some(front) = self.queue.front() {
            let running: Vec<usize> = self.places.iter().filter_map(|p| p.job.as_ref()).map(|j| j.req.tokens.len() + j.req.max_tokens as usize).collect();
            if running.len() >= self.room || !prefix_reuse::admits(&running, front.tokens.len() + front.max_tokens as usize, self.n_ctx) {
                return;
            }
            let free: Vec<usize> = (0..self.places.len()).filter(|&p| self.places[p].job.is_none()).collect();
            let memories: Vec<&[LlamaToken]> = free.iter().map(|&p| self.places[p].memory.tokens.as_slice()).collect();
            let Some(k) = prefix_reuse::slot_for(&memories, &front.tokens) else { return };
            let (p, req) = (free[k], self.queue.pop_front().expect("front exists"));
            let (keep, snapshot_at) = self.prepare(p, &req.tokens);
            let checkpoints = if self.partial { prefix_reuse::checkpoints_for(keep, req.tokens.len(), req.turn_start) } else { Vec::new() };
            self.admitted += 1;
            self.places[p].job = Some(Job {
                started: Instant::now(),
                order: self.admitted,
                keep,
                read: keep,
                checkpoints,
                snapshot_at,
                read_secs: 0.0,
                chain: None,
                next: None,
                written: 0,
                output: Vec::new(),
                req,
            });
        }
    }

    /// The tokens a call may take from the shared beginning: all of it when the prompt starts with
    /// it (and goes on); attention-only memory also a shorter shared part.
    fn from_held(&self, tokens: &[LlamaToken]) -> usize {
        let shared = prefix_reuse::shared_len(&self.held, tokens).min(tokens.len().saturating_sub(1));
        if shared == self.held.len() || !self.partial { shared } else { 0 }
    }

    /// Prepares place `p` for `tokens` from its own memory, the shared beginning or a snapshot
    /// (whichever keeps most); returns how many are kept and where to save a snapshot.
    fn prepare(&mut self, p: usize, tokens: &[LlamaToken]) -> (usize, Option<usize>) {
        let s = seq(p);
        let shared_last = prefix_reuse::shared_len(&self.last_prompt, tokens);
        self.last_prompt = tokens.to_vec();
        let memory = &self.places[p].memory;
        let shared_own = prefix_reuse::shared_len(&memory.tokens, tokens);
        let at: Vec<usize> = memory.checkpoints.iter().map(|c| c.0).collect();
        let reuse = prefix_reuse::plan(&memory.tokens, tokens, self.partial, &at);
        let planned = match reuse {
            Reuse::Fresh => 0,
            Reuse::Truncate { keep } | Reuse::Restore { keep } => keep,
        };
        let mut held = self.from_held(tokens);
        let starts: Vec<&[LlamaToken]> = self.snapshots.iter().map(|s| s.tokens.as_slice()).collect();
        if let Some(i) = prefix_reuse::best_snapshot(&starts, tokens, planned.max(held)) {
            let snapshot = self.snapshots.remove(i);
            self.clear(HELD);
            self.held.clear();
            // SAFETY: the bytes were written by `state_seq_get_data_ext` of this context.
            if unsafe { self.ctx.state_seq_set_data_ext(&snapshot.state, HELD, LlamaStateSeqFlags::empty()) } {
                self.held.clone_from(&snapshot.tokens);
                held = self.held.len();
            } else {
                self.clear(HELD);
                held = 0;
            }
            self.snapshots.push(snapshot); // now the most recently used
        }
        let keep = if held > planned {
            self.clear(s);
            let upto = (held < self.held.len()).then_some(held as u32);
            let copied = self.ctx.copy_kv_cache_seq(HELD, s, None, upto).is_ok();
            let memory = &mut self.places[p].memory;
            memory.checkpoints.retain(|c| c.0 <= shared_own.min(held));
            memory.tokens = tokens[..held].to_vec();
            if copied { held } else { 0 }
        } else {
            match reuse {
                Reuse::Fresh => 0,
                Reuse::Truncate { keep } => {
                    if self.ctx.clear_kv_cache_seq(Some(s as u32), Some(keep as u32), None).unwrap_or(false) { keep } else { 0 }
                }
                Reuse::Restore { keep } => {
                    let memory = &self.places[p].memory;
                    let state = memory.checkpoints.iter().find(|c| c.0 == keep).map(|c| c.1.as_slice()).unwrap_or_default();
                    // SAFETY: the bytes were written by `state_seq_get_data_ext` of this context.
                    let restored = unsafe { self.ctx.state_seq_set_data_ext(state, s, LlamaStateSeqFlags::PARTIAL_ONLY) };
                    if restored && self.ctx.clear_kv_cache_seq(Some(s as u32), Some(keep as u32), None).unwrap_or(false) { keep } else { 0 }
                }
            }
        };
        if keep == 0 {
            self.clear(s);
        }
        let memory = &mut self.places[p].memory;
        memory.tokens.truncate(keep);
        memory.checkpoints.retain(|c| c.0 <= keep);
        (keep, prefix_reuse::snapshot_at(shared_last, keep, tokens.len()))
    }

    fn clear(&mut self, s: i32) {
        let _ = self.ctx.clear_kv_cache_seq(Some(s as u32), None, None);
    }

    /// Frees the memory of idle places and the shared beginning; whether anything was held.
    fn free_idle(&mut self) -> bool {
        let mut freed = !self.held.is_empty();
        self.clear(HELD);
        self.held.clear();
        for p in 0..self.places.len() {
            if self.places[p].job.is_none() && !self.places[p].memory.tokens.is_empty() {
                freed = true;
                self.clear(seq(p));
                self.places[p].memory = Memory::default();
            }
        }
        freed
    }

    /// One decode over every place with a call.
    fn step(&mut self) {
        let mut batch = LlamaBatch::new(CHUNK, 1);
        let mut parts: Vec<(usize, Part, i32)> = Vec::new();
        for (p, place) in self.places.iter().enumerate() {
            if let Some(tok) = place.job.as_ref().and_then(|j| j.next) {
                if batch.add(tok, place.memory.tokens.len() as i32, &[seq(p)], true).is_ok() {
                    parts.push((p, Part::Write(tok), batch.n_tokens() - 1));
                }
            }
        }
        for (p, place) in self.places.iter().enumerate() {
            let Some(job) = place.job.as_ref().filter(|j| j.next.is_none() && j.read < j.req.tokens.len()) else { continue };
            let budget = CHUNK.saturating_sub(batch.n_tokens() as usize);
            if budget == 0 {
                break;
            }
            let tokens = &job.req.tokens;
            let next_checkpoint = job.checkpoints.iter().copied().find(|&c| c > job.read).unwrap_or(usize::MAX);
            let next_snapshot = job.snapshot_at.filter(|&at| at > job.read).unwrap_or(usize::MAX);
            let end = (job.read + budget).min(tokens.len()).min(next_checkpoint).min(next_snapshot);
            for (i, &tok) in tokens[job.read..end].iter().enumerate() {
                let pos = job.read + i;
                let _ = batch.add(tok, pos as i32, &[seq(p)], pos == tokens.len() - 1);
            }
            parts.push((p, Part::Read(end), batch.n_tokens() - 1));
        }
        match self.ctx.decode(&mut batch) {
            Ok(()) => {
                for (p, part, idx) in parts {
                    self.after(p, part, idx);
                }
            }
            Err(DecodeError::NoKvCacheSlot) => self.memory_full(),
            Err(e) => {
                for (p, _, _) in parts {
                    self.fail(p, format!("decode: {e}"));
                }
            }
        }
    }

    /// The shared memory ran full: free what idle places hold, else stop the call started last
    /// (it runs again later), else the one call fails.
    fn memory_full(&mut self) {
        if self.free_idle() {
            return;
        }
        let running: Vec<usize> = (0..self.places.len()).filter(|&p| self.places[p].job.is_some()).collect();
        let latest = running.iter().copied().max_by_key(|&p| self.places[p].job.as_ref().map_or(0, |j| j.order));
        match latest {
            Some(p) if running.len() > 1 => {
                let job = self.places[p].job.take().expect("running");
                self.clear(seq(p));
                self.places[p].memory = Memory::default();
                eprintln!("gguf: shared memory full with {} calls at once — one waits and runs again", running.len());
                self.queue.push_front(job.req);
                self.room = running.len() - 1;
            }
            Some(p) => self.fail(p, format!("decode: {}", DecodeError::NoKvCacheSlot)),
            None => {}
        }
    }

    /// Takes in what one place added to a decoded step.
    fn after(&mut self, p: usize, part: Part, idx: i32) {
        let s = seq(p);
        let place = &mut self.places[p];
        let Some(job) = place.job.as_mut() else { return };
        match part {
            Part::Write(tok) => {
                place.memory.tokens.push(tok);
                job.next = None;
                if job.written >= job.req.max_tokens {
                    return self.finish(p, true);
                }
            }
            Part::Read(end) => {
                place.memory.tokens.extend_from_slice(&job.req.tokens[job.read..end]);
                job.read = end;
                if job.checkpoints.contains(&end) {
                    let size = self.ctx.state_seq_get_size_ext(s, LlamaStateSeqFlags::PARTIAL_ONLY);
                    let mut state = vec![0u8; size];
                    // SAFETY: `state` holds the size llama.cpp asked for.
                    let written = unsafe { self.ctx.state_seq_get_data_ext(state.as_mut_ptr(), s, LlamaStateSeqFlags::PARTIAL_ONLY) };
                    state.truncate(written);
                    place.memory.checkpoints.push((end, state));
                    let keep = prefix_reuse::retain(place.memory.checkpoints.iter().map(|c| c.0).collect(), job.req.turn_start);
                    place.memory.checkpoints.retain(|c| keep.contains(&c.0));
                }
                if job.snapshot_at == Some(end) {
                    let start = job.req.tokens[..end].to_vec();
                    self.share(s, start);
                }
                let job = self.places[p].job.as_mut().expect("still running");
                if end < job.req.tokens.len() {
                    return;
                }
                job.read_secs = job.started.elapsed().as_secs_f64();
                let mut chain = sampler();
                // Prompt tokens into the chain, so the answer does not repeat the prompt.
                for &t in &job.req.tokens {
                    chain.accept(t);
                }
                job.chain = Some(chain);
                if job.req.max_tokens == 0 {
                    return self.finish(p, true);
                }
            }
        }
        self.sample(p, idx);
    }

    /// Sequence `s` has read `start`, the beginning later calls share: it becomes the shared
    /// beginning (copied to [`HELD`]) and a snapshot in host memory.
    fn share(&mut self, s: i32, start: Vec<LlamaToken>) {
        let size = self.ctx.state_seq_get_size_ext(s, LlamaStateSeqFlags::empty());
        if size <= prefix_reuse::SNAPSHOT_BYTES {
            let mut state = vec![0u8; size];
            // SAFETY: `state` holds the size llama.cpp asked for.
            let written = unsafe { self.ctx.state_seq_get_data_ext(state.as_mut_ptr(), s, LlamaStateSeqFlags::empty()) };
            state.truncate(written);
            self.snapshots.push(Snapshot { tokens: start.clone(), state });
            let sizes: Vec<usize> = self.snapshots.iter().map(|s| s.state.len()).collect();
            self.snapshots.drain(..prefix_reuse::snapshots_to_drop(&sizes));
        }
        self.clear(HELD);
        self.held = if self.ctx.copy_kv_cache_seq(s, HELD, None, None).is_ok() { start } else { Vec::new() };
    }

    /// The next token of place `p` from the logits at `idx`.
    fn sample(&mut self, p: usize, idx: i32) {
        let job = self.places[p].job.as_mut().expect("running");
        let chain = job.chain.as_mut().expect("set when the prompt was read");
        let tok = chain.sample(&self.ctx, idx);
        if self.model.is_eog_token(tok) {
            return self.finish(p, false);
        }
        chain.accept(tok);
        if let Ok(bytes) = self.model.token_to_piece_bytes(tok, 128, true, None) {
            job.output.extend_from_slice(&bytes);
        }
        job.written += 1;
        job.next = Some(tok);
    }

    /// The call in place `p` is answered; `hit_limit`: its answer budget ran out first.
    fn finish(&mut self, p: usize, hit_limit: bool) {
        let job = self.places[p].job.take().expect("running");
        self.room = self.places.len();
        let (prompt, keep) = (job.req.tokens.len(), job.keep);
        let answer_secs = job.started.elapsed().as_secs_f64() - job.read_secs;
        eprintln!(
            "gguf: place {} of {}: prompt {prompt} tokens ({keep} kept) read in {:.2} s, answer {} tokens in {answer_secs:.2} s{}",
            p + 1,
            self.places.len(),
            job.read_secs,
            job.written,
            if hit_limit { " (stopped at the answer limit)" } else { "" }
        );
        let answer = super::chat_text::harmony_final(&String::from_utf8_lossy(&job.output)).map(|text| Generation {
            text,
            prompt_tokens: prompt,
            kept_tokens: keep,
            prompt_secs: job.read_secs,
            answer_tokens: job.written as usize,
            answer_secs,
            hit_limit,
        });
        let _ = job.req.reply.send(answer);
    }

    /// The call in place `p` ends with `error`; its memory is unknown, so the place starts fresh.
    fn fail(&mut self, p: usize, error: String) {
        if let Some(job) = self.places[p].job.take() {
            let _ = job.req.reply.send(Err(error));
        }
        self.room = self.places.len();
        self.clear(seq(p));
        self.places[p].memory = Memory::default();
    }
}
