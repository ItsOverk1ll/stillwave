-- SPDX-License-Identifier: GPL-3.0-only
-- Schema v1. All IDs are 16-byte UUIDv7 blobs. Every user-data change also
-- writes a change_log row (done by the repository layer, same transaction).

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE artists (
    id           BLOB PRIMARY KEY,
    name         TEXT NOT NULL,
    sort_name    TEXT,
    external_ids TEXT NOT NULL DEFAULT '{}'
) WITHOUT ROWID;

CREATE TABLE albums (
    id              BLOB PRIMARY KEY,
    title           TEXT NOT NULL,
    album_artist_id BLOB REFERENCES artists(id) ON DELETE SET NULL,
    year            INTEGER,
    external_ids    TEXT NOT NULL DEFAULT '{}'
) WITHOUT ROWID;

-- Rowid table on purpose: the FTS index shares its rowid.
CREATE TABLE media_items (
    id           BLOB NOT NULL UNIQUE,
    kind         TEXT NOT NULL,
    title        TEXT NOT NULL,
    artist_id    BLOB REFERENCES artists(id) ON DELETE SET NULL,
    album_id     BLOB REFERENCES albums(id) ON DELETE SET NULL,
    track_number INTEGER,
    disc_number  INTEGER,
    duration_ms  INTEGER,
    isrc         TEXT,
    external_ids TEXT NOT NULL DEFAULT '{}'
);
CREATE INDEX idx_media_items_artist ON media_items(artist_id);
CREATE INDEX idx_media_items_album  ON media_items(album_id);
CREATE INDEX idx_media_items_isrc   ON media_items(isrc) WHERE isrc IS NOT NULL;

CREATE TABLE media_files (
    id               BLOB PRIMARY KEY,
    media_item_id    BLOB NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    device_id        BLOB NOT NULL,
    path             TEXT NOT NULL,
    size_bytes       INTEGER NOT NULL,
    modified_unix_ms INTEGER NOT NULL,
    container        TEXT NOT NULL,
    codec            TEXT NOT NULL,
    sample_rate_hz   INTEGER NOT NULL,
    bit_depth        INTEGER,
    channels         INTEGER NOT NULL,
    duration_ms      INTEGER NOT NULL,
    UNIQUE (device_id, path)
) WITHOUT ROWID;
CREATE INDEX idx_media_files_item ON media_files(media_item_id);

-- A song the user does not own (yet), created by imports.
CREATE TABLE placeholders (
    id           BLOB PRIMARY KEY,
    title        TEXT NOT NULL,
    artist       TEXT NOT NULL,
    album        TEXT,
    duration_ms  INTEGER,
    isrc         TEXT,
    external_ids TEXT NOT NULL DEFAULT '{}',
    resolved_to  BLOB REFERENCES media_items(id) ON DELETE SET NULL
) WITHOUT ROWID;
CREATE INDEX idx_placeholders_isrc ON placeholders(isrc) WHERE isrc IS NOT NULL;

CREATE TABLE playlists (
    id          BLOB PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT
) WITHOUT ROWID;

-- An entry points at exactly one of: a library item or a placeholder.
CREATE TABLE playlist_entries (
    id            BLOB PRIMARY KEY,
    playlist_id   BLOB NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    media_item_id BLOB REFERENCES media_items(id) ON DELETE CASCADE,
    placeholder_id BLOB REFERENCES placeholders(id) ON DELETE CASCADE,
    position      TEXT NOT NULL,
    CHECK ((media_item_id IS NULL) <> (placeholder_id IS NULL))
) WITHOUT ROWID;
CREATE INDEX idx_playlist_entries_order ON playlist_entries(playlist_id, position);

CREATE TABLE plays (
    id               BLOB PRIMARY KEY,
    media_item_id    BLOB NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    device_id        BLOB NOT NULL,
    started_unix_ms  INTEGER NOT NULL,
    played_ms        INTEGER NOT NULL,
    completed        INTEGER NOT NULL
) WITHOUT ROWID;
CREATE INDEX idx_plays_item ON plays(media_item_id);
CREATE INDEX idx_plays_time ON plays(started_unix_ms);

CREATE TABLE likes (
    id             BLOB PRIMARY KEY,
    media_item_id  BLOB REFERENCES media_items(id) ON DELETE CASCADE,
    placeholder_id BLOB REFERENCES placeholders(id) ON DELETE CASCADE,
    liked_at       TEXT NOT NULL,
    CHECK ((media_item_id IS NULL) <> (placeholder_id IS NULL))
) WITHOUT ROWID;
CREATE UNIQUE INDEX idx_likes_item        ON likes(media_item_id)  WHERE media_item_id IS NOT NULL;
CREATE UNIQUE INDEX idx_likes_placeholder ON likes(placeholder_id) WHERE placeholder_id IS NOT NULL;

-- Append-only record of every user-data change, for future multi-device sync.
CREATE TABLE change_log (
    seq            INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_type    TEXT NOT NULL,
    entity_id      BLOB NOT NULL,
    op             TEXT NOT NULL CHECK (op IN ('insert', 'update', 'delete')),
    changed_fields TEXT NOT NULL DEFAULT '[]',
    hlc            TEXT NOT NULL,
    device_id      BLOB NOT NULL
);
CREATE INDEX idx_change_log_hlc ON change_log(hlc);
CREATE INDEX idx_change_log_entity ON change_log(entity_type, entity_id);

-- Search-as-you-type index over title, artist and album. Prefix indexes make
-- "beat" -> "beatles" fast. Rowid mirrors media_items.rowid.
CREATE VIRTUAL TABLE search_index USING fts5(
    title, artist, album,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3 4'
);

CREATE TRIGGER media_items_ai AFTER INSERT ON media_items BEGIN
    INSERT INTO search_index(rowid, title, artist, album) VALUES (
        new.rowid, new.title,
        COALESCE((SELECT name  FROM artists WHERE id = new.artist_id), ''),
        COALESCE((SELECT title FROM albums  WHERE id = new.album_id), ''));
END;

CREATE TRIGGER media_items_ad AFTER DELETE ON media_items BEGIN
    DELETE FROM search_index WHERE rowid = old.rowid;
END;

CREATE TRIGGER media_items_au AFTER UPDATE OF title, artist_id, album_id ON media_items BEGIN
    DELETE FROM search_index WHERE rowid = old.rowid;
    INSERT INTO search_index(rowid, title, artist, album) VALUES (
        new.rowid, new.title,
        COALESCE((SELECT name  FROM artists WHERE id = new.artist_id), ''),
        COALESCE((SELECT title FROM albums  WHERE id = new.album_id), ''));
END;

CREATE TRIGGER artists_au AFTER UPDATE OF name ON artists BEGIN
    UPDATE search_index SET artist = new.name
     WHERE rowid IN (SELECT rowid FROM media_items WHERE artist_id = new.id);
END;

CREATE TRIGGER albums_au AFTER UPDATE OF title ON albums BEGIN
    UPDATE search_index SET album = new.title
     WHERE rowid IN (SELECT rowid FROM media_items WHERE album_id = new.id);
END;
