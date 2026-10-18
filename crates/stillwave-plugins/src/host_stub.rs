// SPDX-License-Identifier: GPL-3.0-only
//! Stand-in used when the `wasmtime-host` feature is off (iOS).
//!
//! iOS forbids JIT, so wasmtime's Cranelift backend cannot run there. The
//! planned backend is wasmtime's Pulley interpreter (or another interpreter);
//! it must expose this same API. Until a spike confirms the choice, loading a
//! plugin reports [`PluginError::RuntimeUnavailable`]. See `docs/plugins.md`.

use crate::error::PluginError;
use crate::trust::VerifiedPlugin;

pub trait HostApi: Send {
    fn log(&mut self, plugin_id: &str, message: &str);
    fn library_track_count(&mut self) -> i64;
    fn create_playlist(&mut self, name: &str) -> bool;
}

/// Same fields as the wasmtime build, so callers compile unchanged.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub fuel_per_call: u64,
    pub max_memory_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            fuel_per_call: 50_000_000,
            max_memory_bytes: 64 * 1024 * 1024,
        }
    }
}

pub struct PluginHost;

impl PluginHost {
    pub fn new(_limits: Limits) -> Result<Self, PluginError> {
        Ok(Self)
    }

    pub fn load(
        &self,
        _plugin: &VerifiedPlugin,
        _api: Box<dyn HostApi>,
    ) -> Result<LoadedPlugin, PluginError> {
        Err(PluginError::RuntimeUnavailable)
    }
}

pub struct LoadedPlugin;

impl LoadedPlugin {
    pub fn call(&mut self, _export: &str) -> Result<i32, PluginError> {
        Err(PluginError::RuntimeUnavailable)
    }
}
