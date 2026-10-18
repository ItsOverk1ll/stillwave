-- SPDX-License-Identifier: GPL-3.0-only
-- Schema v5: an indexed album sort key (album artist, then title) so the albums
-- list pages from an index instead of re-sorting every album on every page.
-- Kept current by triggers, so no code path can forget it.

ALTER TABLE albums ADD COLUMN sort_key TEXT;

UPDATE albums SET sort_key = lower(
    COALESCE((SELECT name FROM artists WHERE id = albums.album_artist_id), '')
    || char(1) || title);

CREATE INDEX idx_albums_sort ON albums(sort_key);

CREATE TRIGGER albums_sort_ai AFTER INSERT ON albums BEGIN
    UPDATE albums SET sort_key = lower(
        COALESCE((SELECT name FROM artists WHERE id = new.album_artist_id), '')
        || char(1) || new.title)
     WHERE id = new.id;
END;

CREATE TRIGGER albums_sort_au AFTER UPDATE OF title, album_artist_id ON albums BEGIN
    UPDATE albums SET sort_key = lower(
        COALESCE((SELECT name FROM artists WHERE id = new.album_artist_id), '')
        || char(1) || new.title)
     WHERE id = new.id;
END;

CREATE TRIGGER artists_sort_au AFTER UPDATE OF name ON artists BEGIN
    UPDATE albums SET sort_key = lower(new.name || char(1) || title)
     WHERE album_artist_id = new.id;
END;
