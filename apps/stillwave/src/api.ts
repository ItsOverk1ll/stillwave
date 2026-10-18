// SPDX-License-Identifier: GPL-3.0-only
import { invoke } from "@tauri-apps/api/core";

/** Mirrors `AppInfo` in src-tauri/src/lib.rs. */
export interface AppInfo {
  appName: string;
  appVersion: string;
  coreVersion: string;
  schemaVersion: number;
  dbPath: string;
}

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}
