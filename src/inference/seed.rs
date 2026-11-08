//! Seed of the local chat model's sampler: fixed at 1337 in the app; the environment variable
//! `MATRIX_GGUF_SEED` replaces it, set only by the `model_compare` example (`--seed`) for repeat
//! measurements.

/// Environment variable that replaces the sampler seed.
pub const SEED_ENV: &str = "MATRIX_GGUF_SEED";
const DEFAULT_SEED: u32 = 1337;

/// The sampler seed: the variable's value when it is a number, else the fixed default.
pub fn sampler_seed(value: Option<&str>) -> u32 {
    value.and_then(|v| v.trim().parse().ok()).unwrap_or(DEFAULT_SEED)
}
