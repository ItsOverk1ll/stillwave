# ADR 0006: Audio output per platform

Status: accepted (Phase 0)

## Context

Bit-perfect output needs OS-specific APIs; no cross-platform audio crate guarantees untouched samples.

## Decision

WASAPI (Windows, exclusive and shared), ALSA direct or PipeWire (Linux, Pi), AAudio (Android, bit-perfect USB on 14+), native AVAudioSession via a Swift plugin (iPhone). Phase 0 defines traits and a `SignalPath` model and empty cfg-gated backends.

## Consequences

More code than one abstraction crate, but the signal path can be reported honestly. Android and iOS behaviour needs spikes on real devices (not yet done).
