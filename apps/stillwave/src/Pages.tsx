// SPDX-License-Identifier: GPL-3.0-only
import { useEffect, useState } from "react";
import * as api from "./api";
import type { AlbumDetail, ArtistDetail } from "./api";
import { AlbumChip, albumSubtitle, artistSubtitle } from "./Collections";
import { TrackRowView } from "./TrackList";

interface PageProps {
  id: string;
  currentId: string | null;
  onPlay: (context: api.PlayContext, index: number) => void;
  onOpenArtist: (id: string) => void;
  onOpenAlbum: (id: string) => void;
  onError: (message: string) => void;
}

/** Loads `load(id)` once per id; `null` while loading or after an error. */
function useDetail<T>(id: string, load: (id: string) => Promise<T>, onError: (m: string) => void) {
  const [data, setData] = useState<{ id: string; value: T } | null>(null);
  useEffect(() => {
    let alive = true;
    load(id).then(
      (value) => {
        if (alive) setData({ id, value });
      },
      (e: unknown) => {
        if (alive) onError(String(e));
      },
    );
    return () => {
      alive = false;
    };
  }, [id, load, onError]);
  return data?.id === id ? data.value : null;
}

/** An album: header, play button and its tracks in order. */
export function AlbumPage({ id, currentId, onPlay, onOpenArtist, onError }: PageProps) {
  const detail = useDetail<AlbumDetail>(id, api.albumDetail, onError);
  if (!detail) return <p className="empty">Loading album…</p>;
  const { album, tracks } = detail;
  const context: api.PlayContext = { kind: "album", id };
  const firstPlayable = tracks.findIndex((t) => t.playable);
  return (
    <section className="detail" aria-label={`Album ${album.title}`}>
      <header className="detail-head">
        <div>
          <h2>{album.title}</h2>
          <p className="sub">
            {album.artist && album.artistId ? (
              <button
                type="button"
                className="link"
                onClick={() => {
                  onOpenArtist(album.artistId ?? "");
                }}
              >
                {album.artist}
              </button>
            ) : (
              album.artist
            )}
            {album.artist ? " · " : ""}
            {albumSubtitle(album, false)}
          </p>
        </div>
        <AlbumChip album={album} />
        <button
          type="button"
          className="primary"
          disabled={firstPlayable < 0}
          onClick={() => {
            onPlay(context, firstPlayable);
          }}
        >
          Play album
        </button>
      </header>
      <ol className="tracklist" aria-label="Tracks">
        {tracks.map((t, i) => (
          <li
            key={t.id}
            className={t.id === currentId ? "row static current" : "row static"}
            onDoubleClick={() => {
              if (t.playable) onPlay(context, i);
            }}
          >
            <TrackRowView
              // Only name the artist when it differs from the album's (compilations).
              track={{ ...t, album: null, artist: t.artistId === album.artistId ? null : t.artist }}
              current={t.id === currentId}
              showNumber
              onPlay={() => {
                onPlay(context, i);
              }}
              onOpenArtist={t.artistId !== album.artistId ? onOpenArtist : undefined}
            />
          </li>
        ))}
      </ol>
    </section>
  );
}

/** An artist: their albums, albums they appear on, and play-all. */
export function ArtistPage({ id, onPlay, onOpenAlbum, onError }: PageProps) {
  const detail = useDetail<ArtistDetail>(id, api.artistDetail, onError);
  if (!detail) return <p className="empty">Loading artist…</p>;
  const { artist, albums } = detail;
  const own = albums.filter((a) => !a.appearsOn);
  const appears = albums.filter((a) => a.appearsOn);
  const section = (title: string, list: typeof albums) =>
    list.length > 0 && (
      <>
        <h3>{title}</h3>
        <ul className="tracklist" aria-label={title}>
          {list.map((a) => (
            <li key={a.id} className="row static">
              <button
                type="button"
                className="row-button"
                onClick={() => {
                  onOpenAlbum(a.id);
                }}
              >
                <span className="meta">
                  <span className="title">{a.title}</span>
                  <span className="sub">{albumSubtitle(a, a.appearsOn)}</span>
                </span>
                <AlbumChip album={a} />
              </button>
            </li>
          ))}
        </ul>
      </>
    );
  return (
    <section className="detail" aria-label={`Artist ${artist.name}`}>
      <header className="detail-head">
        <div>
          <h2>{artist.name}</h2>
          <p className="sub">{artistSubtitle(artist)}</p>
        </div>
        <button
          type="button"
          className="primary"
          disabled={artist.trackCount === 0}
          onClick={() => {
            onPlay({ kind: "artist", id }, 0);
          }}
        >
          Play all
        </button>
      </header>
      {section("Albums", own)}
      {section("Appears on", appears)}
    </section>
  );
}
