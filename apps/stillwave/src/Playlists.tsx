// SPDX-License-Identifier: GPL-3.0-only
import { useCallback, useEffect, useState } from "react";
import * as api from "./api";
import type { ImportResult, PlaylistsView, TrackRow } from "./api";
import { formatDuration, plural } from "./format";
import { TrackRowView } from "./TrackList";

/** "12 songs · 9 in your library" */
function ownership(total: number, owned: number): string {
  if (total === 0) return "Empty";
  if (owned === total) return plural(total, "song");
  return `${plural(total, "song")} · ${owned.toLocaleString()} in your library`;
}

interface ListProps {
  onOpenPlaylist: (id: string) => void;
  onOpenLiked: () => void;
  onError: (message: string) => void;
}

/** Liked Songs, every playlist, and the way in from Spotify. */
export function PlaylistList({ onOpenPlaylist, onOpenLiked, onError }: ListProps) {
  const [data, setData] = useState<PlaylistsView | null>(null);
  const [importing, setImporting] = useState(false);

  const load = useCallback(() => {
    api.playlists().then(setData, (e: unknown) => {
      onError(String(e));
    });
  }, [onError]);
  useEffect(load, [load]);

  if (!data) return <p className="empty">Loading playlists…</p>;
  const nothing = data.playlists.length === 0 && data.likedCount === 0;

  return (
    <section className="detail" aria-label="Playlists">
      <header className="detail-head">
        <div>
          <h2>Playlists</h2>
          <p className="sub">{nothing ? "None yet." : plural(data.playlists.length, "playlist")}</p>
        </div>
        <button
          type="button"
          className={nothing ? "primary" : undefined}
          aria-expanded={importing}
          onClick={() => {
            setImporting((v) => !v);
          }}
        >
          Import from Spotify…
        </button>
      </header>

      {importing && (
        <SpotifyImport
          onDone={() => {
            load();
          }}
          onClose={() => {
            setImporting(false);
          }}
          onError={onError}
        />
      )}

      <ul className="tracklist" aria-label="Playlist list">
        {data.likedCount > 0 && (
          <li className="row static">
            <button type="button" className="row-button" onClick={onOpenLiked}>
              <span className="meta">
                <span className="title">♥ Liked Songs</span>
                <span className="sub">{ownership(data.likedCount, data.likedOwned)}</span>
              </span>
            </button>
          </li>
        )}
        {data.playlists.map((p) => (
          <li key={p.id} className="row static">
            <button
              type="button"
              className="row-button"
              onClick={() => {
                onOpenPlaylist(p.id);
              }}
            >
              <span className="meta">
                <span className="title">{p.name}</span>
                <span className="sub">{ownership(p.entryCount, p.ownedCount)}</span>
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

interface ImportProps {
  onDone: (result: ImportResult) => void;
  onClose: () => void;
  onError: (message: string) => void;
}

/** Explains where the files come from, picks them, imports, and says how it went. */
export function SpotifyImport({ onDone, onClose, onError }: ImportProps) {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ImportResult | null>(null);

  const run = (paths: Promise<string[]>) => {
    paths
      .then((chosen) => {
        if (chosen.length === 0) return;
        setBusy(true);
        return api.importSpotify(chosen).then((r) => {
          setResult(r);
          onDone(r);
        });
      })
      .catch((e: unknown) => {
        onError(String(e));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  return (
    <div className="import" role="region" aria-label="Import from Spotify">
      {result ? (
        <ImportReport result={result} />
      ) : (
        <>
          <p>
            Stillwave reads playlists from files you download from Spotify. It never signs in to
            Spotify and never downloads music. Songs you own are matched to your files; the rest
            stay in the playlist as “not in your library” and fill in when you add them.
          </p>
          <ol className="steps">
            <li>
              On spotify.com, open Account → Privacy settings → <em>Download your data</em> and
              request <em>Account data</em>. Spotify emails a link, usually within a few days.
            </li>
            <li>Unzip the download, then choose the unzipped folder here.</li>
          </ol>
          <p className="muted">
            Already have CSV playlist exports (for example from Exportify)? Choose those files
            instead: they import straight away and match more precisely.
          </p>
        </>
      )}
      <div className="actions">
        <button
          type="button"
          className="primary"
          disabled={busy}
          onClick={() => {
            run(api.pickSpotifyFolder().then((f) => (f ? [f] : [])));
          }}
        >
          {busy ? "Importing…" : "Choose Spotify data folder…"}
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={() => {
            run(api.pickSpotifyFiles());
          }}
        >
          Choose CSV or JSON files…
        </button>
        <button type="button" onClick={onClose}>
          {result ? "Done" : "Cancel"}
        </button>
      </div>
    </div>
  );
}

function ImportReport({ result }: { result: ImportResult }) {
  const owned = result.playlists.reduce((n, p) => n + p.matched, 0);
  const total = result.playlists.reduce((n, p) => n + p.total, 0);
  return (
    <div role="status">
      <p>
        <strong>Imported {plural(result.playlists.length, "playlist")}</strong>
        {total > 0 && `: ${owned.toLocaleString()} of ${plural(total, "song")} are in your library`}
        {result.likedTotal != null &&
          `; Liked Songs: ${(result.likedMatched ?? 0).toLocaleString()} of ${result.likedTotal.toLocaleString()}`}
        .
      </p>
      {result.skipped > 0 && (
        <p className="muted">
          {plural(result.skipped, "podcast episode or audiobook", "podcast episodes or audiobooks")}{" "}
          left out.
        </p>
      )}
      {result.problems.length > 0 && (
        <details>
          <summary>{plural(result.problems.length, "file")} could not be read</summary>
          <ul>
            {result.problems.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}

interface PageProps {
  /** A playlist ID, or `null` for Liked Songs. */
  id: string | null;
  currentId: string | null;
  onPlay: (context: api.PlayContext, index: number) => void;
  onOpenArtist: (id: string) => void;
  onOpenAlbum: (id: string) => void;
  onError: (message: string) => void;
}

/** A playlist (or Liked Songs): its songs in order, owned or not. */
export function PlaylistPage({
  id,
  currentId,
  onPlay,
  onOpenArtist,
  onOpenAlbum,
  onError,
}: PageProps) {
  const [data, setData] = useState<{
    name: string;
    description: string | null;
    entries: TrackRow[];
  } | null>(null);

  useEffect(() => {
    let alive = true;
    const loaded = id
      ? api.playlistDetail(id).then((d) => ({
          name: d.playlist.name,
          description: d.playlist.description,
          entries: d.entries,
        }))
      : api.likedDetail().then((entries) => ({ name: "Liked Songs", description: null, entries }));
    loaded.then(
      (d) => {
        if (alive) setData(d);
      },
      (e: unknown) => {
        if (alive) onError(String(e));
      },
    );
    return () => {
      alive = false;
    };
  }, [id, onError]);

  if (!data) return <p className="empty">Loading playlist…</p>;
  const context: api.PlayContext = id ? { kind: "playlist", id } : { kind: "liked" };
  const owned = data.entries.filter((t) => t.inLibrary).length;
  const firstPlayable = data.entries.findIndex((t) => t.playable);
  const length = data.entries.reduce((ms, t) => ms + (t.durationMs ?? 0), 0);

  return (
    <section className="detail" aria-label={`Playlist ${data.name}`}>
      <header className="detail-head">
        <div>
          <h2>{data.name}</h2>
          <p className="sub">
            {[ownership(data.entries.length, owned), length > 0 ? formatDuration(length) : null]
              .filter(Boolean)
              .join(" · ")}
          </p>
          {data.description && <p className="muted">{data.description}</p>}
        </div>
        <button
          type="button"
          className="primary"
          disabled={firstPlayable < 0}
          onClick={() => {
            onPlay(context, firstPlayable);
          }}
        >
          Play
        </button>
      </header>
      {data.entries.length === 0 ? (
        <p className="empty">This playlist is empty.</p>
      ) : (
        <ol className="tracklist" aria-label="Songs">
          {data.entries.map((t, i) => (
            <li
              key={`${i.toString()}-${t.id}`}
              className={[
                "row static",
                t.id === currentId ? "current" : "",
                t.inLibrary ? "" : "missing",
              ]
                .filter(Boolean)
                .join(" ")}
              onDoubleClick={() => {
                if (t.playable) onPlay(context, i);
              }}
            >
              <TrackRowView
                track={t}
                current={t.id === currentId}
                onPlay={() => {
                  onPlay(context, i);
                }}
                onOpenArtist={onOpenArtist}
                onOpenAlbum={onOpenAlbum}
              />
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
