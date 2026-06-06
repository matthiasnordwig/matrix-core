-- schema_v7.sql
-- Add chunking_strategy to contexts table
ALTER TABLE contexts ADD COLUMN chunking_strategy TEXT NOT NULL DEFAULT 'prompt';
