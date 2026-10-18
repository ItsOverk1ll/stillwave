// SPDX-License-Identifier: GPL-3.0-only
//! Playback engine: a queue of files, decoded on a worker thread and written to
//! an output stream. Platform-neutral; the device comes from an [`OutputDevice`].
//!
//! Gapless: the output stream buffers a few hundred milliseconds ahead, so when a
//! file ends the next one is opened and decoded while that buffer still plays.
//! If the next file has the same format the same stream carries on with no gap.
//! If the format differs the stream is drained and reopened (a short gap is
//! unavoidable without resampling, and resampling is never done silently).
//!
//! Nothing here changes samples: no gain, no resampling, no dither.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::format::StreamFormat;
use crate::signal_path::SignalPath;
use crate::symphonia_decoder::FileDecoder;
use crate::traits::{AudioError, Decoder, OutputDevice, OutputMode, OutputStream};

/// Frames decoded and written per step. Small enough for responsive pause/seek.
const CHUNK_FRAMES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Stopped,
    Playing,
    Paused,
    /// The whole queue played to the end.
    Finished,
    /// Playback stopped on an error; see [`PlayerStatus::error`].
    Error,
}

#[derive(Debug, Clone)]
pub struct PlayerOptions {
    pub mode: OutputMode,
    /// If the device refuses the file's exact format in exclusive mode, play
    /// through the shared (mixer) path instead of failing. The signal path then
    /// reports that the audio is processed. Off by default.
    pub allow_shared_fallback: bool,
}

impl Default for PlayerOptions {
    fn default() -> Self {
        Self {
            mode: OutputMode::Exclusive,
            allow_shared_fallback: false,
        }
    }
}

/// What the UI shows.
#[derive(Debug, Clone)]
pub struct PlayerStatus {
    pub state: PlayState,
    pub queue: Vec<PathBuf>,
    /// Index into `queue` of the track being heard.
    pub index: Option<usize>,
    pub position_frames: u64,
    pub total_frames: Option<u64>,
    pub format: Option<StreamFormat>,
    /// What happens between the file and the device for the track being heard.
    pub signal_path: Option<SignalPath>,
    /// Last problem, e.g. a file that could not be opened (it is skipped).
    pub error: Option<String>,
}

impl PlayerStatus {
    fn idle() -> Self {
        Self {
            state: PlayState::Stopped,
            queue: Vec::new(),
            index: None,
            position_frames: 0,
            total_frames: None,
            format: None,
            signal_path: None,
            error: None,
        }
    }
}

enum Cmd {
    Play(Vec<PathBuf>),
    Pause,
    Resume,
    Stop,
    Seek(u64),
    Next,
    Shutdown,
}

