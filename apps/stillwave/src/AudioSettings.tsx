// SPDX-License-Identifier: GPL-3.0-only
import { useEffect, useState } from "react";
import * as api from "./api";
import type { AudioDevice, AudioMode } from "./api";

const MODE_HELP: Record<AudioMode, string> = {
  exclusive:
    "Bit-perfect only. The device takes the exact format of each file; if it cannot, playback stops with an error.",
  exclusive_fallback:
    "Bit-perfect when the device allows it, otherwise through the Windows mixer (shown as Processed).",
  shared: "Always through the Windows mixer. Convenient, but never bit-perfect.",
};

interface Props {
  onError: (message: string) => void;
}

/** Choose the output device and how strictly to keep audio untouched. */
export default function AudioSettings({ onError }: Props) {
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  const [mode, setMode] = useState<AudioMode>("exclusive");
  const [deviceId, setDeviceId] = useState<string>("");

  useEffect(() => {
    Promise.all([api.getSettings(), api.listAudioDevices()]).then(
      ([s, d]) => {
        setMode(s.audioMode);
        setDeviceId(s.audioDeviceId ?? "");
        setDevices(d);
      },
      (e: unknown) => {
        onError(String(e));
      },
    );
  }, [onError]);

  const save = (m: AudioMode, d: string) => {
    setMode(m);
    setDeviceId(d);
    api.setAudioOptions(m, d === "" ? null : d).catch((e: unknown) => {
      onError(String(e));
    });
  };

  return (
    <details className="settings">
      <summary>Audio output</summary>
      <div className="field">
        <label htmlFor="audio-device">Device</label>
        <select
          id="audio-device"
          value={deviceId}
          onChange={(e) => {
            save(mode, e.target.value);
          }}
        >
          <option value="">System default</option>
          {devices.map((d) => (
            <option key={d.id} value={d.id}>
              {d.name}
              {d.isDefault ? " (default)" : ""}
            </option>
          ))}
        </select>
      </div>
      <div className="field">
        <label htmlFor="audio-mode">Mode</label>
        <select
          id="audio-mode"
          value={mode}
          onChange={(e) => {
            save(e.target.value as AudioMode, deviceId);
          }}
        >
          <option value="exclusive">Exclusive (bit-perfect)</option>
          <option value="exclusive_fallback">Exclusive, mixer if refused</option>
          <option value="shared">Windows mixer</option>
        </select>
        <p className="hint">{MODE_HELP[mode]}</p>
      </div>
    </details>
  );
}
