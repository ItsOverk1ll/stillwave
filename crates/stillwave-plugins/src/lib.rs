// SPDX-License-Identifier: GPL-3.0-only
//! Plugin manifest, signature verification and the sandboxed WASM host.
//!
//! Flow: read a [`PluginPackage`] → [`trust::verify`] (signature, tier, app
//! version) → [`PluginHost::load`] → call exports. Plugins never touch the
//! playback audio path.

pub mod error;
pub mod manifest;
pub mod package;
pub mod trust;

#[cfg(feature = "wasmtime-host")]
mod host;
#[cfg(not(feature = "wasmtime-host"))]
#[path = "host_stub.rs"]
mod host;

pub use error::PluginError;
pub use host::{HostApi, Limits, LoadedPlugin, PluginHost};
pub use manifest::{Manifest, ManifestError, Permission, Tier};
pub use package::{PluginPackage, SignatureFile};
pub use trust::{verify, TrustedKeys, VerifiedPlugin};

/// Generates a fresh ed25519 signing key from the OS random source.
pub fn generate_signing_key() -> Result<ed25519_dalek::SigningKey, PluginError> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| PluginError::Io(format!("random source: {e}")))?;
    Ok(ed25519_dalek::SigningKey::from_bytes(&seed))
}
