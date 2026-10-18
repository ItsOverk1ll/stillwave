// SPDX-License-Identifier: GPL-3.0-only
//! Playback glue: choosing an output device, building a [`Player`], and turning
//! its status into something the UI can show.

use serde::Serialize;
use stillwave_audio::{OutputMode, PlayState, Player, PlayerOptions, PlayerStatus, QualityBadge};

/// How audio reaches the device. Stored as a setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMode {
    /// Bit-perfect only: if the device refuses the exact format, playback fails.
    Exclusive,
    /// Bit-perfect when possible, otherwise through the Windows mixer (shown as processed).
    ExclusiveWithFallback,
    /// Always through the Windows mixer.
    Shared,
}

impl AudioMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "exclusive" => Some(Self::Exclusive),
            "exclusive_fallback" => Some(Self::ExclusiveWithFallback),
            "shared" => Some(Self::Shared),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exclusive => "exclusive",
            Self::ExclusiveWithFallback => "exclusive_fallback",
            Self::Shared => "shared",
        }
    }

    // Only the Windows player exists so far; other platforms return an error before this.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn options(self) -> PlayerOptions {
        PlayerOptions {
            mode: if self == Self::Shared {
                OutputMode::Shared
            } else {
                OutputMode::Exclusive
            },
            allow_shared_fallback: self == Self::ExclusiveWithFallback,
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// What is shown for a queue entry (and for the track being heard).
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntry {
    pub item_id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
}

// Each flag is an independent fact shown to the user.
#[allow(clippy::struct_excessive_bools)]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalView {
    pub source: String,
    pub output: String,
    pub exclusive: bool,
    pub resampled: bool,
    pub dsp: bool,
    pub dithered: bool,
    pub software_volume: bool,
    pub lossy: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    /// `stopped`, `playing`, `paused`, `finished` or `error`.
    pub state: &'static str,
    pub index: Option<usize>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub now_playing: Option<QueueEntry>,
    /// `bit_perfect`, `processed` or `lossy` while a track is loaded.
    pub badge: Option<&'static str>,
    pub signal: Option<SignalView>,
    pub error: Option<String>,
    /// Matches `play_queue`'s version; when it changes the UI refetches the queue.
    pub queue_version: u64,
}

impl PlayerView {
    pub fn idle() -> Self {
        Self {
            state: "stopped",
            index: None,
            position_ms: 0,
            duration_ms: None,
            now_playing: None,
            badge: None,
            signal: None,
            error: None,
            queue_version: 0,
        }
    }

    pub fn from_status(st: &PlayerStatus, queue: &[QueueEntry], queue_version: u64) -> Self {
        let rate = st.format.map_or(0, |f| u64::from(f.sample_rate_hz));
        let ms = |frames: u64| (frames * 1000).checked_div(rate).unwrap_or(0);
        let signal = st.signal_path.as_ref();
        Self {
            state: match st.state {
                PlayState::Stopped => "stopped",
                PlayState::Playing => "playing",
                PlayState::Paused => "paused",
                PlayState::Finished => "finished",
                PlayState::Error => "error",
            },
            index: st.index,
            position_ms: ms(st.position_frames),
            duration_ms: st.total_frames.map(ms),
            now_playing: st.index.and_then(|i| queue.get(i).cloned()),
            badge: signal.map(|s| match s.badge() {
                QualityBadge::BitPerfect => "bit_perfect",
                QualityBadge::Processed => "processed",
                QualityBadge::Lossy => "lossy",
            }),
            signal: signal.map(|s| SignalView {
                source: s.source.to_string(),
                output: s.output.to_string(),
                exclusive: s.exclusive,
                resampled: s.resampled,
                dsp: s.dsp_active,
                dithered: s.dithered,
                software_volume: s.software_volume,
                lossy: s.lossy_source,
            }),
            error: st.error.clone(),
            queue_version,
        }
    }
}

#[cfg(windows)]
pub fn list_devices() -> Vec<AudioDevice> {
    stillwave_audio::wasapi::list_devices()
        .unwrap_or_default()
        .into_iter()
        .map(|d| AudioDevice {
            id: d.id,
            name: d.friendly_name,
            is_default: d.is_default,
        })
        .collect()
}

#[cfg(not(windows))]
pub fn list_devices() -> Vec<AudioDevice> {
    Vec::new()
}

/// Builds a player for the chosen device (`None` = system default).
#[cfg(windows)]
pub fn make_player(mode: AudioMode, device_id: Option<&str>) -> Result<Player, String> {
    use stillwave_audio::wasapi::{default_device, list_devices};
    let device = match device_id {
        Some(id) => list_devices()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|d| d.id == id)
            .ok_or("The chosen audio device is not available. Pick another in Settings.")?,
        None => default_device().map_err(|e| e.to_string())?,
    };
    Ok(Player::new(Box::new(device), mode.options()))
}

#[cfg(not(windows))]
pub fn make_player(_mode: AudioMode, _device_id: Option<&str>) -> Result<Player, String> {
    Err("Playback is only available on Windows in this pre-alpha.".into())
}
