// SPDX-License-Identifier: MIT
//! "Hello" example plugin: logs a line and reports the library size.

use stillwave_plugin_sdk as sdk;

/// Entry point the host calls. Returns 42 so the host can tell it ran.
#[no_mangle]
pub extern "C" fn hello() -> i32 {
    sdk::log("hello from a Stillwave plugin");
    42
}

/// Needs `library.read` (declared in manifest.toml).
#[no_mangle]
pub extern "C" fn track_count() -> i32 {
    i32::try_from(sdk::library_track_count()).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    #[test]
    fn hello_returns_42() {
        assert_eq!(super::hello(), 42);
    }
}
