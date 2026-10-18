// SPDX-License-Identifier: GPL-3.0-only
import type { AlbumView, Badge, SignalView, TrackRow } from "./api";

/** 187000 -> "3:07". */
export function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return "";
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m.toString()}:${s.toString().padStart(2, "0")}`;
}

/** "FLAC 24/96" style chip for the list. Empty when nothing is known. */
export function formatQuality(t: Pick<TrackRow, "codec" | "bitDepth" | "sampleRateHz">): string {
  if (!t.codec) return "";
  const rate = t.sampleRateHz == null ? "" : trimRate(t.sampleRateHz);
  const depth = t.bitDepth == null ? "" : `${t.bitDepth.toString()}/`;
  const detail = rate ? ` ${depth}${rate}` : "";
  return `${t.codec.toUpperCase()}${detail}`;
}

function trimRate(hz: number): string {
  const k = hz / 1000;
  return Number.isInteger(k) ? k.toString() : k.toFixed(1);
}

export const BADGE_LABEL: Record<Badge, string> = {
  bit_perfect: "Bit-perfect",
  processed: "Processed",
  lossy: "Lossy source",
};

/**
 * Plain-language reasons for a badge, so it is never just a colour. A bit-perfect
 * path has none to give beyond what it is; the others say what is in the way.
 */
export function explainSignal(badge: Badge, s: SignalView): string[] {
  if (badge === "bit_perfect") {
    return [
      "Samples reach the device exactly as stored in the file: no resampling, EQ, dither or volume change.",
    ];
  }
  const reasons: string[] = [];
  if (s.lossy) reasons.push("The file uses lossy compression, so it cannot be bit-perfect.");
  if (!s.exclusive)
    reasons.push("Playing through the Windows mixer, which may change volume or format.");
  if (s.resampled) reasons.push("The sample rate is being converted.");
  if (s.dsp) reasons.push("Audio effects are on.");
  if (s.dithered) reasons.push("Dither is applied while reducing bit depth.");
  if (s.softwareVolume && s.exclusive) reasons.push("Volume is applied in software.");
  return reasons.length > 0 ? reasons : ["The output format differs from the file."];
}

/** 1 file, 2 files. Pass `many` for irregular plurals (match, matches). */
export function plural(n: number, noun: string, many = `${noun}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? noun : many}`;
}

/** "FLAC 24/96" for an album (its best file), or "Mixed" when codecs differ. */
export function albumQuality(a: AlbumView): string {
  if (a.trackCount === 0) return "";
  if (!a.codec) return "Mixed";
  return formatQuality({
    codec: a.codec,
    bitDepth: a.maxBitDepth,
    sampleRateHz: a.maxSampleRateHz,
  });
}
