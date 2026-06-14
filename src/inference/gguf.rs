use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::context::params::LlamaContextParams;
use std::num::NonZeroU32;
use std::sync::{OnceLock, Mutex};

static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
static BACKEND_INIT_MUTEX: Mutex<()> = Mutex::new(());

fn get_backend() -> Result<&'static LlamaBackend, String> {
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let _lock = BACKEND_INIT_MUTEX.lock().unwrap();
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let backend = LlamaBackend::init().map_err(|e| format!("llama backend init: {e}"))?;
    let _ = BACKEND.set(backend);
    BACKEND.get().ok_or_else(|| "Failed to set LlamaBackend".to_string())
}

#[derive(Clone, Copy)]
pub enum ChatTemplate {
    Phi3,
    ChatML,
    Llama3,
}

pub struct GgufEngine {
    model: LlamaModel,
    n_ctx: u32,
    template: ChatTemplate,
}

impl GgufEngine {
    pub fn load(model_path: &str, n_ctx: u32) -> Result<Self, String> {
        let backend = get_backend()?;
        #[allow(unused_mut)]
        let mut model_params = LlamaModelParams::default();
        
        // Disable mmap on iOS because iOS Personal Teams lack the Extended Virtual
        // Addressing entitlement, meaning large mmaps will silently fail and return null.
        #[cfg(target_os = "ios")]
        {
            model_params = model_params.with_use_mmap(false);
        }
        #[cfg(not(target_os = "ios"))]
        {
            model_params = model_params.with_use_mmap(true);
        }
        
        #[cfg(feature = "gguf-metal")]
        {
            model_params = model_params.with_n_gpu_layers(999);
        }
        let model = LlamaModel::load_from_file(backend, model_path, &model_params)
            .map_err(|e| format!("model load {model_path}: {e}"))?;
            
        let lower_path = model_path.to_lowercase();
        let template = if lower_path.contains("qwen") || lower_path.contains("chatml") {
            ChatTemplate::ChatML
        } else if lower_path.contains("llama-3") || lower_path.contains("llama3") {
            ChatTemplate::Llama3
        } else {
            ChatTemplate::Phi3
        };
            
        Ok(Self { model, n_ctx, template })
    }

    pub fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, String> {
        let backend = get_backend()?;
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(self.n_ctx));
        let mut ctx = self.model.new_context(backend, ctx_params)
            .map_err(|e| format!("context create: {e}"))?;

        // 1. Template
        let (formatted_prompt, stop_tokens) = match self.template {
            ChatTemplate::ChatML => (
                format!("<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n", prompt),
                vec!["<|im_end|>"]
            ),
            ChatTemplate::Llama3 => (
                format!("<|start_header_id|>user<|end_header_id|>\n\n{}<|eot_id|><|start_header_id|>assistant<|end_header_id|>\n\n", prompt),
                vec!["<|eot_id|>", "<|end_of_text|>"]
            ),
            ChatTemplate::Phi3 => (
                format!("<|user|>\n{}<|end|>\n<|assistant|>\n", prompt),
                vec!["<|end|>"]
            ),
        };

        // 2. Tokenize
        let tokens = self.model.str_to_token(&formatted_prompt, llama_cpp_2::model::AddBos::Always)
            .map_err(|e| format!("tokenize: {e}"))?;

        // Check if prompt fits in context window
        let available_ctx = self.n_ctx.saturating_sub(max_tokens);
        if tokens.len() > available_ctx as usize {
            return Err(format!("Prompt zu lang: {} Tokens. (Verfügbar: {} Kontext - {} Reserve = {})", 
                tokens.len(), self.n_ctx, max_tokens, available_ctx));
        }

        // 3. Eval prompt in chunks
        let chunk_size = 512;
        let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(chunk_size, 1);
        let mut n_past = 0;
        
        for chunk in tokens.chunks(chunk_size) {
            batch.clear();
            for (i, &tok) in chunk.iter().enumerate() {
                let is_last = (n_past as usize + i) == (tokens.len() - 1);
                batch.add(tok, n_past + i as i32, &[0], is_last).unwrap();
            }
            ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
            n_past += chunk.len() as i32;
        }

        // 4. Sampling loop
        let mut output = String::new();
        for _ in 0..max_tokens {
            let mut candidates = llama_cpp_2::token::data_array::LlamaTokenDataArray::from_iter(ctx.candidates_ith(batch.n_tokens() - 1), false);
            let next_token = candidates.sample_token_greedy();
            
            if next_token == self.model.token_eos() {
                break;
            }

            if let Ok(token_bytes) = self.model.token_to_piece_bytes(next_token, 128, true, None) {
                let token_str = String::from_utf8_lossy(&token_bytes);
                
                let mut should_stop = false;
                for stop in &stop_tokens {
                    if token_str.contains(stop) {
                        should_stop = true;
                        break;
                    }
                }
                if should_stop { break; }
                
                output.push_str(&token_str);
            }

            batch.clear();
            batch.add(next_token, n_past, &[0], true).unwrap();
            n_past += 1;

            ctx.decode(&mut batch).map_err(|e| format!("decode: {e}"))?;
        }

        Ok(output)
    }
}
