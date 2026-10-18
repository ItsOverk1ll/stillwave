// SPDX-License-Identifier: GPL-3.0-only
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "./api";
import type { PlayerView, SignalView } from "./api";
import AudioSettings from "./AudioSettings";
import { explainSignal, formatDuration, formatQuality } from "./format";
import PlayerBar from "./PlayerBar";

vi.mock("./api");
const mocked = vi.mocked(api);

const signal = (over: Partial<SignalView> = {}): SignalView => ({
  source: "24-bit / 96 kHz / 2 ch",
  output: "24-bit / 96 kHz / 2 ch",
  exclusive: true,
  resampled: false,
  dsp: false,
  dithered: false,
  softwareVolume: false,
  lossy: false,
  ...over,
});

const playing = (over: Partial<PlayerView> = {}): PlayerView => ({
  state: "playing",
  index: 0,
  positionMs: 65_000,
  durationMs: 187_000,
  nowPlaying: {
    title: "Roygbiv",
    artist: "Boards of Canada",
    album: "Music Has the Right to Children",
  },
  badge: "bit_perfect",
  signal: signal(),
  error: null,
  ...over,
});

beforeEach(() => {
  mocked.pause.mockResolvedValue();
  mocked.resume.mockResolvedValue();
  mocked.stop.mockResolvedValue();
  mocked.nextTrack.mockResolvedValue();
  mocked.seek.mockResolvedValue();
});

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

describe("PlayerBar", () => {
  it("shows what is playing, its position and a Bit-perfect badge", async () => {
    mocked.playerStatus.mockResolvedValue(playing());
    render(<PlayerBar onError={vi.fn()} />);
    expect(await screen.findByText("Roygbiv")).toBeTruthy();
    expect(screen.getByText(/Boards of Canada/)).toBeTruthy();
    expect(screen.getByText("1:05")).toBeTruthy();
    expect(screen.getByText("3:07")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Bit-perfect" })).toBeTruthy();
  });

  it("opens the signal path and says why a path is not bit-perfect", async () => {
    mocked.playerStatus.mockResolvedValue(
      playing({ badge: "processed", signal: signal({ exclusive: false, softwareVolume: true }) }),
    );
    render(<PlayerBar onError={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Processed" }));
    const region = screen.getByRole("region", { name: "Signal path" });
    expect(region.textContent).toContain("Windows mixer");
    expect(region.textContent).toContain("24-bit / 96 kHz / 2 ch");
  });

  it("labels lossy sources honestly", async () => {
    mocked.playerStatus.mockResolvedValue(
      playing({ badge: "lossy", signal: signal({ lossy: true }) }),
    );
    render(<PlayerBar onError={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Lossy source" }));
    expect(screen.getByRole("region", { name: "Signal path" }).textContent).toContain(
      "lossy compression",
    );
  });

  it("pauses, stops and skips", async () => {
    mocked.playerStatus.mockResolvedValue(playing());
    render(<PlayerBar onError={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Pause" }));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    fireEvent.click(screen.getByRole("button", { name: "Next track" }));
    await waitFor(() => {
      expect(mocked.pause).toHaveBeenCalled();
      expect(mocked.stop).toHaveBeenCalled();
      expect(mocked.nextTrack).toHaveBeenCalled();
    });
  });

  it("resumes when paused", async () => {
    mocked.playerStatus.mockResolvedValue(playing({ state: "paused" }));
    render(<PlayerBar onError={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Play" }));
    await waitFor(() => {
      expect(mocked.resume).toHaveBeenCalled();
    });
  });

  it("seeks to where the slider is released", async () => {
    mocked.playerStatus.mockResolvedValue(playing());
    render(<PlayerBar onError={vi.fn()} />);
    const slider = await screen.findByRole("slider", { name: "Position" });
    fireEvent.change(slider, { target: { value: "120000" } });
    fireEvent.pointerUp(slider);
    await waitFor(() => {
      expect(mocked.seek).toHaveBeenCalledWith(120_000);
    });
  });

  it("is quiet when nothing is playing, and shows a failure", async () => {
    mocked.playerStatus.mockResolvedValue({
      state: "stopped",
      index: null,
      positionMs: 0,
      durationMs: null,
      nowPlaying: null,
      badge: null,
      signal: null,
      error: "device unplugged",
    });
    render(<PlayerBar onError={vi.fn()} />);
    expect(await screen.findByText("Nothing playing.")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("device unplugged");
    expect(screen.queryByRole("slider")).toBeNull();
  });
});

describe("AudioSettings", () => {
  beforeEach(() => {
    mocked.getSettings.mockResolvedValue({
      musicFolder: null,
      audioMode: "exclusive",
      audioDeviceId: null,
    });
    mocked.listAudioDevices.mockResolvedValue([
      { id: "a", name: "Speakers (USB DAC)", isDefault: true },
      { id: "b", name: "HDMI Audio", isDefault: false },
    ]);
    mocked.setAudioOptions.mockResolvedValue();
  });

  it("lists devices and saves a different one", async () => {
    render(<AudioSettings onError={vi.fn()} />);
    fireEvent.click(screen.getByText("Audio output"));
    expect(await screen.findByText(/Speakers \(USB DAC\) \(default\)/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Device"), { target: { value: "b" } });
    await waitFor(() => {
      expect(mocked.setAudioOptions).toHaveBeenCalledWith("exclusive", "b");
    });
  });

  it("explains each mode and saves the choice", async () => {
    render(<AudioSettings onError={vi.fn()} />);
    expect(await screen.findByText(/Bit-perfect only/)).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Mode"), { target: { value: "shared" } });
    expect(screen.getByText(/never bit-perfect/)).toBeTruthy();
    await waitFor(() => {
      expect(mocked.setAudioOptions).toHaveBeenCalledWith("shared", null);
    });
  });
});

describe("formatting", () => {
  it("formats durations and qualities", () => {
    expect(formatDuration(187_000)).toBe("3:07");
    expect(formatDuration(5_000)).toBe("0:05");
    expect(formatDuration(null)).toBe("");
    expect(formatQuality({ codec: "flac", bitDepth: 24, sampleRateHz: 96_000 })).toBe("FLAC 24/96");
    expect(formatQuality({ codec: "mp3", bitDepth: null, sampleRateHz: 44_100 })).toBe("MP3 44.1");
    expect(formatQuality({ codec: null, bitDepth: null, sampleRateHz: null })).toBe("");
  });

  it("gives a reason for every non-bit-perfect badge", () => {
    expect(explainSignal("processed", signal({ exclusive: false }))[0]).toMatch(/Windows mixer/);
    expect(explainSignal("processed", signal({ resampled: true }))).toContain(
      "The sample rate is being converted.",
    );
    expect(explainSignal("lossy", signal({ lossy: true }))[0]).toMatch(/lossy/);
    expect(explainSignal("bit_perfect", signal())[0]).toMatch(/exactly as stored/);
  });
});
