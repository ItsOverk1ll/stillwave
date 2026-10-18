// SPDX-License-Identifier: GPL-3.0-only
use serde::{Deserialize, Serialize};

/// PCM stream parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StreamFormat {
    pub sample_rate_hz: u32,
    pub bit_depth: u8,
    pub channels: u8,
}

impl StreamFormat {
    #[must_use]
    pub const fn new(sample_rate_hz: u32, bit_depth: u8, channels: u8) -> Self {
        Self {
            sample_rate_hz,
            bit_depth,
            channels,
        }
    }
}

impl std::fmt::Display for StreamFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}-bit / {} kHz / {} ch",
            self.bit_depth,
            f64::from(self.sample_rate_hz) / 1000.0,
            self.channels
        )
    }
}
