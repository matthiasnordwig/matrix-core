use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::context::params::{LlamaContextParams, KvCacheType};
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

pub struct GgufEngine {
    model: LlamaModel,
    n_ctx: u32,
}

impl GgufEngine {
    pub fn load(model_path: &str, n_ctx: u32) -> Result<Self, String> {
        let backend = get_backend()?;
        #[allow(unused_mut)]
        let mut model_params = LlamaModelParams::default();
        
        model_params = model_params.with_use_mmap(true);
        
        #[cfg(feature = "gguf-metal")]
        {
            model_params = model_params.with_n_gpu_layers(999);
        }
        if !std::path::Path::new(model_path).exists() {
            return Err(format!("FILE NOT FOUND! iOS sucht nach der Datei unter dem Pfad: {}", model_path));
        }

        let model = LlamaModel::load_from_file(backend, model_path, &model_params)
            .map_err(|e| format!("model load {model_path}: {e}"))?;
            
        Ok(Self { model, n_ctx })
    }

    pub fn generate(&self, messages: &[(&str, &str)], max_tokens: u32, is_reasoning: bool, json_schema: Option<&str>, kv_quantization: Option<&str>, cpu_threads: Option<i64>) -> Result<String, String> {
        let backend = get_backend()?;
        let mut ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(self.n_ctx));

        if let Some(q) = kv_quantization {
            let kv_type = match q {
                "Q8_0" => KvCacheType::Q8_0,
                "Q4_0" => KvCacheType::Q4_0,
                _ => KvCacheType::F16,
            };
            ctx_params = ctx_params.with_type_k(kv_type).with_type_v(kv_type);
        }
        
        if let Some(t) = cpu_threads {
            ctx_params = ctx_params.with_n_threads(t as i32).with_n_threads_batch(t as i32);
        }

        let mut ctx = self.model.new_context(backend, ctx_params)
            .map_err(|e| format!("context create: {e}"))?;

        // 1. Template (read native from GGUF)
        let tmpl = self.model.chat_template(None)
            .map_err(|e| format!("model has no default chat template or invalid: {e}"))?;
            
        let mut chat_msgs = Vec::new();
        for (role, content) in messages {
            chat_msgs.push(llama_cpp_2::model::LlamaChatMessage::new(role.to_string(), content.to_string())
                .map_err(|e| format!("invalid chat message: {e}"))?);
        }
        
        let formatted_prompt = self.model.apply_chat_template(&tmpl, &chat_msgs, true)
            .map_err(|e| format!("failed to apply chat template: {e}"))?;

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

        let mut samplers = vec![
            llama_cpp_2::sampling::LlamaSampler::penalties(64, 1.1, 0.0, 0.0),
            llama_cpp_2::sampling::LlamaSampler::top_k(40),
            llama_cpp_2::sampling::LlamaSampler::top_p(0.9, 1),
            llama_cpp_2::sampling::LlamaSampler::temp(0.7),
            llama_cpp_2::sampling::LlamaSampler::dist(1337),
        ];

        if let Some(schema) = json_schema {
            let grammar_str = llama_cpp_2::json_schema_to_grammar(schema)
                .map_err(|e| format!("invalid json schema: {}", e))?;
            
            let grammar = if is_reasoning {
                llama_cpp_2::sampling::LlamaSampler::grammar_lazy(
                    &self.model,
                    &grammar_str,
                    "root",
                    &["</think>"],
                    &[]
                ).map_err(|e| format!("grammar lazy init: {:?}", e))?
            } else {
                llama_cpp_2::sampling::LlamaSampler::grammar(
                    &self.model,
                    &grammar_str,
                    "root"
                ).map_err(|e| format!("grammar init: {:?}", e))?
            };
            samplers.insert(0, grammar);
        }

        let mut chain = llama_cpp_2::sampling::LlamaSampler::chain_simple(samplers);

        // Accept prompt tokens into the chain to avoid repeating the prompt
        for &t in &tokens {
            chain.accept(t);
        }

        // 4. Sampling loop
        let mut output = String::new();
        for _ in 0..max_tokens {
            let next_token = chain.sample(&ctx, batch.n_tokens() - 1);
            
            // Check EOS/EOG (handles ChatML <|im_end|>, Llama <|eot_id|>, etc automatically)
            if self.model.is_eog_token(next_token) {
                break;
            }

            chain.accept(next_token);

            if let Ok(token_bytes) = self.model.token_to_piece_bytes(next_token, 128, true, None) {
                let token_str = String::from_utf8_lossy(&token_bytes);
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
