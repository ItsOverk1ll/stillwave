// SPDX-License-Identifier: GPL-3.0-only
//! Describes everything between the file and the output so the UI can show an
//! honest quality badge. Principle 1: bit-perfect unless the user opted out.

use serde::{Deserialize, Serialize};

use crate::format::StreamFormat;

/// What happens to the audio on its way out. Each flag is an independent fact
/// the UI reports, so a state machine would add nothing.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalPath {
    /// Format as stored in the file.
    pub source: StreamFormat,
    /// Format actually handed to the device.
    pub output: StreamFormat,
    /// Sample rate conversion is active.
    pub resampled: bool,
    /// Any DSP (EQ, crossfade, levelling, ...) is active.
    pub dsp_active: bool,
    /// Dither is applied (bit depth is being reduced).
    pub dithered: bool,
    /// Volume is applied in software rather than at the device.
    pub software_volume: bool,
    /// Device opened in exclusive mode (bypasses the OS mixer).
    pub exclusive: bool,
    /// The source codec is lossy.
    pub lossy_source: bool,
}

/// Coarse label for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityBadge {
    /// Samples reach the device untouched.
    BitPerfect,
    /// Lossless source, but processed (resampled, DSP, dither, software volume).
    Processed,
    /// Lossy source.
    Lossy,
}

impl SignalPath {
    /// A path with nothing between file and device ("Pure mode").
    #[must_use]
    pub const fn pure(format: StreamFormat, exclusive: bool, lossy_source: bool) -> Self {
        Self {
            source: format,
            output: format,
            resampled: false,
            dsp_active: false,
            dithered: false,
            software_volume: false,
            exclusive,
            lossy_source,
        }
    }

    /// True only when output samples equal source samples exactly.
    #[must_use]
    pub fn is_bit_perfect(&self) -> bool {
        self.source == self.output
            && !self.resampled
            && !self.dsp_active
            && !self.dithered
            && !self.software_volume
    }

    #[must_use]
    pub fn badge(&self) -> QualityBadge {
        if self.lossy_source {
            QualityBadge::Lossy
        } else if self.is_bit_perfect() {
            QualityBadge::BitPerfect
        } else {
            QualityBadge::Processed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CD: StreamFormat = StreamFormat::new(44_100, 16, 2);
    const HI: StreamFormat = StreamFormat::new(96_000, 24, 2);

    #[test]
    fn pure_path_is_bit_perfect() {
        let p = SignalPath::pure(HI, true, false);
        assert!(p.is_bit_perfect());
        assert_eq!(p.badge(), QualityBadge::BitPerfect);
    }

    #[test]
    fn any_processing_breaks_bit_perfect() {
        let base = SignalPath::pure(CD, false, false);
        let cases = [
            SignalPath {
                resampled: true,
                output: HI,
                ..base.clone()
            },
            SignalPath {
                dsp_active: true,
                ..base.clone()
            },
            SignalPath {
                dithered: true,
                ..base.clone()
            },
            SignalPath {
                software_volume: true,
                ..base.clone()
            },
            SignalPath {
                output: HI,
                ..base.clone()
            },
        ];
        for p in cases {
            assert!(!p.is_bit_perfect(), "{p:?}");
            assert_eq!(p.badge(), QualityBadge::Processed);
        }
    }

    #[test]
    fn lossy_sources_are_never_badged_bit_perfect() {
        assert_eq!(
            SignalPath::pure(CD, false, true).badge(),
            QualityBadge::Lossy
        );
    }

    #[test]
    fn format_displays_for_the_ui() {
        assert_eq!(HI.to_string(), "24-bit / 96 kHz / 2 ch");
        assert_eq!(CD.to_string(), "16-bit / 44.1 kHz / 2 ch");
    }
}
