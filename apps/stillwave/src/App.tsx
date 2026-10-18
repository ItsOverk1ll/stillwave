// SPDX-License-Identifier: GPL-3.0-only
import { useEffect, useState } from "react";
import { getAppInfo, type AppInfo } from "./api";
import "./styles.css";

type State =
  { status: "loading" } | { status: "ready"; info: AppInfo } | { status: "error"; message: string };

export default function App() {
  const [state, setState] = useState<State>({ status: "loading" });

  useEffect(() => {
    let cancelled = false;
    getAppInfo().then(
      (info) => {
        if (!cancelled) setState({ status: "ready", info });
      },
      (err: unknown) => {
        if (!cancelled) setState({ status: "error", message: String(err) });
      },
    );
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <main className="home">
      <h1>Stillwave</h1>
      <p className="tagline">Your music, your files, bit-perfect.</p>

      {state.status === "loading" && <p role="status">Loading…</p>}
      {state.status === "error" && (
        <p role="alert" className="error">
          Could not reach the app core: {state.message}
        </p>
      )}
      {state.status === "ready" && (
        <dl className="info">
          <dt>App version</dt>
          <dd>{state.info.appVersion}</dd>
          <dt>Core version</dt>
          <dd>{state.info.coreVersion}</dd>
          <dt>Database schema</dt>
          <dd>v{state.info.schemaVersion}</dd>
          <dt>Database file</dt>
          <dd className="path">{state.info.dbPath}</dd>
        </dl>
      )}
      <p className="status">Pre-alpha. Playback arrives in Phase 1.</p>
    </main>
  );
}
