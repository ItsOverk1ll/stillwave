// SPDX-License-Identifier: GPL-3.0-only
//! Audio interfaces. Phase 0 defines the shape only: no decoding or output yet.
//!
//! Sample convention: interleaved `i32`, left-aligned (a 16-bit sample is
//! shifted into the top 16 bits). Every integer depth up to 32 bits is then
//! carried exactly, so a bit-perfect path never converts through floats.

pub mod backends;
pub mod format;
pub mod pack;
pub mod player;
pub mod signal_path;
pub mod symphonia_decoder;
pub mod traits;
#[cfg(windows)]
pub mod wasapi;

pub use format::StreamFormat;
pub use player::{PlayState, Player, PlayerOptions, PlayerStatus};
pub use signal_path::{QualityBadge, SignalPath};
pub use symphonia_decoder::FileDecoder;
pub use traits::{AudioError, Decoder, OutputDevice, OutputMode, OutputStream};
