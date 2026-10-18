// SPDX-License-Identifier: GPL-3.0-only
//! WASM host (wasmtime). A plugin can import only the `stillwave` functions
//! below; every one that touches user data checks the manifest's permissions
//! first. Plugins never touch the audio path: no host function exposes it.
//!
//! Guest ABI (all `i32`, UTF-8 strings passed as pointer + length into guest memory):
//! - `stillwave.log(ptr, len)`                     no permission
//! - `stillwave.library_track_count() -> i64`      needs `library.read`
//! - `stillwave.playlist_create(ptr, len) -> i32`  needs `playlists.write`; 1 ok, 0 failed

use wasmtime::{Caller, Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

use crate::error::PluginError;
use crate::manifest::{Manifest, Permission};
use crate::trust::VerifiedPlugin;

/// What the app lets plugins do. Implemented by the app over the database;
/// only called after the permission check passes.
pub trait HostApi: Send {
    fn log(&mut self, plugin_id: &str, message: &str);
    fn library_track_count(&mut self) -> i64;
    fn create_playlist(&mut self, name: &str) -> bool;
}

/// Resource limits so a misbehaving plugin cannot hang or exhaust the app.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Wasm instructions (roughly) allowed per call.
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

/// Raised inside a host call; recovered at the call boundary as
/// [`PluginError::PermissionDenied`].
#[derive(Debug)]
struct Denied(Permission);

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "permission denied: {}", self.0)
    }
}
impl std::error::Error for Denied {}

struct State {
    manifest: Manifest,
    api: Box<dyn HostApi>,
    limits: StoreLimits,
}

fn require(caller: &Caller<'_, State>, p: Permission) -> wasmtime::Result<()> {
    if caller.data().manifest.has(p) {
        Ok(())
    } else {
        Err(wasmtime::Error::new(Denied(p)))
    }
}

fn read_str(caller: &mut Caller<'_, State>, ptr: i32, len: i32) -> wasmtime::Result<String> {
    let memory = caller
        .get_export("memory")
        .and_then(wasmtime::Extern::into_memory)
        .ok_or_else(|| wasmtime::format_err!("plugin exports no memory"))?;
    let (ptr, len) = (
        usize::try_from(ptr).map_err(|_| wasmtime::format_err!("bad pointer"))?,
        usize::try_from(len).map_err(|_| wasmtime::format_err!("bad length"))?,
    );
    if len > 64 * 1024 {
        return Err(wasmtime::format_err!("string too long"));
    }
    let bytes = memory
        .data(&*caller)
        .get(
            ptr..ptr
                .checked_add(len)
                .ok_or_else(|| wasmtime::format_err!("bad range"))?,
        )
        .ok_or_else(|| wasmtime::format_err!("string outside plugin memory"))?;
    String::from_utf8(bytes.to_vec()).map_err(|_| wasmtime::format_err!("string is not UTF-8"))
}

/// Creates sandboxed plugin instances.
pub struct PluginHost {
    engine: Engine,
    limits: Limits,
}

impl PluginHost {
    pub fn new(limits: Limits) -> Result<Self, PluginError> {
        let mut cfg = Config::new();
        cfg.consume_fuel(true);
        let engine = Engine::new(&cfg).map_err(|e| PluginError::Runtime(e.to_string()))?;
        Ok(Self { engine, limits })
    }

    /// Instantiates a verified plugin. Fails if it imports anything the host
    /// does not provide.
    pub fn load(
        &self,
        plugin: &VerifiedPlugin,
        api: Box<dyn HostApi>,
    ) -> Result<LoadedPlugin, PluginError> {
        let rt = |e: wasmtime::Error| PluginError::Runtime(e.to_string());
        let module = Module::new(&self.engine, &plugin.wasm_bytes).map_err(rt)?;
        let mut linker: Linker<State> = Linker::new(&self.engine);

        linker
            .func_wrap(
                "stillwave",
                "log",
                |mut c: Caller<'_, State>, ptr: i32, len: i32| -> wasmtime::Result<()> {
                    let msg = read_str(&mut c, ptr, len)?;
                    let id = c.data().manifest.id.clone();
                    c.data_mut().api.log(&id, &msg);
                    Ok(())
                },
            )
            .map_err(rt)?;
        linker
            .func_wrap(
                "stillwave",
                "library_track_count",
                |mut c: Caller<'_, State>| -> wasmtime::Result<i64> {
                    require(&c, Permission::LibraryRead)?;
                    Ok(c.data_mut().api.library_track_count())
                },
            )
            .map_err(rt)?;
        linker
            .func_wrap(
                "stillwave",
                "playlist_create",
                |mut c: Caller<'_, State>, ptr: i32, len: i32| -> wasmtime::Result<i32> {
                    require(&c, Permission::PlaylistsWrite)?;
                    let name = read_str(&mut c, ptr, len)?;
                    Ok(i32::from(c.data_mut().api.create_playlist(&name)))
                },
            )
            .map_err(rt)?;

        let state = State {
            manifest: plugin.manifest.clone(),
            api,
            limits: StoreLimitsBuilder::new()
                .memory_size(self.limits.max_memory_bytes)
                .build(),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_fuel(self.limits.fuel_per_call).map_err(rt)?;
        let instance = linker.instantiate(&mut store, &module).map_err(rt)?;
        Ok(LoadedPlugin {
            store,
            instance,
            fuel_per_call: self.limits.fuel_per_call,
        })
    }
}

/// A running plugin instance.
pub struct LoadedPlugin {
    store: Store<State>,
    instance: wasmtime::Instance,
    fuel_per_call: u64,
}

impl LoadedPlugin {
    /// Calls an exported `() -> i32` function.
    pub fn call(&mut self, export: &str) -> Result<i32, PluginError> {
        self.store
            .set_fuel(self.fuel_per_call)
            .map_err(|e| PluginError::Runtime(e.to_string()))?;
        let func = self
            .instance
            .get_typed_func::<(), i32>(&mut self.store, export)
            .map_err(|e| PluginError::Runtime(format!("export `{export}`: {e}")))?;
        func.call(&mut self.store, ())
            .map_err(|e| match e.downcast_ref::<Denied>() {
                Some(Denied(p)) => PluginError::PermissionDenied(*p),
                None => PluginError::Runtime(e.to_string()),
            })
    }
}
