-- Migration 002: Add per-podcast sync scheduling support
ALTER TABLE podcasts ADD COLUMN check_interval INTEGER;
