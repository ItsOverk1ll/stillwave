// SPDX-License-Identifier: GPL-3.0-only
use crate::format::StreamFormat;
use crate::signal_path::SignalPath;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("unsupported format: {0}")]
    Unsupported(String),
    #[error("device error: {0}")]
    Device(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("not implemented on this platform yet")]
    NotImplemented,
}

/// Turns a file into PCM. Implemented on Symphonia in Phase 1.
pub trait Decoder: Send {
    /// Format of the decoded stream (equals the file's native format).
    fn format(&self) -> StreamFormat;
    /// Total length in frames, when known.
    fn total_frames(&self) -> Option<u64>;
    /// True for lossy codecs (MP3, AAC, Vorbis, Opus). Never bit-perfect.
    fn is_lossy(&self) -> bool {
        false
    }
    /// Fills `out` with interleaved left-aligned samples; returns samples written
    /// (0 means end of stream). Never resamples or alters level.
    fn read(&mut self, out: &mut [i32]) -> Result<usize, AudioError>;
    /// Seeks to a frame position.
    fn seek(&mut self, frame: u64) -> Result<(), AudioError>;
}

/// How the device is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Bypass the OS mixer (`WASAPI` exclusive, `ALSA` direct, `AAudio` exclusive).
    Exclusive,
    /// Go through the OS mixer; the format may be altered by the OS.
    Shared,
}

/// A sound device exposed by a platform backend.
pub trait OutputDevice {
    fn name(&self) -> String;
    /// Formats the device accepts natively in `mode`.
    fn supported_formats(&self, mode: OutputMode) -> Result<Vec<StreamFormat>, AudioError>;
    /// Opens a stream. Must not resample: if `format` is unsupported it errors,
    /// and the caller decides what to do (and records it in the [`SignalPath`]).
    fn open(
        &self,
        format: StreamFormat,
        mode: OutputMode,
    ) -> Result<Box<dyn OutputStream>, AudioError>;
}

/// An open output stream.
pub trait OutputStream: Send {
    /// What the device is really doing, for the quality badge.
    fn signal_path(&self, source: StreamFormat, lossy_source: bool) -> SignalPath;
    /// Queues as many of `samples` (interleaved, left-aligned) as fit right now and
    /// returns how many were taken (possibly 0). Never blocks.
    fn try_write(&mut self, samples: &[i32]) -> Result<usize, AudioError>;

    /// Queues all of `samples`, waiting for room. Prefer [`try_write`](Self::try_write)
    /// where the caller must stay responsive (pause, seek).
    fn write(&mut self, samples: &[i32]) -> Result<usize, AudioError> {
        let mut rest = samples;
        while !rest.is_empty() {
            let n = self.try_write(rest)?;
            rest = &rest[n..];
            if n == 0 {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
        Ok(samples.len())
    }
    /// Blocks until everything written so far has been played.
    fn drain(&mut self) -> Result<(), AudioError>;
    /// Throws away audio that was written but not yet played (used by seek and stop).
    fn flush(&mut self) -> Result<(), AudioError>;
    /// Audio frames the device has actually consumed so far (silence excluded).
    fn frames_played(&self) -> u64;
    fn pause(&mut self) -> Result<(), AudioError>;
    fn resume(&mut self) -> Result<(), AudioError>;
}
