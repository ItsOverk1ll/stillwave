// SPDX-License-Identifier: GPL-3.0-only
//! Audio interfaces. Phase 0 defines the shape only: no decoding or output yet.
//!
//! Sample convention: interleaved `i32`, left-aligned (a 16-bit sample is
//! shifted into the top 16 bits). Every integer depth up to 32 bits is then
//! carried exactly, so a bit-perfect path never converts through floats.

pub mod backends;
pub mod format;
pub mod signal_path;
pub mod symphonia_decoder;
pub mod traits;

pub use format::StreamFormat;
pub use signal_path::{QualityBadge, SignalPath};
pub use symphonia_decoder::FileDecoder;
pub use traits::{AudioError, Decoder, OutputDevice, OutputMode, OutputStream};
