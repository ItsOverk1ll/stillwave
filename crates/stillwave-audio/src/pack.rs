// SPDX-License-Identifier: GPL-3.0-only
//! Turns left-aligned `i32` samples into the byte layout an audio device wants.
//!
//! This is the last step before the device buffer, so it is kept free of unsafe
//! code and tested bit-for-bit. Samples stay MSB-justified in their container,
//! which is how `WAVEFORMATEXTENSIBLE` defines them: 24-bit audio in a 32-bit
//! container has its low byte zero.

/// Bytes per sample in the device buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// 16-bit samples, 2 bytes.
    Bits16,
    /// 24-bit samples packed in 3 bytes.
    Bits24Packed,
    /// 32-bit container (carrying 16, 24 or 32 valid bits), 4 bytes.
    Bits32,
}

impl Container {
    #[must_use]
    pub const fn bytes(self) -> usize {
        match self {
            Self::Bits16 => 2,
            Self::Bits24Packed => 3,
            Self::Bits32 => 4,
        }
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            Self::Bits16 => 16,
            Self::Bits24Packed => 24,
            Self::Bits32 => 32,
        }
    }

    /// True when every sample of `valid_bits` audio survives this container unchanged.
    #[must_use]
    pub const fn holds_exactly(self, valid_bits: u8) -> bool {
        valid_bits <= self.bits()
    }
}

/// Packs `samples` into `out` (little-endian). `out` must hold exactly
/// `samples.len() * container.bytes()` bytes.
///
/// Only the top bits are kept for narrow containers. That is lossless when the
/// audio's valid bit depth fits the container (see [`Container::holds_exactly`]);
/// callers must not pick a narrower container for deeper audio unless they have
/// chosen to reduce bit depth (with dither, which is not done here).
///
/// # Panics
///
/// Panics if `out` has the wrong length; that is a programming error.
pub fn pack(samples: &[i32], container: Container, out: &mut [u8]) {
    assert_eq!(
        out.len(),
        samples.len() * container.bytes(),
        "output buffer has the wrong size"
    );
    match container {
        Container::Bits16 => {
            for (s, o) in samples.iter().zip(out.chunks_exact_mut(2)) {
                o.copy_from_slice(&((s >> 16) as i16).to_le_bytes());
            }
        }
        Container::Bits24Packed => {
            for (s, o) in samples.iter().zip(out.chunks_exact_mut(3)) {
                o.copy_from_slice(&s.to_le_bytes()[1..4]);
            }
        }
        Container::Bits32 => {
            for (s, o) in samples.iter().zip(out.chunks_exact_mut(4)) {
                o.copy_from_slice(&s.to_le_bytes());
            }
        }
    }
}

/// Reverse of [`pack`]: container bytes back to left-aligned `i32`.
///
/// # Panics
///
/// Panics if `bytes` is not a whole number of samples or `out` has the wrong length.
pub fn unpack(bytes: &[u8], container: Container, out: &mut [i32]) {
    assert_eq!(bytes.len(), out.len() * container.bytes());
    match container {
        Container::Bits16 => {
            for (b, o) in bytes.chunks_exact(2).zip(out) {
                *o = i32::from(i16::from_le_bytes([b[0], b[1]])) << 16;
            }
        }
        Container::Bits24Packed => {
            for (b, o) in bytes.chunks_exact(3).zip(out) {
                *o = i32::from_le_bytes([0, b[0], b[1], b[2]]);
            }
        }
        Container::Bits32 => {
            for (b, o) in bytes.chunks_exact(4).zip(out) {
                *o = i32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            }
        }
    }
}

/// Converts left-aligned `i32` to `f32` in [-1, 1) for shared-mode mixers.
/// Exact for audio up to 24 bits (an `f32` has a 24-bit significand).
#[must_use]
pub fn to_f32(sample: i32) -> f32 {
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    {
        (f64::from(sample) / 2_147_483_648.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDGES: [i32; 8] = [
        0,
        1 << 16,
        -(1 << 16),
        0x7fff_0000,
        i32::MIN,
        0x1234_5600,
        -0x1234_5600,
        0x00ab_cd00,
    ];

    fn round_trip(c: Container, samples: &[i32]) {
        let mut bytes = vec![0u8; samples.len() * c.bytes()];
        pack(samples, c, &mut bytes);
        let mut back = vec![0i32; samples.len()];
        unpack(&bytes, c, &mut back);
        assert_eq!(back, samples, "{c:?}");
    }

    #[test]
    fn container_32_is_the_identity() {
        round_trip(Container::Bits32, &EDGES);
    }

    #[test]
    fn packed_24_round_trips_24_bit_audio() {
        let s24: Vec<i32> = EDGES.iter().map(|s| s & !0xff).collect();
        round_trip(Container::Bits24Packed, &s24);
    }

    #[test]
    fn container_16_round_trips_16_bit_audio() {
        let s16: Vec<i32> = EDGES.iter().map(|s| s & !0xffff).collect();
        round_trip(Container::Bits16, &s16);
    }

    #[test]
    fn little_endian_layout_is_as_wasapi_expects() {
        let mut out = [0u8; 3];
        pack(&[0x1122_3300], Container::Bits24Packed, &mut out);
        assert_eq!(out, [0x33, 0x22, 0x11]);
        let mut out = [0u8; 2];
        pack(&[0x1122_0000], Container::Bits16, &mut out);
        assert_eq!(out, [0x22, 0x11]);
        let mut out = [0u8; 4];
        pack(&[0x1122_3344], Container::Bits32, &mut out);
        assert_eq!(out, [0x44, 0x33, 0x22, 0x11]);
    }

    #[test]
    fn narrow_containers_report_whether_they_are_lossless() {
        assert!(Container::Bits16.holds_exactly(16));
        assert!(!Container::Bits16.holds_exactly(24));
        assert!(Container::Bits24Packed.holds_exactly(24));
        assert!(!Container::Bits24Packed.holds_exactly(32));
        assert!(Container::Bits32.holds_exactly(32));
    }

    #[test]
    #[allow(clippy::cast_possible_truncation)]
    fn f32_conversion_is_exact_up_to_24_bits() {
        for s in [0, 1 << 8, -(1 << 8), 0x7fff_ff00, i32::MIN, 0x1234_5600] {
            let f = to_f32(s);
            let back = (f64::from(f) * 2_147_483_648.0) as i64;
            assert_eq!(back, i64::from(s), "{s:#x}");
        }
    }

    #[test]
    #[should_panic(expected = "wrong size")]
    fn wrong_buffer_size_is_a_programming_error() {
        pack(&[0, 0], Container::Bits16, &mut [0u8; 3]);
    }
}
