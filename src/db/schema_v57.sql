-- Chat-transcript metadata (bubble UI): per-turn provenance + answer payload.
--
-- Assistant turns now record WHICH model served them (`model` = the resolved
-- endpoint's model_id — with pools this can differ per turn), the reasoning
-- effort in use, and `answer_json` — the serialized {sources, citations} of the
-- answer, so a resumed session can re-render clickable [n] citations and the
-- sources list for historic turns (previously only the plain answer text
-- survived). All NULL for user turns and for rows persisted before v57.
ALTER TABLE chat_messages ADD COLUMN model TEXT;
ALTER TABLE chat_messages ADD COLUMN reasoning_effort TEXT;
ALTER TABLE chat_messages ADD COLUMN answer_json TEXT;
