// SPDX-License-Identifier: MIT
//! SDK for Stillwave plugins.
//!
//! A plugin is a WebAssembly module built for `wasm32-unknown-unknown` that
//! exports plain `extern "C" fn() -> i32` entry points. It can only call the
//! functions here, and each one that touches user data needs a permission
//! declared in the plugin's `manifest.toml`. A call without permission traps.
//!
//! On non-wasm targets the functions are harmless stand-ins so plugin logic can
//! be unit-tested with `cargo test`.

/// Result of asking the host to do something that can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostResult {
    Ok,
    Failed,
}

#[cfg(target_arch = "wasm32")]
mod ffi {
    #[link(wasm_import_module = "stillwave")]
    extern "C" {
        #[link_name = "log"]
        pub fn log(ptr: *const u8, len: i32);
        #[link_name = "library_track_count"]
        pub fn library_track_count() -> i64;
        #[link_name = "playlist_create"]
        pub fn playlist_create(ptr: *const u8, len: i32) -> i32;
    }
}

/// Writes a line to the app's plugin log. No permission needed.
pub fn log(message: &str) {
    #[cfg(target_arch = "wasm32")]
    // SAFETY: pointer and length describe a live `&str` in this module's memory.
    unsafe {
        ffi::log(message.as_ptr(), i32::try_from(message.len()).unwrap_or(i32::MAX));
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = message;
}

/// Number of tracks in the library. Needs `library.read`.
#[must_use]
pub fn library_track_count() -> i64 {
    #[cfg(target_arch = "wasm32")]
    // SAFETY: no pointers are passed.
    return unsafe { ffi::library_track_count() };
    #[cfg(not(target_arch = "wasm32"))]
    return 0;
}

/// Creates an empty playlist. Needs `playlists.write`.
#[must_use]
pub fn create_playlist(name: &str) -> HostResult {
    #[cfg(target_arch = "wasm32")]
    // SAFETY: pointer and length describe a live `&str` in this module's memory.
    let ok = unsafe { ffi::playlist_create(name.as_ptr(), i32::try_from(name.len()).unwrap_or(i32::MAX)) } == 1;
    #[cfg(not(target_arch = "wasm32"))]
    let ok = !name.is_empty();
    if ok {
        HostResult::Ok
    } else {
        HostResult::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stand_ins_work_off_wasm() {
        log("hello");
        assert_eq!(library_track_count(), 0);
        assert_eq!(create_playlist("x"), HostResult::Ok);
        assert_eq!(create_playlist(""), HostResult::Failed);
    }
}
