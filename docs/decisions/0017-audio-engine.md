# ADR 0017: Phase 1 audio engine design

Status: accepted (Phase 1)

## Context

Principle 1 says the file as recorded leaves the app unless the user chooses otherwise. Phase 1 had to turn that into code that can be tested without a human listening.

## Decision

- **Decoding** uses Symphonia. Integer PCM (WAV, AIFF, FLAC, ALAC) is moved into the top bits of an `i32` with plain shifts: no gain, rounding or dither. Lossy codecs decode to float and are quantised at 24-bit; the decoder reports `is_lossy()` and such files can never be badged bit-perfect. Lossless is decided by codec, not by whether a bit depth is reported (ALAC reports none; its depth is read from its configuration record).
- **Packing** (`pack.rs`) is the last step before a device buffer and contains no unsafe code. Tests prove FLAC and ALAC fixtures, decoded and packed, equal the PCM bytes in the source WAV exactly (16/44.1, 24/96, 24/192).
- **Output** (WASAPI) runs a render thread that owns all COM objects, fed by a lock-free ring buffer (about 300 ms). The caller never blocks the render thread and the render thread never decodes. Exclusive mode tries the narrowest lossless container first (16-bit audio may travel in a wider container with zero padding and stay bit-perfect). It never resamples or reduces bit depth; an unsupported format is an error.
- **Shared mode** (the Windows mixer) is available but off by default as a fallback. When used, the signal path reports it as processed, never bit-perfect.
- **Unsafe** is allowed in exactly one module (`wasapi.rs`); the rest of `stillwave-audio` is `deny(unsafe_code)`. The crate therefore has its own lint table instead of the workspace one.
- **Player** (`player.rs`) is platform-neutral and talks only to `OutputDevice`/`OutputStream`. Gapless comes from the output buffer: the next file is opened while buffered audio plays, on the same stream when the format matches. A format change drains and reopens the stream (a short gap), because the alternative is silent resampling.
- Writes are non-blocking (`try_write`) so pause, seek and stop respond immediately even when the buffer is full or the device is paused.

## Consequences

- The bit-perfect claim is enforced by tests that run in CI against generated fixtures, not by listening.
- Not covered by tests: what a real DAC does with the bytes, device removal, and glitch-free playback under load. Those need hardware (see docs/phase1-audio-findings.md).
- A USB DAC that only accepts 24-bit packed or 32-bit containers still works, since containers are negotiated.
