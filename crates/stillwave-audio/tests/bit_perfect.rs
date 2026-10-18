// SPDX-License-Identifier: GPL-3.0-only
#![allow(clippy::verbose_bit_mask, clippy::chunks_exact_to_as_chunks)]
//! Bit-perfect decoding tests.
//!
//! Reference = the PCM inside each generated WAV, parsed here by hand (not by
//! Symphonia), converted to left-aligned `i32`. WAV, FLAC and ALAC decodes of the
//! same fixture must match it sample for sample. Generate fixtures first:
//! `python scripts/gen_fixtures.py` (FLAC/ALAC need ffmpeg).
//!
//! Without fixtures the tests skip with a message, unless
//! `STILLWAVE_REQUIRE_FIXTURES=1` (set in CI), in which case they fail.

use std::path::{Path, PathBuf};

use stillwave_audio::{Decoder, FileDecoder, StreamFormat};

const FORMATS: &[(&str, u32, u8)] = &[
    ("16-44k", 44_100, 16),
    ("24-96k", 96_000, 24),
    ("24-192k", 192_000, 24),
];

/// CI sets this to `1` so missing fixtures fail instead of skipping.
fn require_fixtures() -> bool {
    std::env::var("STILLWAVE_REQUIRE_FIXTURES").as_deref() == Ok("1")
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/generated")
}

fn fixtures_ready(ext: &str) -> bool {
    let ok = FORMATS
        .iter()
        .all(|(tag, ..)| dir().join(format!("sweep-{tag}.{ext}")).exists());
    if !ok {
        assert!(
            !require_fixtures(),
            "fixtures (*.{ext}) missing but STILLWAVE_REQUIRE_FIXTURES is set"
        );
        eprintln!("SKIPPED: no *.{ext} fixtures; run scripts/gen_fixtures.py");
    }
    ok
}

/// FNV-1a over the little-endian bytes of each sample.
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

