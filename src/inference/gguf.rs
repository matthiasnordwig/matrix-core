use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::context::params::LlamaContextParams;
use std::num::NonZeroU32;

pub struct GgufEngine {
    backend: LlamaBackend,
    model: LlamaModel,
    n_ctx: u32,
}

impl GgufEngine {
    pub fn load(model_path: &str, n_ctx: u32) -> Result<Self, String> {
        let backend = LlamaBackend::init().map_err(|e| format!("llama backend init: {e}"))?;
        let model_params = LlamaModelParams::default().with_use_mmap(true);
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| format!("model load {model_path}: {e}"))?;
        Ok(Self { backend, model, n_ctx })
    }

    pub fn generate(&self, prompt: &str, max_tokens: u32) -> Result<String, String> {
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(self.n_ctx));
        let mut ctx = self.model.new_context(&self.backend, ctx_params)
            .map_err(|e| format!("context create: {e}"))?;

        // 1. Template
        let formatted_prompt = format!("<|user|>\n{}<|end|>\n<|assistant|>\n", prompt);

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
                if token_str.contains("<|end|>") {
                    break;
                }
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
