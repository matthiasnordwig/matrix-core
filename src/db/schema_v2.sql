-- Matrix Rust Core — migration v2
-- Pre-chunk window is now measured in (estimated) TOKENS, not sentences.

ALTER TABLE chunking_profiles RENAME COLUMN window_sentences TO window_tokens;

-- Lift implausibly small legacy values (e.g. the old default of 5 sentences)
-- to a sensible token budget so existing profiles don't produce thousands of
-- tiny windows.
UPDATE chunking_profiles SET window_tokens = 1500 WHERE window_tokens < 100;
