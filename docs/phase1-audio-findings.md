# Phase 1 audio findings

## What is verified (automated, runs in CI)

- Decoding WAV, FLAC and ALAC at 16-bit/44.1 kHz, 24-bit/96 kHz and 24-bit/192 kHz equals an independently parsed WAV reference sample for sample.
- The bytes handed to a device (decode, then pack) equal the PCM bytes inside the original WAV.
- Gapless pairs decode to the exact concatenation, with no padding or priming samples and no silence at the join.
- The player delivers a file, a gapless queue, and a format-changing queue to a mock device bit-exactly, seeks accurately, pauses and stops promptly, skips unreadable files, and refuses to fall back to the mixer unless asked.
- The scanner reads tags and properties (Lofty), skips unchanged files on rescan, keeps item identity across retags, never modifies audio files, and is cancellable.

## What is NOT verified

- **Real hardware playback.** Nobody has listened to it. `cargo run -p stillwave-audio --example play -- <file>` does this; exclusive mode silences other apps while it plays.
- Behaviour when a device is unplugged mid-playback, on sleep/resume, or under CPU load (the underrun counter exists; there is no stress test yet).
- Linux (ALSA, PipeWire), Android (AAudio) and iOS: only the traits and empty backends exist.
- Lofty and Symphonia on very large or damaged real libraries (the tests use generated files).

## Device probe on the development PC (2026-10-18, `--example devices`)

| Endpoint | Exclusive PCM (stereo) | Shared mix format |
| --- | --- | --- |
| Speakers (QuadCast S) | 16-bit: 44.1, 48 kHz | 24-bit / 48 kHz / 2 ch |
| LG ULTRAGEAR+ (NVIDIA HD Audio, HDMI) | 16 and 24-bit: 44.1, 48 kHz | 24-bit / 48 kHz / 2 ch |
| Speakers (PRO X 2 LIGHTSPEED headset, system default) | none accepted as stereo | 24-bit / 48 kHz / 8 ch |
| Voicemod virtual device | 16-bit: 48 kHz | 24-bit / 48 kHz / 2 ch |

Consequences: none of these devices can play 24-bit/96 kHz or higher in exclusive mode, so true high-resolution bit-perfect output cannot be tested on this machine without a USB DAC. The headset advertises an 8-channel layout and refused exclusive stereo; negotiating the channel layout for such devices is future work.

## Known limits

- A sample rate the device cannot take is never resampled; the player reports an error (or plays through the mixer only if `allow_shared_fallback` is set).
- A format change between tracks leaves a short gap.
- Multichannel files and channel masks beyond stereo are untested.
- The Tauri app does not play audio or scan yet. Phase 1 still needs library views and wiring the player and scanner into the app.
