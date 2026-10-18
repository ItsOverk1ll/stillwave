// SPDX-License-Identifier: GPL-3.0-only
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import * as api from "./api";
import type { ScanFinished, ScanProgress, Settings, TrackRow } from "./api";

vi.mock("./api");

const mocked = vi.mocked(api);

function track(n: number, over: Partial<TrackRow> = {}): TrackRow {
  return {
    id: `id-${n.toString()}`,
    title: `Song ${n.toString()}`,
    artist: "The Band",
    album: "The Album",
    trackNumber: n,
    durationMs: 187_000,
    codec: "flac",
    sampleRateHz: 96_000,
    bitDepth: 24,
    playable: true,
    ...over,
  };
}

const settings = (over: Partial<Settings> = {}): Settings => ({
  musicFolder: "D:\\Music",
  audioMode: "exclusive",
  audioDeviceId: null,
  ...over,
});

let progressCb: ((p: ScanProgress) => void) | undefined;
let finishedCb: ((f: ScanFinished) => void) | undefined;

beforeEach(() => {
  progressCb = undefined;
  finishedCb = undefined;
  mocked.getAppInfo.mockResolvedValue({
    appName: "Stillwave",
    appVersion: "0.0.1",
    coreVersion: "0.0.1",
    schemaVersion: 3,
    dbPath: "C:\\data\\stillwave.db",
  });
  mocked.getSettings.mockResolvedValue(settings());
  mocked.listAudioDevices.mockResolvedValue([]);
  mocked.startScan.mockResolvedValue();
  mocked.cancelScan.mockResolvedValue();
  mocked.playFrom.mockResolvedValue();
  mocked.setAudioOptions.mockResolvedValue();
  mocked.playerStatus.mockResolvedValue({
    state: "stopped",
    index: null,
    positionMs: 0,
    durationMs: null,
    nowPlaying: null,
    badge: null,
    signal: null,
    error: null,
  });
  mocked.libraryPage.mockImplementation((query, offset, limit) => {
    const all = query ? [track(7, { title: "Abba match" })] : [track(1), track(2), track(3)];
    return Promise.resolve({ rows: all.slice(offset, offset + limit), total: all.length });
  });
  mocked.onScanProgress.mockImplementation((cb) => {
    progressCb = cb;
    return Promise.resolve(() => undefined);
  });
  mocked.onScanFinished.mockImplementation((cb) => {
    finishedCb = cb;
    return Promise.resolve(() => undefined);
  });
});

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

const finished = (over: Partial<ScanFinished> = {}): ScanFinished => ({
  found: 3,
  added: 3,
  updated: 0,
  unchanged: 0,
  removed: 0,
  failed: [],
  failedCount: 0,
  cancelled: false,
  error: null,
  ...over,
});

describe("first run", () => {
  it("asks for a music folder and scans the one chosen", async () => {
    mocked.getSettings.mockResolvedValue(settings({ musicFolder: null }));
    mocked.pickFolder.mockResolvedValue("E:\\Tunes");
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /choose music folder/i }));
    await waitFor(() => {
      expect(mocked.startScan).toHaveBeenCalledWith("E:\\Tunes");
    });
  });

  it("does nothing if the folder picker is cancelled", async () => {
    mocked.getSettings.mockResolvedValue(settings({ musicFolder: null }));
    mocked.pickFolder.mockResolvedValue(null);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /choose music folder/i }));
    await waitFor(() => {
      expect(mocked.pickFolder).toHaveBeenCalled();
    });
    expect(mocked.startScan).not.toHaveBeenCalled();
  });
});

