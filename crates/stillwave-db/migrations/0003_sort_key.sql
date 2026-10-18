-- SPDX-License-Identifier: GPL-3.0-only
-- Schema v3: a precomputed, indexed sort key so a 50,000-track library list can
-- be paged without sorting every row on every page. Maintained by the importer;
-- items without one sort first.

ALTER TABLE media_items ADD COLUMN sort_key TEXT;
CREATE INDEX idx_media_items_sort ON media_items(sort_key);
