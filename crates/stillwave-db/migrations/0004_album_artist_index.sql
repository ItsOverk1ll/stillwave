-- SPDX-License-Identifier: GPL-3.0-only
-- Schema v4: artist pages list an artist's albums; index the album-artist link.

CREATE INDEX idx_albums_artist ON albums(album_artist_id);
