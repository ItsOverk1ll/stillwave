-- SPDX-License-Identifier: GPL-3.0-only
-- Schema v6: imported playlists remember where they came from, so importing the
-- same export again updates them instead of making copies; placeholders can be
-- found by their Spotify URI for the same reason.

ALTER TABLE playlists ADD COLUMN source_key TEXT;
CREATE UNIQUE INDEX idx_playlists_source ON playlists(source_key) WHERE source_key IS NOT NULL;

CREATE INDEX idx_placeholders_spotify
    ON placeholders(json_extract(external_ids, '$.spotify'))
    WHERE json_extract(external_ids, '$.spotify') IS NOT NULL;

CREATE INDEX idx_placeholders_unresolved ON placeholders(resolved_to) WHERE resolved_to IS NULL;