/// Handle to the playback worker.
pub struct Player {
    tx: Sender<Cmd>,
    status: Arc<Mutex<PlayerStatus>>,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    #[must_use]
    pub fn new(device: Box<dyn OutputDevice + Send>, options: PlayerOptions) -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(PlayerStatus::idle()));
        let st = Arc::clone(&status);
        let thread = std::thread::Builder::new()
            .name("stillwave-player".into())
            .spawn(move || Worker::new(device, options, st).run(&rx))
            .ok();
        Self { tx, status, thread }
    }

    /// Replaces the queue and starts playing from the first file.
    pub fn play(&self, queue: Vec<PathBuf>) {
        // Mark the state now, not when the worker gets to it, so a caller that
        // checks the status straight after `play` never sees the old state.
        if let Ok(mut s) = self.status.lock() {
            s.state = PlayState::Playing;
            s.error = None;
        }
        let _ = self.tx.send(Cmd::Play(queue));
    }
    pub fn pause(&self) {
        let _ = self.tx.send(Cmd::Pause);
    }
    pub fn resume(&self) {
        let _ = self.tx.send(Cmd::Resume);
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
    /// Seeks within the current track (frame position).
    pub fn seek(&self, frame: u64) {
        let _ = self.tx.send(Cmd::Seek(frame));
    }
    pub fn next(&self) {
        let _ = self.tx.send(Cmd::Next);
    }

    #[must_use]
    pub fn status(&self) -> PlayerStatus {
        self.status
            .lock()
            .map_or_else(|_| PlayerStatus::idle(), |s| s.clone())
    }

    /// Waits until playback finished, stopped or failed. Returns false on timeout.
    #[must_use]
    pub fn wait_until_idle(&self, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        loop {
            if matches!(
                self.status().state,
                PlayState::Finished | PlayState::Stopped | PlayState::Error
            ) {
                return true;
            }
            if Instant::now() > end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Where a track starts in the stream's running frame count.
struct Boundary {
    start_frame: u64,
    index: usize,
    /// Frame within the file that plays at `start_frame` (non-zero after a seek).
    offset_frames: u64,
    total_frames: Option<u64>,
    signal_path: SignalPath,
}

struct Playing {
    queue: Vec<PathBuf>,
    index: usize,
    decoder: FileDecoder,
    stream: Box<dyn OutputStream>,
    stream_format: StreamFormat,
    /// Frames written to `stream` that count as played-or-pending.
    written_frames: u64,
    boundaries: VecDeque<Boundary>,
    paused: bool,
    buf: Vec<i32>,
    /// Decoded samples the stream has not accepted yet, and how many are done.
    pending: usize,
    pending_pos: usize,
}

struct Worker {
    device: Box<dyn OutputDevice + Send>,
    options: PlayerOptions,
    status: Arc<Mutex<PlayerStatus>>,
    playing: Option<Playing>,
}

impl Worker {
    fn new(
        device: Box<dyn OutputDevice + Send>,
        options: PlayerOptions,
        status: Arc<Mutex<PlayerStatus>>,
    ) -> Self {
        Self {
            device,
            options,
            status,
            playing: None,
        }
    }

    fn run(mut self, rx: &Receiver<Cmd>) {
        loop {
            let cmd = if self.playing.is_some() {
                match rx.try_recv() {
                    Ok(c) => Some(c),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match rx.recv() {
                    Ok(c) => Some(c),
                    Err(_) => return,
                }
            };
            if let Some(cmd) = cmd {
                if !self.handle(cmd) {
                    return;
                }
            }
            if self.playing.is_some() {
                self.step();
            }
        }
    }

    /// Returns false to shut the worker down.
    fn handle(&mut self, cmd: Cmd) -> bool {
        match cmd {
            Cmd::Shutdown => return false,
            Cmd::Play(queue) => self.start(&queue),
            Cmd::Stop => {
                self.end(PlayState::Stopped, None);
            }
            Cmd::Pause => {
                if let Some(p) = &mut self.playing {
                    let _ = p.stream.pause();
                    p.paused = true;
                    self.publish(PlayState::Paused);
                }
            }
            Cmd::Resume => {
                if let Some(p) = &mut self.playing {
                    let _ = p.stream.resume();
                    p.paused = false;
                    self.publish(PlayState::Playing);
                }
            }
            Cmd::Seek(frame) => self.seek(frame),
            Cmd::Next => self.skip(),
        }
        true
    }

    fn start(&mut self, queue: &[PathBuf]) {
        self.end(PlayState::Stopped, None);
        let mut index = 0;
        let mut first_problem: Option<String> = None;
        while index < queue.len() {
            match self.begin_track(queue, index, &mut None) {
                Ok((p, _)) => {
                    self.playing = Some(p);
                    self.set_queue(queue);
                    self.publish(PlayState::Playing);
                    return;
                }
                Err(e) => {
                    self.note_error(queue, index, &e);
                    if first_problem.is_none() {
                        first_problem = Some(e.to_string());
                    }
                    index += 1;
                }
            }
        }
        self.set_queue(queue);
        // Say why: usually every track failed for the same reason (e.g. the
        // device refused the format), and that reason is what the user can act on.
        let why = first_problem.unwrap_or_else(|| "The queue is empty.".into());
        self.end(
            PlayState::Error,
            Some(format!("Nothing could be played. {why}")),
        );
    }

    /// Opens `queue[index]`, reusing `existing` stream if the format matches.
    fn begin_track(
        &mut self,
        queue: &[PathBuf],
        index: usize,
        existing: &mut Option<(Box<dyn OutputStream>, StreamFormat, u64)>,
    ) -> Result<(Playing, bool), AudioError> {
        // Open the file first: if it is unreadable, `existing` is left untouched
        // so the caller can try the next track on the same stream.
        let decoder = FileDecoder::open(&queue[index])?;
        let format = decoder.format();
        let reuse = existing.as_ref().is_some_and(|(_, f, _)| *f == format);
        let (stream, stream_format, written) = if reuse {
            existing
                .take()
                .ok_or_else(|| AudioError::Device("stream vanished".into()))?
        } else {
            if let Some((mut old, _, _)) = existing.take() {
                old.drain()?;
            }
            (self.open_stream(format)?, format, 0)
        };
        let signal_path = stream.signal_path(format, decoder.is_lossy());
        let mut boundaries = VecDeque::new();
        boundaries.push_back(Boundary {
            start_frame: written,
            index,
            offset_frames: 0,
            total_frames: decoder.total_frames(),
            signal_path,
        });
        let playing = Playing {
            queue: queue.to_vec(),
            index,
            decoder,
            stream,
            stream_format,
            written_frames: written,
            boundaries,
            paused: false,
            buf: vec![0; CHUNK_FRAMES * usize::from(format.channels)],
            pending: 0,
            pending_pos: 0,
        };
        Ok((playing, reuse))
    }

    fn open_stream(&self, format: StreamFormat) -> Result<Box<dyn OutputStream>, AudioError> {
        match self.device.open(format, self.options.mode) {
            Err(AudioError::Unsupported(_)) if self.options.allow_shared_fallback => {
                self.device.open(format, OutputMode::Shared)
            }
            other => other,
        }
    }

    /// One unit of work: decode a chunk and write it, or move to the next track.
    fn step(&mut self) {
        let Some(p) = &mut self.playing else { return };
        if p.paused {
            std::thread::sleep(Duration::from_millis(10));
            self.update_position();
            return;
        }
        let channels = usize::from(p.stream_format.channels);
        // First hand over what the stream could not take last time. When it is
        // full we return to the command loop instead of blocking, so pause,
        // seek and stop stay responsive.
        if p.pending_pos < p.pending {
            match p.stream.try_write(&p.buf[p.pending_pos..p.pending]) {
                Ok(0) => {
                    std::thread::sleep(Duration::from_millis(2));
                    self.update_position();
                }
                Ok(n) => {
                    p.pending_pos += n;
                    p.written_frames += (n / channels) as u64;
                    self.update_position();
                }
                Err(e) => self.end(PlayState::Error, Some(e.to_string())),
            }
            return;
        }
        match p.decoder.read(&mut p.buf) {
            Ok(0) => {
                self.advance();
            }
            Ok(n) => {
                p.pending = n;
                p.pending_pos = 0;
            }
            Err(e) => self.end(PlayState::Error, Some(e.to_string())),
        }
    }

    /// Current track ended: open the next one, on the same stream if possible.
    fn advance(&mut self) {
        let Some(p) = self.playing.take() else { return };
        let next = p.index + 1;
        self.open_following(p, next);
    }

    fn skip(&mut self) {
        let Some(mut p) = self.playing.take() else {
            return;
        };
        let _ = p.stream.flush();
        p.written_frames = p.stream.frames_played();
        let next = p.index + 1;
        self.open_following(p, next);
    }

    fn open_following(&mut self, p: Playing, mut next: usize) {
        let queue = p.queue.clone();
        let mut carry = Some((p.stream, p.stream_format, p.written_frames));
        let mut last_boundaries = p.boundaries;
        while next < queue.len() {
            match self.begin_track(&queue, next, &mut carry) {
                Ok((mut np, same_stream)) => {
                    // Keep earlier boundaries so the position stays right while
                    // the previous track's buffered audio is still playing.
                    if same_stream {
                        last_boundaries.extend(np.boundaries.drain(..));
                        np.boundaries = last_boundaries;
                    }
                    self.playing = Some(np);
                    self.update_position();
                    return;
                }
                Err(e) => {
                    self.note_error(&queue, next, &e);
                    next += 1;
                }
            }
        }
        // Past the end: let the buffered audio finish, then report Finished.
        if let Some((mut stream, _, _)) = carry {
            let _ = stream.drain();
        }
        self.end(PlayState::Finished, None);
    }

    fn seek(&mut self, frame: u64) {
        let Some(p) = &mut self.playing else { return };
        if let Err(e) = p.decoder.seek(frame) {
            self.set_error(Some(e.to_string()));
            return;
        }
        // Drop audio that was buffered ahead of the seek point.
        p.pending = 0;
        p.pending_pos = 0;
        let _ = p.stream.flush();
        p.written_frames = p.stream.frames_played();
        let sp = p.stream.signal_path(p.stream_format, p.decoder.is_lossy());
        p.boundaries.clear();
        p.boundaries.push_back(Boundary {
            start_frame: p.written_frames,
            index: p.index,
            offset_frames: frame,
            total_frames: p.decoder.total_frames(),
            signal_path: sp,
        });
        self.update_position();
    }

    /// Works out which track is being heard and how far into it we are.
    fn update_position(&mut self) {
        let Some(p) = &mut self.playing else { return };
        let played = p.stream.frames_played();
        while p.boundaries.len() > 1 && p.boundaries[1].start_frame <= played {
            p.boundaries.pop_front();
        }
        let Some(b) = p.boundaries.front() else {
            return;
        };
        let (index, position, total, sp) = (
            b.index,
            b.offset_frames + played.saturating_sub(b.start_frame),
            b.total_frames,
            b.signal_path.clone(),
        );
        let fmt = p.stream_format;
        let state = if p.paused {
            PlayState::Paused
        } else {
            PlayState::Playing
        };
        if let Ok(mut s) = self.status.lock() {
            s.state = state;
            s.index = Some(index);
            s.position_frames = position;
            s.total_frames = total;
            s.format = Some(fmt);
            s.signal_path = Some(sp);
        }
    }

    fn end(&mut self, state: PlayState, error: Option<String>) {
        if let Some(mut p) = self.playing.take() {
            if state == PlayState::Finished {
                let _ = p.stream.drain();
            } else {
                let _ = p.stream.flush();
            }
        }
        if let Ok(mut s) = self.status.lock() {
            s.state = state;
            if state == PlayState::Finished {
                if let Some(total) = s.total_frames {
                    s.position_frames = total;
                }
            } else if state != PlayState::Playing {
                s.position_frames = 0;
            }
            if error.is_some() {
                s.error = error;
            }
            if matches!(state, PlayState::Stopped) {
                s.index = None;
                s.format = None;
                s.signal_path = None;
                s.total_frames = None;
            }
        }
    }

    fn publish(&mut self, state: PlayState) {
        if let Ok(mut s) = self.status.lock() {
            s.state = state;
        }
        self.update_position();
    }

    fn set_queue(&self, queue: &[PathBuf]) {
        if let Ok(mut s) = self.status.lock() {
            s.queue = queue.to_vec();
        }
    }

    fn set_error(&self, e: Option<String>) {
        if let Ok(mut s) = self.status.lock() {
            s.error = e;
        }
    }

    fn note_error(&self, queue: &[PathBuf], index: usize, e: &AudioError) {
        self.set_error(Some(format!("{}: {e}", queue[index].display())));
    }
}
