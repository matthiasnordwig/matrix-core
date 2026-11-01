//! Tests der ONNX-Eingabegrenze: Sie wirkt ohne Modelldatei, ein kleiner Tokenizer genügt.

use std::collections::HashMap;

use tokenizers::models::wordlevel::WordLevel;
use tokenizers::pre_tokenizers::whitespace::Whitespace;
use tokenizers::Tokenizer;

use super::onnx::{cap_input_tokens, input_token_limit};

fn word_tokenizer() -> Tokenizer {
    let vocab: HashMap<String, u32> = [("a".to_string(), 0), ("<unk>".to_string(), 1)].into_iter().collect();
    let model = WordLevel::builder().vocab(vocab).unk_token("<unk>".to_string()).build().unwrap();
    let mut tokenizer = Tokenizer::new(model);
    tokenizer.with_pre_tokenizer(Some(Whitespace {}));
    tokenizer
}

#[test]
fn long_input_is_cut_to_the_limit() {
    let mut tokenizer = word_tokenizer();
    cap_input_tokens(&mut tokenizer, 2_048).unwrap();
    let encoded = tokenizer.encode("a ".repeat(2_548), false).unwrap();
    assert_eq!(encoded.get_ids().len(), 2_048);
}

#[test]
fn short_input_stays_whole() {
    let mut tokenizer = word_tokenizer();
    cap_input_tokens(&mut tokenizer, 2_048).unwrap();
    let encoded = tokenizer.encode("a a a", false).unwrap();
    assert_eq!(encoded.get_ids().len(), 3);
}

#[test]
fn the_input_limit_follows_the_memory_of_the_device() {
    let gb = |n: u64| Some(n << 30);
    assert_eq!(input_token_limit(gb(6)), 2_048);
    assert_eq!(input_token_limit(gb(8)), 2_048);
    assert_eq!(input_token_limit(gb(12)), 4_096);
    assert_eq!(input_token_limit(gb(16)), 4_096);
    assert_eq!(input_token_limit(gb(24)), 8_192);
    assert_eq!(input_token_limit(None), 4_096);
}
