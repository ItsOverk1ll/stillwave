// SPDX-License-Identifier: GPL-3.0-only
import { useEffect, useState } from "react";
import * as api from "./api";
import type { PlayerView, QueueEntry } from "./api";

const POLL_MS = 250;

export interface PlayerState {
  view: PlayerView | null;
  /** The current queue; refetched only when its version changes. */
  queue: QueueEntry[];
}

/**
 * One shared view of playback for the whole UI: the bar, the lists (now-playing
 * highlight) and the queue panel all read the same status, so they never disagree.
 */
export function usePlayer(): PlayerState {
  const [view, setView] = useState<PlayerView | null>(null);
  const [queue, setQueue] = useState<{ version: number; entries: QueueEntry[] }>({
    version: -1,
    entries: [],
  });

  useEffect(() => {
    let alive = true;
    const tick = () => {
      api.playerStatus().then(
        (v) => {
          if (alive) setView(v);
        },
        () => undefined,
      );
    };
    tick();
    const id = setInterval(tick, POLL_MS);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  const wanted = view?.queueVersion;
  useEffect(() => {
    if (wanted === undefined || wanted === queue.version) return;
    let alive = true;
    api.playQueue().then(
      (q) => {
        if (alive) setQueue(q);
      },
      () => undefined,
    );
    return () => {
      alive = false;
    };
  }, [wanted, queue.version]);

  return { view, queue: queue.entries };
}

/** The item being heard, if any (for highlighting it in lists). */
export function currentItemId(view: PlayerView | null): string | null {
  if (!view || (view.state !== "playing" && view.state !== "paused")) return null;
  return view.nowPlaying?.itemId ?? null;
}
