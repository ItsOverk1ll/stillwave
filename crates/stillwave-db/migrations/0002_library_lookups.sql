-- SPDX-License-Identifier: GPL-3.0-only
-- Schema v2: fast find-or-create of artists and albums by name during scans.

CREATE INDEX idx_artists_name ON artists(name COLLATE NOCASE);
CREATE INDEX idx_albums_title ON albums(title COLLATE NOCASE, album_artist_id);
