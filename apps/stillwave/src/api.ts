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
  artistId: string | null;
  album: string | null;
  albumId: string | null;
  trackNumber: number | null;
  durationMs: number | null;
  codec: string | null;
  sampleRateHz: number | null;
  bitDepth: number | null;
  /** False when the file is missing from this device. */
  playable: boolean;
  /** False for a playlist or Liked Songs entry that is not in the library. */
  inLibrary: boolean;
}

export interface Page<T> {
  rows: T[];
  total: number;
}
export type LibraryPage = Page<TrackRow>;

export interface AlbumView {
  id: string;
  title: string;
  artist: string | null;
  artistId: string | null;
  year: number | null;
  trackCount: number;
  durationMs: number;
  /** Set when every track shares one codec. */
  codec: string | null;
  maxSampleRateHz: number | null;
  maxBitDepth: number | null;
}

export interface ArtistView {
  id: string;
  name: string;
  albumCount: number;
  trackCount: number;
}

export interface AlbumDetail {
  album: AlbumView;
  tracks: TrackRow[];
}

export interface ArtistDetail {
  artist: ArtistView;
  /** `appearsOn`: credited to someone else but includes this artist. */
  albums: (AlbumView & { appearsOn: boolean })[];
}

/** What a play starts from; decides what goes in the queue. */
export type PlayContext =
  | { kind: "library"; query: string }
  | { kind: "album"; id: string }
  | { kind: "artist"; id: string }
  | { kind: "playlist"; id: string }
  | { kind: "liked" };

export interface QueueEntry {
  itemId: string;
  title: string;
  artist: string | null;
  album: string | null;
  durationMs: number | null;
}

export interface QueueView {
  version: number;
  entries: QueueEntry[];
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
  nowPlaying: QueueEntry | null;
  badge: Badge | null;
  signal: SignalView | null;
  error: string | null;
  /** Changes when the queue is replaced; refetch `playQueue` then. */
  queueVersion: number;
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

export const albumsPage = (offset: number, limit: number): Promise<Page<AlbumView>> =>
  invoke<Page<AlbumView>>("albums_page", { offset, limit });
export const artistsPage = (offset: number, limit: number): Promise<Page<ArtistView>> =>
  invoke<Page<ArtistView>>("artists_page", { offset, limit });
export const albumDetail = (id: string): Promise<AlbumDetail> =>
  invoke<AlbumDetail>("album_detail", { id });
export const artistDetail = (id: string): Promise<ArtistDetail> =>
  invoke<ArtistDetail>("artist_detail", { id });

/** Plays `context` from its row `index`. */
export const play = (context: PlayContext, index: number): Promise<void> =>
  invoke("play", { context, index });
export const playQueue = (): Promise<QueueView> => invoke<QueueView>("play_queue");
export const jumpTo = (index: number): Promise<void> => invoke("jump_to", { index });
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

// ---- playlists and Spotify import ---------------------------------------------

export interface PlaylistView {
  id: string;
  name: string;
  description: string | null;
  entryCount: number;
  /** Entries that are in the library. */
  ownedCount: number;
}

export interface PlaylistsView {
  playlists: PlaylistView[];
  likedCount: number;
  likedOwned: number;
}

export interface PlaylistDetail {
  playlist: PlaylistView;
  entries: TrackRow[];
}

export interface ImportResult {
  playlists: { id: string; name: string; total: number; matched: number }[];
  likedTotal: number | null;
  likedMatched: number | null;
  /** Podcast episodes and audiobooks, which are not imported. */
  skipped: number;
  problems: string[];
}

export const playlists = (): Promise<PlaylistsView> => invoke<PlaylistsView>("playlists");
export const playlistDetail = (id: string): Promise<PlaylistDetail> =>
  invoke<PlaylistDetail>("playlist_detail", { id });
export const likedDetail = (): Promise<TrackRow[]> => invoke<TrackRow[]>("liked_detail");
export const importSpotify = (paths: string[]): Promise<ImportResult> =>
  invoke<ImportResult>("import_spotify", { paths });

/** The unzipped Spotify data download (the folder holding Playlist1.json). */
export async function pickSpotifyFolder(): Promise<string | null> {
  const chosen = await open({
    directory: true,
    multiple: false,
    title: "Choose your unzipped Spotify data folder",
  });
  return typeof chosen === "string" ? chosen : null;
}

/** CSV playlist exports, or individual Playlist*.json / YourLibrary.json files. */
export async function pickSpotifyFiles(): Promise<string[]> {
  const chosen = await open({
    directory: false,
    multiple: true,
    title: "Choose Spotify playlist files",
    filters: [{ name: "Playlist exports", extensions: ["csv", "json"] }],
  });
  if (chosen == null) return [];
  return Array.isArray(chosen) ? chosen : [chosen];
}
