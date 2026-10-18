// SPDX-License-Identifier: GPL-3.0-only
import { useEffect, useRef } from "react";
import type { QueueEntry } from "./api";
import { formatDuration } from "./format";

interface Props {
  entries: QueueEntry[];
  /** Index of the entry being heard. */
  current: number | null;
  onJump: (index: number) => void;
  onClose: () => void;
}

/** What is playing now and what plays next. Click an entry to jump to it. */
export default function QueuePanel({ entries, current, onJump, onClose }: Props) {
  const currentRef = useRef<HTMLLIElement>(null);

  // Keep the playing entry in view as the queue advances.
  useEffect(() => {
    // Test environments (jsdom) have no scrollIntoView, although the DOM types say it exists.
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
    currentRef.current?.scrollIntoView?.({ block: "nearest" });
  }, [current]);

  return (
    <aside className="queue" aria-label="Play queue">
      <header>
        <h2>Queue</h2>
        <span className="muted">
          {entries.length > 0 && current != null
            ? `${(current + 1).toLocaleString()} of ${entries.length.toLocaleString()}`
            : ""}
        </span>
        <button type="button" aria-label="Close queue" onClick={onClose}>
          ✕
        </button>
      </header>
      {entries.length === 0 ? (
        <p className="empty">Nothing queued. Press play on a song, album or artist.</p>
      ) : (
        <ol>
          {entries.map((e, i) => {
            const isCurrent = i === current;
            return (
              <li
                key={`${i.toString()}-${e.itemId}`}
                ref={isCurrent ? currentRef : undefined}
                className={isCurrent ? "current" : i < (current ?? 0) ? "played" : undefined}
              >
                <button
                  type="button"
                  aria-current={isCurrent ? "true" : undefined}
                  onClick={() => {
                    onJump(i);
                  }}
                >
                  <span className="meta">
                    <span className="title">
                      {isCurrent ? "♫ " : ""}
                      {e.title}
                    </span>
                    <span className="sub">{e.artist ?? ""}</span>
                  </span>
                  <span className="time">{formatDuration(e.durationMs)}</span>
                </button>
              </li>
            );
          })}
        </ol>
      )}
    </aside>
  );
}
