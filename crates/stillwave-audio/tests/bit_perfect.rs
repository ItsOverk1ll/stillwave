// SPDX-License-Identifier: GPL-3.0-only
//! Harness skeleton for the Phase 1 bit-perfect tests.
//!
//! Plan: for each fixture, decode with the real `Decoder`, hash the samples, and
//! compare with the hash of the PCM inside the matching WAV (the reference).
//! FLAC and ALAC must match the WAV exactly. Gapless pairs must produce no
//! zero-run at the join. Ignored until Phase 1 provides a decoder.
//!
//! Generate fixtures first: `python scripts/gen_fixtures.py`.

use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/generated")
}

/// FNV-1a over the little-endian bytes of each sample: stable and dependency-free.
fn hash_samples(samples: &[i32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for s in samples {
        for b in s.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

#[test]
fn sample_hash_is_sensitive_to_a_single_bit() {
    let a = [1, 2, 3, 4];
    let mut b = a;
    b[2] ^= 1;
    assert_ne!(hash_samples(&a), hash_samples(&b));
    assert_eq!(hash_samples(&a), hash_samples(&a));
}

#[test]
#[ignore = "needs Phase 1 decoder and generated fixtures"]
fn decoded_samples_match_the_wav_reference() {
    assert!(fixtures_dir().exists(), "run scripts/gen_fixtures.py first");
    unimplemented!("Phase 1: decode each fixture, compare hash_samples against the WAV reference");
}

#[test]
#[ignore = "needs Phase 1 decoder and generated fixtures"]
fn gapless_pairs_have_no_silence_at_the_join() {
    unimplemented!("Phase 1: decode a then b, assert no run of zero frames across the boundary");
}
