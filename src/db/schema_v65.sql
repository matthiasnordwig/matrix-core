-- schema_v65: image projector of a local vision model.
--
--   * llm_endpoints.mmproj_path — path of the `mmproj-*.gguf` file that lets a
--     local GGUF chat model (provider `gguf`, model file in `base_url`) read
--     pictures through llama.cpp's multimodal library. NULL = text only. Only
--     with it may a `gguf` endpoint carry `supports_vision`. Re-pointed at
--     startup like `base_url` when the app's folders moved.
--
-- Plain ALTER TABLE ADD COLUMN, runs through db/mod.rs::migrate's default arm.

ALTER TABLE llm_endpoints ADD COLUMN mmproj_path TEXT NULL;
