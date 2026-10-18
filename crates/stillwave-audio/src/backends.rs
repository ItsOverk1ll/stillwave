// SPDX-License-Identifier: GPL-3.0-only
//! Per-platform output backends. Empty in Phase 0; each module is compiled only
//! on its platform so the structure exists and CI proves it builds everywhere.

#[cfg(windows)]
pub mod wasapi {
    //! WASAPI (exclusive + shared). Phase 1.
    pub const NAME: &str = "WASAPI";
}

#[cfg(target_os = "linux")]
pub mod alsa {
    //! `ALSA` direct / `PipeWire` (Linux, Raspberry Pi). Phase 1.
    pub const NAME: &str = "ALSA/PipeWire";
}

#[cfg(target_os = "android")]
pub mod aaudio {
    //! `AAudio` with bit-perfect USB on Android 14+. Phase 2.
    pub const NAME: &str = "AAudio";
}

#[cfg(target_os = "ios")]
pub mod ios {
    //! Native `AVAudioSession` through a Swift plugin. Phase 2.
    pub const NAME: &str = "AVAudioSession";
}

/// Name of the backend compiled for this platform, if any.
#[cfg(windows)]
pub const PLATFORM_BACKEND: Option<&str> = Some(wasapi::NAME);
#[cfg(target_os = "linux")]
pub const PLATFORM_BACKEND: Option<&str> = Some(alsa::NAME);
#[cfg(target_os = "android")]
pub const PLATFORM_BACKEND: Option<&str> = Some(aaudio::NAME);
#[cfg(target_os = "ios")]
pub const PLATFORM_BACKEND: Option<&str> = Some(ios::NAME);
#[cfg(not(any(windows, target_os = "linux", target_os = "android", target_os = "ios")))]
pub const PLATFORM_BACKEND: Option<&str> = None;