/// Minimal RIFF/WAVE PCM reader: (sample rate, bits, channels, left-aligned samples).
fn read_wav_reference(path: &Path) -> (u32, u8, u8, Vec<i32>) {
    let b = std::fs::read(path).unwrap();
    assert_eq!(&b[0..4], b"RIFF");
    assert_eq!(&b[8..12], b"WAVE");
    let (mut rate, mut bits, mut ch) = (0u32, 0u8, 0u8);
    let mut pos = 12;
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let len = u32::from_le_bytes(b[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &b[pos + 8..pos + 8 + len];
        if id == b"fmt " {
            ch = u8::try_from(u16::from_le_bytes(body[2..4].try_into().unwrap())).unwrap();
            rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            bits = u8::try_from(u16::from_le_bytes(body[14..16].try_into().unwrap())).unwrap();
        } else if id == b"data" {
            let width = usize::from(bits / 8);
            let samples = body
                .chunks_exact(width)
                .map(|c| {
                    let mut w = [0u8; 4];
                    w[4 - width..].copy_from_slice(c); // little-endian, top-aligned
                    i32::from_le_bytes(w)
                })
                .collect();
            return (rate, bits, ch, samples);
        }
        pos += 8 + len + (len & 1);
    }
    panic!("no data chunk in {}", path.display());
}

fn reference_samples(path: &Path) -> Vec<i32> {
    read_wav_reference(path).3
}

fn decode_all(path: &Path) -> (StreamFormat, Option<u64>, Vec<i32>) {
    let mut d = FileDecoder::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (format, total) = (d.format(), d.total_frames());
    let mut all = Vec::new();
    let mut buf = vec![0i32; 4096 * 2 + 1]; // odd size: exercises chunk boundaries
    loop {
        let n = d.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        all.extend_from_slice(&buf[..n]);
    }
    assert_eq!(
        d.skipped_packets(),
        0,
        "{} had corrupt packets",
        path.display()
    );
    (format, total, all)
}

fn first_difference(a: &[i32], b: &[i32]) -> Option<usize> {
    a.iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .or_else(|| (a.len() != b.len()).then(|| a.len().min(b.len())))
}

fn check_matches_reference(ext: &str) {
    if !fixtures_ready(ext) {
        return;
    }
    for (tag, rate, bits) in FORMATS {
        for name in ["sweep", "gapless-a", "gapless-b"] {
            let wav = dir().join(format!("{name}-{tag}.wav"));
            let file = dir().join(format!("{name}-{tag}.{ext}"));
            let (ref_rate, ref_bits, ref_ch, reference) = read_wav_reference(&wav);
            assert_eq!((ref_rate, ref_bits), (*rate, *bits));
            let (format, total, decoded) = decode_all(&file);

            assert_eq!(
                format,
                StreamFormat::new(*rate, *bits, ref_ch),
                "{}: reported format",
                file.display()
            );
            assert_eq!(
                decoded.len(),
                reference.len(),
                "{}: sample count (no padding or priming allowed)",
                file.display()
            );
            if let Some(t) = total {
                assert_eq!(
                    t * u64::from(ref_ch),
                    reference.len() as u64,
                    "{}: total_frames",
                    file.display()
                );
            }
            assert_eq!(
                first_difference(&decoded, &reference),
                None,
                "{}: first differing sample index",
                file.display()
            );
            assert_eq!(hash_samples(&decoded), hash_samples(&reference));
        }
    }
}

#[test]
fn wav_decodes_exactly_to_the_reference() {
    check_matches_reference("wav");
}

#[test]
fn flac_decodes_bit_for_bit() {
    check_matches_reference("flac");
}

#[test]
fn alac_decodes_bit_for_bit() {
    check_matches_reference("m4a");
}

#[test]
fn decoded_16_bit_audio_has_silent_low_bits_and_24_bit_does_not_overflow() {
    if !fixtures_ready("flac") {
        return;
    }
    let (_, _, s16) = decode_all(&dir().join("sweep-16-44k.flac"));
    assert!(
        s16.iter().all(|s| s & 0xFFFF == 0),
        "16-bit data must sit in the top 16 bits"
    );
    let (_, _, s24) = decode_all(&dir().join("sweep-24-96k.flac"));
    assert!(
        s24.iter().all(|s| s & 0xFF == 0),
        "24-bit data must sit in the top 24 bits"
    );
    assert!(
        s24.iter().any(|s| s & 0xFF00 != 0),
        "24-bit fixture should use its low bits"
    );
}

/// Gapless pairs: A ends in a tone and B starts with the continuation. Decoded
/// back to back there must be no extra samples and no silence at the join.
fn check_gapless(ext: &str) {
    if !fixtures_ready(ext) {
        return;
    }
    for (tag, _rate, _bits) in FORMATS {
        let (_, _, a) = decode_all(&dir().join(format!("gapless-a-{tag}.{ext}")));
        let (_, _, b) = decode_all(&dir().join(format!("gapless-b-{tag}.{ext}")));
        let ra = reference_samples(&dir().join(format!("gapless-a-{tag}.wav")));
        let rb = reference_samples(&dir().join(format!("gapless-b-{tag}.wav")));
        let joined: Vec<i32> = a.iter().chain(&b).copied().collect();
        let expected: Vec<i32> = ra.iter().chain(&rb).copied().collect();
        assert_eq!(
            first_difference(&joined, &expected),
            None,
            "{ext} {tag}: joined stream differs"
        );

        // The join: stereo frames around it must carry signal, never a run of zeros.
        let join = a.len();
        let window = &joined[join.saturating_sub(64)..join + 64];
        let zero_frames = window
            .chunks_exact(2)
            .filter(|f| f[0] == 0 && f[1] == 0)
            .count();
        assert!(
            zero_frames <= 2,
            "{ext} {tag}: {zero_frames} silent frames at the join"
        );
    }
}

#[test]
fn flac_pairs_join_without_a_gap() {
    check_gapless("flac");
}

#[test]
fn alac_pairs_join_without_a_gap() {
    check_gapless("m4a");
}

#[test]
fn seeking_lands_on_the_right_sample() {
    if !fixtures_ready("flac") {
        return;
    }
    let path = dir().join("sweep-24-96k.flac");
    let reference = reference_samples(&dir().join("sweep-24-96k.wav"));
    let mut d = FileDecoder::open(&path).unwrap();
    let target_frame = 123_457u64;
    d.seek(target_frame).unwrap();
    let mut buf = vec![0i32; 64];
    let n = d.read(&mut buf).unwrap();
    assert!(n > 0);
    // An accurate seek lands at or before the target; find where we really are.
    let start = reference
        .windows(buf.len().min(n))
        .position(|w| w == &buf[..buf.len().min(n)])
        .expect("decoded data after seek must exist in the reference");
    let landed_frame = (start / 2) as u64;
    assert!(
        landed_frame <= target_frame,
        "accurate seek must not overshoot ({landed_frame} > {target_frame})"
    );
    assert!(
        target_frame - landed_frame < 20_000,
        "seek landed too far before the target"
    );
}

#[test]
fn missing_and_non_audio_files_error_cleanly() {
    assert!(FileDecoder::open(Path::new("Z:/nope/missing.flac")).is_err());
    let junk = std::env::temp_dir().join("stillwave-not-audio.flac");
    std::fs::write(&junk, b"definitely not a flac stream").unwrap();
    assert!(FileDecoder::open(&junk).is_err());
    let _ = std::fs::remove_file(junk);
}

#[test]
fn lossy_files_are_flagged_lossy_and_still_decode() {
    for ext in ["mp3", "ogg"] {
        let path = dir().join(format!("lossy-16-44k.{ext}"));
        if !path.exists() {
            assert!(!require_fixtures(), "{ext} fixture missing");
            eprintln!("SKIPPED: no lossy .{ext} fixture");
            continue;
        }
        let mut d = FileDecoder::open(&path).unwrap();
        assert!(d.is_lossy(), "{ext} must be flagged lossy");
        let mut buf = vec![0i32; 8192];
        let n = d.read(&mut buf).unwrap();
        assert!(n > 0);
        assert!(buf[..n].iter().any(|s| *s != 0), "{ext} decoded to silence");
        // Lossy decodes are rendered at 24-bit resolution, left-aligned.
        assert!(buf[..n].iter().all(|s| s & 0xFF == 0));
    }
}
