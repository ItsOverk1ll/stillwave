// SPDX-License-Identifier: GPL-3.0-only
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import type { Page } from "./api";

const PAGE_SIZE = 100;
const OVERSCAN = 6;
/** Used when the browser reports no height yet (first paint, tests). */
const FALLBACK_VIEWPORT = 600;

interface Props<T> {
  /** Fetches `limit` rows from `offset`, plus the total. */
  fetchPage: (offset: number, limit: number) => Promise<Page<T>>;
  /** Fixed row height lets rows be placed by arithmetic, so huge lists stay smooth. */
  rowHeight: number;
  renderRow: (row: T, index: number) => ReactNode;
  /** Extra class for a row (for example the now-playing one). */
  rowClass?: (row: T) => string | undefined;
  onActivate?: (row: T, index: number) => void;
  label: string;
  empty: string;
  onTotal?: (total: number) => void;
  onError: (message: string) => void;
}

/**
 * A virtualised, paged list. Only rows near the viewport are in the DOM and only
 * the pages they need are fetched. Remount (change `key`) to start over, for
 * example when a search changes or a scan finishes.
 */
export default function VirtualList<T>({
  fetchPage,
  rowHeight,
  renderRow,
  rowClass,
  onActivate,
  label,
  empty,
  onTotal,
  onError,
}: Props<T>) {
  const scroller = useRef<HTMLDivElement>(null);
  const [total, setTotal] = useState<number | null>(null);
  const [pages, setPages] = useState<ReadonlyMap<number, T[]>>(new Map());
  const [scrollTop, setScrollTop] = useState(0);
  const [viewport, setViewport] = useState(FALLBACK_VIEWPORT);
  const mounted = useRef(true);
  const inFlight = useRef(new Set<number>());

  const load = useCallback(
    (page: number) => {
      if (inFlight.current.has(page)) return;
      inFlight.current.add(page);
      fetchPage(page * PAGE_SIZE, PAGE_SIZE).then(
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
    [fetchPage, onTotal, onError],
  );

  // The first page also tells us the total.
  useEffect(() => {
    mounted.current = true;
    load(0);
    return () => {
      mounted.current = false;
    };
  }, [load]);

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
  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN);
  const last = Math.min(count - 1, Math.ceil((scrollTop + viewport) / rowHeight) + OVERSCAN);

  // Fetch whichever pages the visible rows live in.
  useEffect(() => {
    if (total == null || last < first) return;
    for (let p = Math.floor(first / PAGE_SIZE); p <= Math.floor(last / PAGE_SIZE); p++) {
      if (!pages.has(p)) load(p);
    }
  }, [first, last, total, pages, load]);

  if (total === 0) {
    return (
      <p className="empty" role="status">
        {empty}
      </p>
    );
  }

  const items: ReactNode[] = [];
  for (let i = first; i <= last; i++) {
    const row = pages.get(Math.floor(i / PAGE_SIZE))?.[i % PAGE_SIZE];
    const extra = row ? rowClass?.(row) : undefined;
    items.push(
      <li
        key={i}
        className={["row", row ? extra : "loading"].filter(Boolean).join(" ")}
        style={{ top: i * rowHeight, height: rowHeight }}
        onDoubleClick={() => {
          if (row) onActivate?.(row, i);
        }}
      >
        {row ? renderRow(row, i) : null}
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
      <ul className="rows" aria-label={label} style={{ height: count * rowHeight }}>
        {items}
      </ul>
    </div>
  );
}
