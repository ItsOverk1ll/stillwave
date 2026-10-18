// SPDX-License-Identifier: GPL-3.0-only
import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "./api";
import type { AppInfo, ScanFinished, ScanProgress, Settings } from "./api";
import AudioSettings from "./AudioSettings";
import PlayerBar from "./PlayerBar";
import TrackList from "./TrackList";
import "./styles.css";

/** Wait this long after the last keystroke before searching. */
const SEARCH_DELAY_MS = 120;

interface ScanState {
  running: boolean;
  progress: ScanProgress | null;
  result: ScanFinished | null;
}

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const [query, setQuery] = useState("");
  const [version, setVersion] = useState(0);
  const [total, setTotal] = useState<number | null>(null);
  const [scan, setScan] = useState<ScanState>({ running: false, progress: null, result: null });
  const startedRescan = useRef(false);
  // What was last played, so it can be retried after changing audio settings.
  const lastPlay = useRef<{ query: string; index: number } | null>(null);
  // Bumped when settings change elsewhere so the settings panel reloads them.
  const [settingsKey, setSettingsKey] = useState(0);

  const report = useCallback((message: string) => {
    setError(message);
  }, []);

  const beginScan = useCallback(
    (path: string) => {
      setScan({ running: true, progress: null, result: null });
      api.startScan(path).catch((e: unknown) => {
        setScan({ running: false, progress: null, result: null });
        report(String(e));
      });
    },
    [report],
  );

  // Startup: settings and about info; then catch up with changes on disk.
  useEffect(() => {
    api.getAppInfo().then(setInfo, () => undefined);
    api.getSettings().then(
      (s) => {
        setSettings(s);
        if (s.musicFolder && !startedRescan.current) {
          startedRescan.current = true;
          beginScan(s.musicFolder); // incremental: unchanged files are skipped
        }
      },
      (e: unknown) => {
        report(`Could not reach the app core: ${String(e)}`);
      },
    );
  }, [beginScan, report]);

  // Scan events from Rust.
  useEffect(() => {
    const unlisten = Promise.all([
      api.onScanProgress((progress) => {
        setScan((s) => ({ ...s, running: true, progress }));
      }),
      api.onScanFinished((result) => {
        setScan({ running: false, progress: null, result });
        setVersion((v) => v + 1);
        if (result.error) report(result.error);
        // The scan remembers its folder; pick that up whoever started the scan.
        api.getSettings().then(setSettings, () => undefined);
      }),
    ]);
    return () => {
      void unlisten.then((fns) => {
        fns.forEach((f) => {
          f();
        });
      });
    };
  }, [report]);

  // Debounce the search box.
  useEffect(() => {
    const id = setTimeout(() => {
      setQuery(typed);
    }, SEARCH_DELAY_MS);
    return () => {
      clearTimeout(id);
    };
  }, [typed]);

  const chooseFolder = () => {
    api.pickFolder().then(
      (path) => {
        if (!path) return;
        setSettings((s) => (s ? { ...s, musicFolder: path } : s));
        beginScan(path);
      },
      (e: unknown) => {
        report(String(e));
      },
    );
  };

  // The device refused exclusive mode: allow the mixer for it and try again.
  const switchToMixer = () => {
    api
      .getSettings()
      .then((s) => api.setAudioOptions("exclusive_fallback", s.audioDeviceId))
      .then(() => {
        setSettingsKey((k) => k + 1);
        const last = lastPlay.current;
        return last ? api.playFrom(last.query, last.index) : undefined;
      })
      .catch((e: unknown) => {
        report(String(e));
      });
  };

  const folder = settings?.musicFolder ?? null;
  const hasLibrary = total !== null && total > 0;

  return (
    <div className="app">
      <header className="top">
        <h1>Stillwave</h1>
        <div className="folder">
          {folder ? (
            <>
              <span className="muted" title={folder}>
                {folder}
              </span>
              <button
                type="button"
                disabled={scan.running}
                onClick={() => {
                  beginScan(folder);
                }}
              >
                Rescan
              </button>
              <button type="button" disabled={scan.running} onClick={chooseFolder}>
                Change folder
              </button>
            </>
          ) : null}
          {scan.running && (
            <button
              type="button"
              onClick={() => {
                api.cancelScan().catch(() => undefined);
              }}
            >
              Cancel
            </button>
          )}
        </div>
      </header>

      {error && (
        <div className="banner" role="alert">
          <span>{error}</span>
          <button
            type="button"
            aria-label="Dismiss"
            onClick={() => {
              setError(null);
            }}
          >
            ✕
          </button>
        </div>
      )}

      <ScanStatus scan={scan} />

      {!folder && settings && (
        <section className="welcome">
          <h2>Add your music</h2>
          <p>
            Choose the folder where your music lives. Stillwave reads your files, never changes
            them, and keeps everything on this computer.
          </p>
          <button type="button" className="primary" onClick={chooseFolder}>
            Choose music folder…
          </button>
        </section>
      )}

      {folder && (
        <main className="library">
          <div className="searchbar">
            <input
              type="search"
              aria-label="Search your library"
              placeholder="Search title, artist or album"
              value={typed}
              onChange={(e) => {
                setTyped(e.target.value);
              }}
            />
            {total !== null && (
              <span className="muted">
                {query.trim() ? plural(total, "match", "matches") : plural(total, "track")}
              </span>
            )}
          </div>
          {(hasLibrary || total === null || query.trim() !== "" || !scan.running) && (
            <TrackList
              key={`${query}\u0000${version.toString()}`}
              query={query}
              onTotal={setTotal}
              onError={report}
              onPlay={(index) => {
                lastPlay.current = { query, index };
                api.playFrom(query, index).catch((e: unknown) => {
                  report(String(e));
                });
              }}
            />
          )}
        </main>
      )}

      <AudioSettings key={settingsKey} onError={report} />
      <PlayerBar onError={report} onUseMixer={switchToMixer} />

      <details className="about">
        <summary>About</summary>
        {info && (
          <dl className="info">
            <dt>App version</dt>
            <dd>{info.appVersion}</dd>
            <dt>Core version</dt>
            <dd>{info.coreVersion}</dd>
            <dt>Database schema</dt>
            <dd>v{info.schemaVersion}</dd>
            <dt>Database file</dt>
            <dd className="dbpath">{info.dbPath}</dd>
          </dl>
        )}
        <p className="muted">
          Pre-alpha. Nothing is uploaded; your library stays on this computer.
        </p>
      </details>
    </div>
  );
}

