// SPDX-License-Identifier: GPL-3.0-only
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

/** Mirrors `AppInfo` in src-tauri/src/lib.rs. */
export interface AppInfo {
  appName: string;
  appVersion: string;
  coreVersion: string;
  schemaVersion: number;
  dbPath: string;
}

export type AudioMode = "exclusive" | "exclusive_fallback" | "shared";

export interface Settings {
  musicFolder: string | null;
  audioMode: AudioMode;
  audioDeviceId: string | null;
}

export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
}

export interface TrackRow {
  id: string;
  title: string;
  artist: string | null;
  album: string | null;
  trackNumber: number | null;
  durationMs: number | null;
  codec: string | null;
  sampleRateHz: number | null;
  bitDepth: number | null;
  /** False when the file is missing from this device. */
  playable: boolean;
}

export interface LibraryPage {
  rows: TrackRow[];
  total: number;
}

export interface ScanProgress {
  phase: "discovering" | "importing";
  found: number;
  done: number;
  total: number;
}

export interface ScanFinished {
  found: number;
  added: number;
  updated: number;
  unchanged: number;
  removed: number;
  failed: string[];
  failedCount: number;
  cancelled: boolean;
  error: string | null;
}

export type Badge = "bit_perfect" | "processed" | "lossy";

export interface SignalView {
  source: string;
  output: string;
  exclusive: boolean;
  resampled: boolean;
  dsp: boolean;
  dithered: boolean;
  softwareVolume: boolean;
  lossy: boolean;
}

export interface PlayerView {
  state: "stopped" | "playing" | "paused" | "finished" | "error";
  index: number | null;
  positionMs: number;
  durationMs: number | null;
  nowPlaying: { title: string; artist: string | null; album: string | null } | null;
  badge: Badge | null;
  signal: SignalView | null;
  error: string | null;
}

export const getAppInfo = (): Promise<AppInfo> => invoke<AppInfo>("app_info");
export const getSettings = (): Promise<Settings> => invoke<Settings>("get_settings");
export const listAudioDevices = (): Promise<AudioDevice[]> =>
  invoke<AudioDevice[]>("list_audio_devices");
export const setAudioOptions = (mode: AudioMode, deviceId: string | null): Promise<void> =>
  invoke("set_audio_options", { mode, deviceId });

/** Opens the system folder picker. Resolves null if the user cancels. */
export async function pickFolder(): Promise<string | null> {
  const chosen = await open({
    directory: true,
    multiple: false,
    title: "Choose your music folder",
  });
  return typeof chosen === "string" ? chosen : null;
}

export const startScan = (path: string): Promise<void> => invoke("start_scan", { path });
export const cancelScan = (): Promise<void> => invoke("cancel_scan");

export const libraryPage = (query: string, offset: number, limit: number): Promise<LibraryPage> =>
  invoke<LibraryPage>("library_page", { query, offset, limit });

export const playFrom = (query: string, index: number): Promise<void> =>
  invoke("play_from", { query, index });
export const pause = (): Promise<void> => invoke("pause");
export const resume = (): Promise<void> => invoke("resume");
export const stop = (): Promise<void> => invoke("stop");
export const nextTrack = (): Promise<void> => invoke("next_track");
export const seek = (positionMs: number): Promise<void> => invoke("seek", { positionMs });
export const playerStatus = (): Promise<PlayerView> => invoke<PlayerView>("player_status");

export const onScanProgress = (cb: (p: ScanProgress) => void): Promise<UnlistenFn> =>
  listen<ScanProgress>("scan-progress", (e) => {
    cb(e.payload);
  });
export const onScanFinished = (cb: (f: ScanFinished) => void): Promise<UnlistenFn> =>
  listen<ScanFinished>("scan-finished", (e) => {
    cb(e.payload);
  });
