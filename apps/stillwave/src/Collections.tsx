// SPDX-License-Identifier: GPL-3.0-only
import { useCallback } from "react";
import * as api from "./api";
import type { AlbumView, ArtistView } from "./api";
import { albumQuality, formatDuration, plural } from "./format";
import VirtualList from "./VirtualList";

const ROW_HEIGHT = 56;

interface ListProps {
  onError: (message: string) => void;
}

/** All albums, by album artist then title. */
export function AlbumList({ onOpen, onError }: ListProps & { onOpen: (id: string) => void }) {
  const fetchPage = useCallback(
    (offset: number, limit: number) => api.albumsPage(offset, limit),
    [],
  );
  return (
    <VirtualList<AlbumView>
      fetchPage={fetchPage}
      rowHeight={ROW_HEIGHT}
      label="Albums"
      empty="No albums yet. Albums come from your files' album tags."
      onError={onError}
      onActivate={(a) => {
        onOpen(a.id);
      }}
      renderRow={(a) => (
        <button
          type="button"
          className="row-button"
          onClick={() => {
            onOpen(a.id);
          }}
        >
          <span className="meta">
            <span className="title">{a.title}</span>
            <span className="sub">{albumSubtitle(a)}</span>
          </span>
          <AlbumChip album={a} />
        </button>
      )}
    />
  );
}

/** All artists with music, by name. */
export function ArtistList({ onOpen, onError }: ListProps & { onOpen: (id: string) => void }) {
  const fetchPage = useCallback(
    (offset: number, limit: number) => api.artistsPage(offset, limit),
    [],
  );
  return (
    <VirtualList<ArtistView>
      fetchPage={fetchPage}
      rowHeight={ROW_HEIGHT}
      label="Artists"
      empty="No artists yet. Artists come from your files' artist tags."
      onError={onError}
      onActivate={(a) => {
        onOpen(a.id);
      }}
      renderRow={(a) => (
        <button
          type="button"
          className="row-button"
          onClick={() => {
            onOpen(a.id);
          }}
        >
          <span className="meta">
            <span className="title">{a.name}</span>
            <span className="sub">{artistSubtitle(a)}</span>
          </span>
        </button>
      )}
    />
  );
}

export function albumSubtitle(a: AlbumView, withArtist = true): string {
  return [
    withArtist ? a.artist : null,
    a.year?.toString(),
    plural(a.trackCount, "track"),
    formatDuration(a.durationMs),
  ]
    .filter(Boolean)
    .join(" · ");
}

export function artistSubtitle(a: ArtistView): string {
  return [a.albumCount > 0 ? plural(a.albumCount, "album") : null, plural(a.trackCount, "track")]
    .filter(Boolean)
    .join(" · ");
}

export function AlbumChip({ album }: { album: AlbumView }) {
  const q = albumQuality(album);
  return q ? <span className="chip">{q}</span> : null;
}