/** 1 file, 2 files. Pass `many` for irregular plurals (match, matches). */
function plural(n: number, noun: string, many = `${noun}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? noun : many}`;
}

function ScanStatus({ scan }: { scan: ScanState }) {
  if (scan.running) {
    const p = scan.progress;
    return (
      <div className="scan" role="status">
        {p?.phase === "importing" ? (
          <>
            <span>
              Reading tags… {p.done.toLocaleString()} of {p.total.toLocaleString()}
            </span>
            <progress max={p.total} value={p.done} />
          </>
        ) : (
          <>
            <span>Looking for music… {plural(p?.found ?? 0, "file")}</span>
            <progress />
          </>
        )}
      </div>
    );
  }
  const r = scan.result;
  if (!r || r.error) return null;
  const parts = [
    plural(r.found, "file"),
    r.added > 0 ? `${r.added.toLocaleString()} new` : null,
    r.updated > 0 ? `${r.updated.toLocaleString()} updated` : null,
    r.removed > 0 ? `${r.removed.toLocaleString()} removed` : null,
  ].filter(Boolean);
  return (
    <div className="scan done" role="status">
      <span>
        {r.cancelled ? "Scan cancelled. " : "Scan finished: "}
        {parts.join(", ")}.
      </span>
      {r.failedCount > 0 && (
        <details>
          <summary>{plural(r.failedCount, "file")} could not be read</summary>
          <ul>
            {r.failed.map((f) => (
              <li key={f}>{f}</li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
