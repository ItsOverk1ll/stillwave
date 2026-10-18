// SPDX-License-Identifier: GPL-3.0-only
import { useCallback, useEffect, useRef, useState } from "react";
import { libraryPage, type TrackRow } from "./api";
import { formatDuration, formatQuality } from "./format";

/** Fixed row height lets the list place rows by arithmetic: 50,000 rows stay smooth. */
export const ROW_HEIGHT = 52;
const PAGE_SIZE = 100;
const OVERSCAN = 6;
/** Used when the browser reports no height yet (first paint, tests). */
const FALLBACK_VIEWPORT = 600;

interface Props {
  /** Remount (change the React `key`) when the query or library changes: state starts fresh. */
  query: string;
  onPlay: (index: number) => void;
  onTotal?: (total: number) => void;
  onError: (message: string) => void;
}

/**
 * A virtualised, paged list of the library. Only the rows near the viewport exist
 * in the DOM and only the pages they need are fetched from Rust.
 */
export default function TrackList({ query, onPlay, onTotal, onError }: Props) {
  const scroller = useRef<HTMLDivElement>(null);
  const [total, setTotal] = useState<number | null>(null);
  const [pages, setPages] = useState<ReadonlyMap<number, TrackRow[]>>(new Map());
  const [scrollTop, setScrollTop] = useState(0);
  const [viewport, setViewport] = useState(FALLBACK_VIEWPORT);
  const mounted = useRef(true);
  const inFlight = useRef(new Set<number>());

  const fetchPage = useCallback(
    (page: number) => {
      if (inFlight.current.has(page)) return;
      inFlight.current.add(page);
      libraryPage(query, page * PAGE_SIZE, PAGE_SIZE).then(
        (result) => {
          if (!mounted.current) return;
          inFlight.current.delete(page);
          setTotal(result.total);
          onTotal?.(result.total);
          setPages((prev) => new Map(prev).set(page, result.rows));
        },
        (err: unknown) => {
          if (!mounted.current) return;
          inFlight.current.delete(page);
          onError(String(err));
        },
      );
    },
    [query, onTotal, onError],
  );

  // The first page also tells us the total.
  useEffect(() => {
    mounted.current = true;
    fetchPage(0);
    return () => {
      mounted.current = false;
    };
  }, [fetchPage]);

  useEffect(() => {
    const el = scroller.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => {
      setViewport(el.clientHeight || FALLBACK_VIEWPORT);
    });
    ro.observe(el); // reports the size straight away
    return () => {
      ro.disconnect();
    };
  }, []);

  const count = total ?? 0;
  const first = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const last = Math.min(count - 1, Math.ceil((scrollTop + viewport) / ROW_HEIGHT) + OVERSCAN);

  // Fetch whichever pages the visible rows live in.
  useEffect(() => {
    if (total == null) return;
    for (
      let p = Math.floor(first / PAGE_SIZE);
      p <= Math.floor(Math.max(first, last) / PAGE_SIZE);
      p++
    ) {
      if (!pages.has(p) && last >= first) fetchPage(p);
    }
  }, [first, last, total, pages, fetchPage]);

  if (total === 0) {
    return (
      <p className="empty" role="status">
        {query.trim() ? "No tracks match your search." : "No tracks yet."}
      </p>
    );
  }

  const rows: React.ReactNode[] = [];
  for (let i = first; i <= last; i++) {
    const track = pages.get(Math.floor(i / PAGE_SIZE))?.[i % PAGE_SIZE];
    rows.push(
      <li
        key={i}
        className={track ? "row" : "row loading"}
        style={{ top: i * ROW_HEIGHT, height: ROW_HEIGHT }}
        onDoubleClick={() => {
          if (track?.playable) onPlay(i);
        }}
      >
        {track ? (
          <Row
            track={track}
            onPlay={() => {
              onPlay(i);
            }}
          />
        ) : null}
      </li>,
    );
  }

  return (
    <div
      className="scroller"
      ref={scroller}
      onScroll={(e) => {
        setScrollTop(e.currentTarget.scrollTop);
      }}
    >
      <ul className="rows" aria-label="Library" style={{ height: count * ROW_HEIGHT }}>
        {rows}
      </ul>
    </div>
  );
}

function Row({ track, onPlay }: { track: TrackRow; onPlay: () => void }) {
  const sub = [track.artist, track.album].filter(Boolean).join(" · ");
  const quality = formatQuality(track);
  return (
    <>
      <button
        type="button"
        className="play"
        disabled={!track.playable}
        aria-label={`Play ${track.title}`}
        title={track.playable ? "Play from here" : "File not found on this device"}
        onClick={onPlay}
      >
        ▶
      </button>
      <div className="meta">
        <div className="title">{track.title}</div>
        <div className="sub">{track.playable ? sub : `${sub} · file missing`}</div>
      </div>
      {quality && <span className="chip">{quality}</span>}
      <span className="time">{formatDuration(track.durationMs)}</span>
    </>
  );
}
