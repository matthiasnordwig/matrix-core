//! Tests der ONNX-Eingabegrenze: Sie wirkt ohne Modelldatei, ein kleiner Tokenizer genügt.

use std::collections::HashMap;

use tokenizers::models::wordlevel::WordLevel;
use tokenizers::pre_tokenizers::whitespace::Whitespace;
use tokenizers::Tokenizer;

use super::onnx::{cap_input_tokens, MAX_INPUT_TOKENS};

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
    cap_input_tokens(&mut tokenizer).unwrap();
    let encoded = tokenizer.encode("a ".repeat(MAX_INPUT_TOKENS + 500), false).unwrap();
    assert_eq!(encoded.get_ids().len(), MAX_INPUT_TOKENS);
}

#[test]
fn short_input_stays_whole() {
    let mut tokenizer = word_tokenizer();
    cap_input_tokens(&mut tokenizer).unwrap();
    let encoded = tokenizer.encode("a a a", false).unwrap();
    assert_eq!(encoded.get_ids().len(), 3);
}
