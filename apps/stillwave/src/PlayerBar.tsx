// SPDX-License-Identifier: GPL-3.0-only
import { useState } from "react";
import * as api from "./api";
import type { PlayerView } from "./api";
import { BADGE_LABEL, explainSignal, formatDuration } from "./format";

interface Props {
  /** Current playback status (polled by the caller). */
  view: PlayerView | null;
  queueOpen?: boolean;
  onToggleQueue?: () => void;
  onError: (message: string) => void;
  /** Offered when a device refused exclusive mode: switch to mixer fallback and retry. */
  onUseMixer?: () => void;
}

/** True for the error a device gives when it will not take the format in exclusive mode. */
export function isExclusiveRefusal(error: string | null): boolean {
  return error != null && /in exclusive mode/i.test(error);
}

/** Now-playing bar: transport controls, position, and the signal-path badge. */
export default function PlayerBar({ view, queueOpen, onToggleQueue, onError, onUseMixer }: Props) {
  // While the user drags the slider we show their value, not the live position.
  const [dragMs, setDragMs] = useState<number | null>(null);
  const [showPath, setShowPath] = useState(false);

  const run = (p: Promise<unknown>) => {
    p.catch((e: unknown) => {
      onError(String(e));
    });
  };

  const nothingLoaded =
    !view ||
    view.state === "stopped" ||
    view.state === "finished" ||
    (view.state === "error" && !view.nowPlaying);
  if (nothingLoaded) {
    const error = view?.error ?? null;
    return (
      <footer className="player idle" aria-label="Player">
        <span className="muted">
          {view?.state === "finished"
            ? "Finished."
            : view?.state === "error"
              ? "Playback stopped."
              : "Nothing playing."}
        </span>
        {error && (
          <span role="alert" className="error">
            {error}
          </span>
        )}
        {onUseMixer && isExclusiveRefusal(error) && (
          <div className="fix">
            <button type="button" onClick={onUseMixer}>
              Use the Windows mixer when a device refuses
            </button>
            <span className="muted">
              It will play, labelled Processed rather than Bit-perfect. You can change this under
              Audio output.
            </span>
          </div>
        )}
      </footer>
    );
  }

  const playing = view.state === "playing";
  const duration = view.durationMs ?? 0;
  const position = dragMs ?? view.positionMs;
  const np = view.nowPlaying;

  return (
    <footer className="player" aria-label="Player">
      <div className="now">
        <div className="title">{np?.title ?? "Unknown track"}</div>
        <div className="sub">{[np?.artist, np?.album].filter(Boolean).join(" · ")}</div>
      </div>

      <div className="transport">
        <button
          type="button"
          aria-label={playing ? "Pause" : "Play"}
          onClick={() => {
            run(playing ? api.pause() : api.resume());
          }}
        >
          {playing ? "❚❚" : "▶"}
        </button>
        <button
          type="button"
          aria-label="Stop"
          onClick={() => {
            run(api.stop());
          }}
        >
          ■
        </button>
        <button
          type="button"
          aria-label="Next track"
          onClick={() => {
            run(api.nextTrack());
          }}
        >
          ⏭
        </button>
        {onToggleQueue && (
          <button
            type="button"
            aria-label="Queue"
            aria-pressed={queueOpen ?? false}
            onClick={onToggleQueue}
          >
            ☰
          </button>
        )}
      </div>

      <div className="seek">
        <span className="time">{formatDuration(position)}</span>
        <input
          type="range"
          aria-label="Position"
          min={0}
          max={Math.max(duration, 1)}
          step={1000}
          value={Math.min(position, Math.max(duration, 1))}
          disabled={duration === 0}
          onChange={(e) => {
            setDragMs(Number(e.target.value));
          }}
          onPointerUp={() => {
            if (dragMs != null) run(api.seek(dragMs));
            setDragMs(null);
          }}
          onKeyUp={() => {
            if (dragMs != null) run(api.seek(dragMs));
            setDragMs(null);
          }}
        />
        <span className="time">{formatDuration(duration)}</span>
      </div>

      {view.badge && view.signal && (
        <div className="path">
          <button
            type="button"
            className={`badge ${view.badge}`}
            aria-expanded={showPath}
            onClick={() => {
              setShowPath((s) => !s);
            }}
          >
            {BADGE_LABEL[view.badge]}
          </button>
          {showPath && (
            <div className="path-details" role="region" aria-label="Signal path">
              <div>
                <strong>File</strong> {view.signal.source}
              </div>
              <div>
                <strong>Output</strong> {view.signal.output} ·{" "}
                {view.signal.exclusive ? "exclusive" : "Windows mixer"}
              </div>
              <ul>
                {explainSignal(view.badge, view.signal).map((r) => (
                  <li key={r}>{r}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}

      {view.error && (
        <span role="alert" className="error">
          {view.error}
        </span>
      )}
    </footer>
  );
}
