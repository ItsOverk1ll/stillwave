// SPDX-License-Identifier: GPL-3.0-only
#![allow(
    clippy::verbose_bit_mask,
    clippy::cast_possible_truncation,
    clippy::manual_assert_eq,
    clippy::many_single_char_names
)]
//! End-to-end tests of the playback engine against a mock output device.
//!
//! The mock behaves like a real device where it matters: it buffers ahead of
//! "playing" (so flush on seek has something to drop), it can refuse exclusive
//! mode, and its writes can be gated so tests are deterministic. What it
//! captures is exactly what a real device would be handed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use stillwave_audio::{
    AudioError, OutputDevice, OutputMode, OutputStream, PlayState, Player, PlayerOptions,
    SignalPath, StreamFormat,
};

const LAG_FRAMES: u64 = 3000;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/generated")
}

fn fixture(name: &str) -> Option<PathBuf> {
    let p = dir().join(name);
    if p.exists() {
        return Some(p);
    }
    assert!(
        std::env::var("STILLWAVE_REQUIRE_FIXTURES").as_deref() != Ok("1"),
        "fixture {name} missing"
    );
    eprintln!("SKIPPED: fixture {name} missing; run scripts/gen_fixtures.py");
    None
}

/// Reference PCM from a WAV, left-aligned (independent of the decoder under test).
fn reference(path: &Path) -> Vec<i32> {
    let b = std::fs::read(path).unwrap();
    let mut bits = 16usize;
    let mut pos = 12;
    while pos + 8 <= b.len() {
        let len = u32::from_le_bytes(b[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &b[pos + 8..pos + 8 + len];
        if &b[pos..pos + 4] == b"fmt " {
            bits = usize::from(u16::from_le_bytes(body[14..16].try_into().unwrap()));
        } else if &b[pos..pos + 4] == b"data" {
            let w = bits / 8;
            return body
                .chunks_exact(w)
                .map(|c| {
                    let mut x = [0u8; 4];
                    x[4 - w..].copy_from_slice(c);
                    i32::from_le_bytes(x)
                })
                .collect();
        }
        pos += 8 + len + (len & 1);
    }
    panic!("no data");
}

#[derive(Default)]
struct StreamLog {
    format: Option<StreamFormat>,
    mode: Option<OutputMode>,
    samples: Vec<i32>,
    drained: bool,
    flushes: u32,
}

#[derive(Default)]
struct World {
    /// Sample budget for gated writes; `usize::MAX` = ungated.
    budget: Mutex<usize>,
    wake: Condvar,
}

struct MockDevice {
    world: Arc<World>,
    refuse_exclusive: bool,
}

struct MockStream {
    log: Arc<Mutex<StreamLog>>,
    world: Arc<World>,
    format: StreamFormat,
    exclusive: bool,
    paused: bool,
}

impl OutputDevice for MockDevice {
    fn name(&self) -> String {
        "mock".into()
    }
    fn supported_formats(&self, _: OutputMode) -> Result<Vec<StreamFormat>, AudioError> {
        Ok(vec![])
    }
    fn open(
        &self,
        format: StreamFormat,
        mode: OutputMode,
    ) -> Result<Box<dyn OutputStream>, AudioError> {
        if mode == OutputMode::Exclusive && self.refuse_exclusive {
            return Err(AudioError::Unsupported("mock refuses exclusive".into()));
        }
        let log = Arc::new(Mutex::new(StreamLog {
            format: Some(format),
            mode: Some(mode),
            ..StreamLog::default()
        }));
        // Keep every stream's log reachable so tests can inspect what the device got.
        STREAMS.lock().unwrap().push(Arc::clone(&log));
        Ok(Box::new(MockStream {
            log,
            world: Arc::clone(&self.world),
            format,
            exclusive: mode == OutputMode::Exclusive,
            paused: false,
        }))
    }
}

static STREAMS: Mutex<Vec<Arc<Mutex<StreamLog>>>> = Mutex::new(Vec::new());

impl OutputStream for MockStream {
    fn signal_path(&self, source: StreamFormat, lossy: bool) -> SignalPath {
        let mut p = SignalPath::pure(source, self.exclusive, lossy);
        p.software_volume = !self.exclusive; // the shared mixer owns the volume
        p
    }
    fn try_write(&mut self, samples: &[i32]) -> Result<usize, AudioError> {
        // Gate: refuse (without blocking) until the test grants budget.
        let mut b = self.world.budget.lock().unwrap();
        if *b < samples.len() {
            return Ok(0);
        }
        if *b != usize::MAX {
            *b -= samples.len();
        }
        drop(b);
        self.log.lock().unwrap().samples.extend_from_slice(samples);
        Ok(samples.len())
    }
    fn drain(&mut self) -> Result<(), AudioError> {
        self.log.lock().unwrap().drained = true;
        Ok(())
    }
    fn flush(&mut self) -> Result<(), AudioError> {
        let mut l = self.log.lock().unwrap();
        // Unplayed audio is dropped, like a real device buffer.
        let lag = (LAG_FRAMES as usize * usize::from(self.format.channels)).min(l.samples.len());
        let new_len = l.samples.len() - lag;
        l.samples.truncate(new_len);
        l.flushes += 1;
        Ok(())
    }
    fn frames_played(&self) -> u64 {
        let l = self.log.lock().unwrap();
        let written = (l.samples.len() / usize::from(self.format.channels)) as u64;
        if l.drained || self.paused {
            written
        } else {
            written.saturating_sub(LAG_FRAMES)
        }
    }
    fn pause(&mut self) -> Result<(), AudioError> {
        self.paused = true;
        Ok(())
    }
    fn resume(&mut self) -> Result<(), AudioError> {
        self.paused = false;
        Ok(())
    }
}

/// Serialises tests: the mock's stream list is process-global.
static SERIAL: Mutex<()> = Mutex::new(());

struct Rig {
    player: Player,
    world: Arc<World>,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        // A failed assertion must not leave the worker stuck at a closed gate.
        open_gate(&self.world);
    }
}

fn rig(refuse_exclusive: bool, allow_fallback: bool, gated: bool) -> Rig {
    let guard = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    STREAMS.lock().unwrap().clear();
    let world = Arc::new(World {
        budget: Mutex::new(if gated { 0 } else { usize::MAX }),
        ..World::default()
    });
    let device = MockDevice {
        world: Arc::clone(&world),
        refuse_exclusive,
    };
    let player = Player::new(
        Box::new(device),
        PlayerOptions {
            mode: OutputMode::Exclusive,
            allow_shared_fallback: allow_fallback,
        },
    );
    Rig {
        player,
        world,
        _guard: guard,
    }
}

fn streams() -> Vec<Arc<Mutex<StreamLog>>> {
    STREAMS.lock().unwrap().clone()
}

fn open_gate(world: &World) {
    *world.budget.lock().unwrap() = usize::MAX;
    world.wake.notify_all();
}

const WAIT: Duration = Duration::from_secs(20);

#[test]
fn a_file_reaches_the_device_bit_for_bit() {
    let (Some(flac), Some(wav)) = (fixture("sweep-24-96k.flac"), fixture("sweep-24-96k.wav"))
    else {
        return;
    };
    let r = rig(false, false, false);
    r.player.play(vec![flac]);
    assert!(r.player.wait_until_idle(WAIT));
    let st = r.player.status();
    assert_eq!(st.state, PlayState::Finished, "{:?}", st.error);
    let s = streams();
    assert_eq!(s.len(), 1);
    let log = s[0].lock().unwrap();
    assert_eq!(log.mode, Some(OutputMode::Exclusive));
    assert!(
        log.drained,
        "stream must be drained at the end of the queue"
    );
    assert!(
        log.samples == reference(&wav),
        "device received different samples than the WAV holds"
    );
}

#[test]
fn same_format_files_play_gaplessly_on_one_stream() {
    let (Some(a), Some(b)) = (
        fixture("gapless-a-16-44k.flac"),
        fixture("gapless-b-16-44k.flac"),
    ) else {
        return;
    };
    let (Some(ra), Some(rb)) = (
        fixture("gapless-a-16-44k.wav"),
        fixture("gapless-b-16-44k.wav"),
    ) else {
        return;
    };
    let r = rig(false, false, false);
    r.player.play(vec![a, b]);
    assert!(r.player.wait_until_idle(WAIT));
    assert_eq!(r.player.status().state, PlayState::Finished);
    let s = streams();
    assert_eq!(s.len(), 1, "same format must reuse the stream (no gap)");
    let mut expected = reference(&ra);
    expected.extend(reference(&rb));
    assert!(
        s[0].lock().unwrap().samples == expected,
        "joined output differs from a++b"
    );
}

#[test]
fn a_format_change_drains_and_reopens_without_altering_audio() {
    let (Some(a), Some(b)) = (
        fixture("gapless-a-16-44k.flac"),
        fixture("sweep-24-96k.m4a"),
    ) else {
        return;
    };
    let (Some(ra), Some(rb)) = (fixture("gapless-a-16-44k.wav"), fixture("sweep-24-96k.wav"))
    else {
        return;
    };
    let r = rig(false, false, false);
    r.player.play(vec![a, b]);
    assert!(r.player.wait_until_idle(WAIT));
    let s = streams();
    assert_eq!(
        s.len(),
        2,
        "different format needs a new stream; no resampling"
    );
    let (first, second) = (s[0].lock().unwrap(), s[1].lock().unwrap());
    assert_eq!(first.format, Some(StreamFormat::new(44_100, 16, 2)));
    assert_eq!(second.format, Some(StreamFormat::new(96_000, 24, 2)));
    assert!(
        first.drained,
        "the old stream must finish playing before the new one"
    );
    assert!(first.samples == reference(&ra));
    assert!(second.samples == reference(&rb));
}

#[test]
fn seeking_drops_buffered_audio_and_continues_from_the_new_position() {
    let (Some(flac), Some(wav)) = (fixture("sweep-24-96k.flac"), fixture("sweep-24-96k.wav"))
    else {
        return;
    };
    let reference = reference(&wav);
    let r = rig(false, false, true);
    // Let ~1 s of audio through the gate, then seek.
    *r.world.budget.lock().unwrap() = 96_000 * 2;
    r.player.play(vec![flac]);
    std::thread::sleep(Duration::from_millis(400));
    let target = 400_000u64;
    r.player.seek(target);
    std::thread::sleep(Duration::from_millis(100));
    open_gate(&r.world);
    assert!(r.player.wait_until_idle(WAIT));

    let s = streams();
    let log = s[0].lock().unwrap();
    assert_eq!(log.flushes, 1);
    // Before the seek the device got a clean prefix of the reference; after it,
    // an exact suffix that starts at or shortly before the target.
    let k = log
        .samples
        .iter()
        .zip(&reference)
        .take_while(|(a, b)| a == b)
        .count();
    assert!(k > 0, "some audio should have played before the seek");
    let rest = &log.samples[k..];
    assert!(rest.len() >= 64);
    let lo = (target as usize).saturating_sub(30_000);
    let landed = (lo..=target as usize)
        .find(|f| reference[2 * f..2 * f + 64] == rest[..64])
        .expect("post-seek audio must be a slice of the reference at or before the target");
    assert!(
        rest == &reference[2 * landed..],
        "audio after the seek must match the reference to the very end"
    );
}

#[test]
fn an_unreadable_file_is_skipped_and_reported_not_fatal() {
    let Some(good) = fixture("gapless-a-16-44k.wav") else {
        return;
    };
    let junk = std::env::temp_dir().join("stillwave-player-junk.flac");
    std::fs::write(&junk, b"not audio").unwrap();
    let r = rig(false, false, false);
    r.player
        .play(vec![junk.clone(), good.clone(), junk.clone()]);
    assert!(r.player.wait_until_idle(WAIT));
    let st = r.player.status();
    assert_eq!(st.state, PlayState::Finished);
    assert!(st
        .error
        .as_deref()
        .unwrap_or("")
        .contains("stillwave-player-junk"));
    assert!(streams()[0].lock().unwrap().samples == reference(&good));
    let _ = std::fs::remove_file(junk);
}

#[test]
fn exclusive_refusal_is_an_error_unless_shared_fallback_is_allowed() {
    let Some(f) = fixture("gapless-a-16-44k.flac") else {
        return;
    };
    let strict = rig(true, false, false);
    strict.player.play(vec![f.clone()]);
    assert!(strict.player.wait_until_idle(WAIT));
    let st = strict.player.status();
    assert_eq!(st.state, PlayState::Error);
    // The user must see the real reason, not just "nothing could be played".
    let msg = st.error.unwrap_or_default();
    assert!(
        msg.contains("Nothing could be played") && msg.contains("mock refuses exclusive"),
        "{msg}"
    );
    assert!(
        streams().is_empty(),
        "nothing may play through the mixer unasked"
    );
    drop(strict);

    let lenient = rig(true, true, false);
    lenient.player.play(vec![f]);
    assert!(lenient.player.wait_until_idle(WAIT));
    let st = lenient.player.status();
    assert_eq!(st.state, PlayState::Finished);
    assert_eq!(streams()[0].lock().unwrap().mode, Some(OutputMode::Shared));
    // The honest label: not bit-perfect when the mixer is involved.
    let sp = st.signal_path.expect("signal path reported");
    assert!(!sp.is_bit_perfect());
}

#[test]
fn the_signal_path_of_an_exclusive_lossless_file_is_bit_perfect() {
    let Some(f) = fixture("sweep-24-96k.flac") else {
        return;
    };
    let r = rig(false, false, true); // gated, so we can look while it plays
    r.player.play(vec![f]);
    std::thread::sleep(Duration::from_millis(100));
    let st = r.player.status();
    let sp = st.signal_path.expect("signal path while playing");
    assert!(sp.is_bit_perfect() && sp.exclusive);
    assert_eq!(st.format, Some(StreamFormat::new(96_000, 24, 2)));
    r.player.stop();
    open_gate(&r.world);
}

#[test]
fn pause_holds_position_and_stop_resets() {
    let Some(f) = fixture("sweep-24-96k.flac") else {
        return;
    };
    let r = rig(false, false, true);
    *r.world.budget.lock().unwrap() = 96_000 * 2 * 2; // 2 s of audio
    r.player.play(vec![f]);
    std::thread::sleep(Duration::from_millis(300));
    r.player.pause();
    std::thread::sleep(Duration::from_millis(100));
    let a = r.player.status();
    std::thread::sleep(Duration::from_millis(150));
    let b = r.player.status();
    assert_eq!(a.state, PlayState::Paused);
    assert_eq!(
        a.position_frames, b.position_frames,
        "position must not move while paused"
    );
    r.player.stop();
    std::thread::sleep(Duration::from_millis(100));
    let c = r.player.status();
    assert_eq!((c.state, c.position_frames), (PlayState::Stopped, 0));
    open_gate(&r.world);
}
