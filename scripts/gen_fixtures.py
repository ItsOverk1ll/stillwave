#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
"""Generate reference audio for bit-perfect and gapless tests.

Output goes to fixtures/generated/ (git-ignored). WAV is written with the
standard library only. FLAC and ALAC (.m4a) are encoded from the WAVs with
ffmpeg when it is on PATH; otherwise they are skipped with a notice.

Files per format (16/44.1, 24/96, 24/192), all stereo:
  sweep-<fmt>.wav     10 s logarithmic sine sweep 20 Hz -> 20 kHz, -6 dBFS
  gapless-a-<fmt>.wav 2 s: silence for 1 s, then a 1 kHz tone to the last sample
  gapless-b-<fmt>.wav 2 s: 1 kHz tone from the first sample (phase-continuous with a)
Playing a then b with no gap must give a continuous tone with no silent frames.
"""
import math
import shutil
import struct
import subprocess
import sys
import wave
from pathlib import Path

FORMATS = [(44_100, 16), (96_000, 24), (192_000, 24)]
OUT = Path(__file__).resolve().parent.parent / "fixtures" / "generated"
AMP = 0.5  # -6 dBFS


def pack(samples, bits):
    full = (1 << (bits - 1)) - 1
    out = bytearray()
    for s in samples:
        v = max(-full - 1, min(full, round(s * full)))
        out += v.to_bytes(bits // 8, "little", signed=True)
    return bytes(out)


def write_wav(path, rate, bits, mono):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(bits // 8)
        w.setframerate(rate)
        stereo = []
        for s in mono:
            stereo += (s, s)
        w.writeframes(pack(stereo, bits))


def sweep(rate, seconds=10, f0=20.0, f1=20_000.0):
    n = rate * seconds
    k = math.log(f1 / f0)
    return [
        AMP * math.sin(2 * math.pi * f0 * seconds / k * (math.exp(i / n * k) - 1))
        for i in range(n)
    ]


def tone(rate, frames, start):
    return [AMP * math.sin(2 * math.pi * 1000 * (start + i) / rate) for i in range(frames)]


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        print("note: ffmpeg not found; generating WAV only (no FLAC/ALAC)", file=sys.stderr)
    for rate, bits in FORMATS:
        tag = f"{bits}-{rate // 1000}k"
        half = rate  # 1 s
        a = [0.0] * half + tone(rate, half, 0)
        b = tone(rate, 2 * half, half)
        wavs = {
            f"sweep-{tag}": sweep(rate),
            f"gapless-a-{tag}": a,
            f"gapless-b-{tag}": b,
        }
        for name, mono in wavs.items():
            wav = OUT / f"{name}.wav"
            write_wav(wav, rate, bits, mono)
            if ffmpeg:
                sample_fmt = "s16" if bits == 16 else "s32"
                flac_fmt = "s16" if bits == 16 else "s32"
                subprocess.run([ffmpeg, "-y", "-loglevel", "error", "-i", str(wav),
                                "-sample_fmt", flac_fmt, "-bits_per_raw_sample", str(bits),
                                str(OUT / f"{name}.flac")], check=True)
                subprocess.run([ffmpeg, "-y", "-loglevel", "error", "-i", str(wav),
                                "-c:a", "alac", "-sample_fmt", sample_fmt,
                                str(OUT / f"{name}.m4a")], check=True)
    print(f"wrote fixtures to {OUT}")


if __name__ == "__main__":
    main()