describe("scanning", () => {
  it("rescans the saved folder on start and shows progress then a summary", async () => {
    render(<App />);
    await waitFor(() => {
      expect(mocked.startScan).toHaveBeenCalledWith("D:\\Music");
    });
    await waitFor(() => {
      expect(progressCb).toBeDefined();
    });
    act(() => progressCb?.({ phase: "discovering", found: 1200, done: 0, total: 0 }));
    expect(screen.getByText(/Looking for music… 1,200 files/)).toBeTruthy();

    act(() => progressCb?.({ phase: "importing", found: 10, done: 3, total: 10 }));
    expect(screen.getByText(/Reading tags… 3 of 10/)).toBeTruthy();

    const before = mocked.libraryPage.mock.calls.length;
    act(() => finishedCb?.(finished({ found: 10, added: 7 })));
    expect(screen.getByText(/Scan finished: 10 files, 7 new\./)).toBeTruthy();
    await waitFor(() => {
      expect(mocked.libraryPage.mock.calls.length).toBeGreaterThan(before); // list reloads
    });
  });

  it("learns the saved folder when a scan finishes", async () => {
    mocked.getSettings.mockResolvedValue(settings({ musicFolder: null }));
    render(<App />);
    await screen.findByRole("button", { name: /choose music folder/i });
    mocked.getSettings.mockResolvedValue(settings({ musicFolder: "D:\\Music" }));
    act(() => finishedCb?.(finished()));
    expect(await screen.findByText("Song 1")).toBeTruthy();
  });

  it("lists unreadable files without hiding the rest", async () => {
    render(<App />);
    await waitFor(() => {
      expect(finishedCb).toBeDefined();
    });
    act(() =>
      finishedCb?.(finished({ failedCount: 2, failed: ["D:\\Music\\bad.flac: not audio"] })),
    );
    expect(screen.getByText(/2 files could not be read/)).toBeTruthy();
    expect(screen.getByText(/bad\.flac/)).toBeTruthy();
  });

  it("shows a scan error as an alert", async () => {
    render(<App />);
    await waitFor(() => {
      expect(finishedCb).toBeDefined();
    });
    act(() => finishedCb?.(finished({ error: "folder vanished" })));
    expect((await screen.findByRole("alert")).textContent).toContain("folder vanished");
  });
});

describe("library", () => {
  it("shows tracks with their quality and duration", async () => {
    render(<App />);
    expect(await screen.findByText("Song 1")).toBeTruthy();
    expect(screen.getAllByText("FLAC 24/96").length).toBe(3);
    expect(screen.getAllByText("3:07").length).toBe(3);
    expect(await screen.findByText("3 tracks")).toBeTruthy();
  });

  it("searches as you type", async () => {
    render(<App />);
    await screen.findByText("Song 1");
    fireEvent.change(screen.getByRole("searchbox", { name: /search your library/i }), {
      target: { value: "abba" },
    });
    expect(await screen.findByText("Abba match")).toBeTruthy();
    expect(mocked.libraryPage).toHaveBeenCalledWith("abba", 0, 100);
    expect(screen.queryByText("Song 1")).toBeNull();
    expect(await screen.findByText("1 match")).toBeTruthy();
  });

  it("says so when nothing matches", async () => {
    mocked.libraryPage.mockResolvedValue({ rows: [], total: 0 });
    render(<App />);
    fireEvent.change(await screen.findByRole("searchbox"), { target: { value: "zzz" } });
    expect(await screen.findByText(/No tracks match your search/)).toBeTruthy();
  });

  it("plays from the chosen row within the current search", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Play Song 2" }));
    await waitFor(() => {
      expect(mocked.playFrom).toHaveBeenCalledWith("", 1);
    });
  });

  it("does not offer to play a track whose file is missing", async () => {
    mocked.libraryPage.mockResolvedValue({ rows: [track(1, { playable: false })], total: 1 });
    render(<App />);
    const play = await screen.findByRole("button", { name: "Play Song 1" });
    expect((play as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/file missing/)).toBeTruthy();
  });

  it("explains a playback failure instead of failing silently", async () => {
    mocked.playFrom.mockRejectedValue(
      "The device does not support 24-bit / 96 kHz in exclusive mode",
    );
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Play Song 1" }));
    expect((await screen.findByRole("alert")).textContent).toContain("exclusive mode");
  });
});

describe("about", () => {
  it("still shows versions, schema and database path from the Rust side", async () => {
    render(<App />);
    expect(await screen.findByText("v3")).toBeTruthy();
    expect(screen.getByText("C:\\data\\stillwave.db")).toBeTruthy();
  });

  it("explains itself when the Rust core is unreachable", async () => {
    mocked.getSettings.mockRejectedValue(new Error("no tauri"));
    render(<App />);
    expect((await screen.findByRole("alert")).textContent).toContain("no tauri");
  });
});
