// SPDX-License-Identifier: GPL-3.0-only
import { useCallback } from "react";
import { libraryPage, type TrackRow } from "./api";
import { formatDuration, formatQuality } from "./format";
import VirtualList from "./VirtualList";

export const ROW_HEIGHT = 52;

interface Props {
  /** Remount (change the React `key`) when the query or library changes. */
  query: string;
  /** The item being heard, highlighted in the list. */
  currentId: string | null;
  onPlay: (index: number) => void;
  onOpenArtist: (id: string) => void;
  onOpenAlbum: (id: string) => void;
  onTotal?: (total: number) => void;
  onError: (message: string) => void;
}

/** The songs list: every track, or the tracks matching a search. */
export default function TrackList({
  query,
  currentId,
  onPlay,
  onOpenArtist,
  onOpenAlbum,
  onTotal,
  onError,
}: Props) {
  const fetchPage = useCallback(
    (offset: number, limit: number) => libraryPage(query, offset, limit),
    [query],
  );
  return (
    <VirtualList<TrackRow>
      fetchPage={fetchPage}
      rowHeight={ROW_HEIGHT}
      label="Library"
      empty={query.trim() ? "No tracks match your search." : "No tracks yet."}
      onTotal={onTotal}
      onError={onError}
      rowClass={(t) => (t.id === currentId ? "current" : undefined)}
      onActivate={(t, i) => {
        if (t.playable) onPlay(i);
      }}
      renderRow={(t, i) => (
        <TrackRowView
          track={t}
          current={t.id === currentId}
          onPlay={() => {
            onPlay(i);
          }}
          onOpenArtist={onOpenArtist}
          onOpenAlbum={onOpenAlbum}
        />
      )}
    />
  );
}

interface RowProps {
  track: TrackRow;
  current: boolean;
  onPlay: () => void;
  onOpenArtist?: (id: string) => void;
  onOpenAlbum?: (id: string) => void;
  /** Album pages show the track number instead of the album name. */
  showNumber?: boolean;
}

/** One track row; shared by the songs list, album pages and artist pages. */
export function TrackRowView({
  track,
  current,
  onPlay,
  onOpenArtist,
  onOpenAlbum,
  showNumber = false,
}: RowProps) {
  const quality = formatQuality(track);
  return (
    <>
      <button
        type="button"
        className="play"
        disabled={!track.playable}
        aria-label={`Play ${track.title}`}
        title={
          track.playable
            ? "Play from here"
            : track.inLibrary
              ? "File not found on this device"
              : "Not in your library"
        }
        onClick={onPlay}
      >
        {current ? "♫" : showNumber && track.trackNumber != null ? track.trackNumber : "▶"}
      </button>
      <div className="meta">
        <div className="title" aria-current={current ? "true" : undefined}>
          {track.title}
          {current && <span className="sr-only"> (now playing)</span>}
        </div>
        <div className="sub">
          <Link id={track.artistId} text={track.artist} onOpen={onOpenArtist} />
          {track.album && !showNumber && (
            <>
              {track.artist ? " · " : ""}
              <Link id={track.albumId} text={track.album} onOpen={onOpenAlbum} />
            </>
          )}
          {!track.inLibrary ? " · not in your library" : !track.playable && " · file missing"}
        </div>
      </div>
      {quality && <span className="chip">{quality}</span>}
      <span className="time">{formatDuration(track.durationMs)}</span>
    </>
  );
}

/** A name that opens its page when there is one to open. */
function Link({
  id,
  text,
  onOpen,
}: {
  id: string | null;
  text: string | null;
  onOpen?: (id: string) => void;
}) {
  if (!text) return null;
  if (!id || !onOpen) return <span>{text}</span>;
  return (
    <button
      type="button"
      className="link"
      onClick={(e) => {
        e.stopPropagation();
        onOpen(id);
      }}
    >
      {text}
    </button>
  );
}
