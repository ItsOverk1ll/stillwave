# Plugins

Plugins are WebAssembly modules with a permissions manifest, signed with ed25519.
They never touch the playback audio path: no host function exposes it.

## Package

A package is a directory:

| File | Purpose |
| --- | --- |
| `manifest.toml` | id, name, version, author, description, `min_app_version`, `tier`, `permissions`, optional `[network] domains` |
| `plugin.wasm` | the module (`wasm32-unknown-unknown`, no WASI) |
| `signature.sig` | `signer = <public key hex>` and `signature = <hex>` |

Manifest is TOML (human-written, comments allowed, familiar from Cargo). Unknown
fields and unknown permissions are errors, so a typo can never silently grant or
drop a permission.

Permissions: `network` (plus an explicit domain list; bare lowercase hostnames only),
`microphone.clip`, `clipboard.read`, `files.read_selected`, `playlists.read`,
`playlists.write`, `library.read`.

## Signing and trust

The signature covers a domain-separated, length-prefixed encoding of the manifest
bytes and the wasm bytes (`stillwave-plugin-v1\0 ‖ len ‖ manifest ‖ len ‖ wasm`), so
neither can be changed or swapped independently, and a signature from another
purpose cannot be replayed. Checks, in order:

1. The signer key is in the app's trusted set (`official` or `community` list), else `UntrustedSigner`.
2. The signature verifies (strict ed25519), else `BadSignature`.
3. The manifest parses and validates.
4. `tier = "official"` requires an **official** key. A community key cannot claim it.
5. `min_app_version` is not newer than the app.

Only the signed bytes are ever executed. The trusted key lists come from the app
(official key is built in; community keys will come from the Phase 7 registry).

**Private keys are never committed.** `keys/`, `*.key` and `dist/` are git-ignored.

## Host sandbox

`stillwave-plugins` uses wasmtime on desktop and Android. A plugin can import only
the `stillwave` functions (`log`, `library_track_count`, `playlist_create`); any other
import fails to load. Each data-touching call checks the manifest first and traps
with `PermissionDenied` if the permission is missing, before the app code runs.
Calls are limited by instruction fuel (default 50M per call) and 64 MiB of memory.
Guest pointers are bounds-checked.

## iOS

iOS forbids JIT, so wasmtime's Cranelift backend cannot run there. The crate's
`wasmtime-host` feature is turned off for iOS and `host_stub.rs` provides the same
API, returning `RuntimeUnavailable`. The intended backend is wasmtime's **Pulley**
interpreter. **Unverified:** not yet built or run on iOS; Phase 2 must spike this.

## Tooling

```bash
# one-off, local dev key (git-ignored)
cargo run -p stillwave-plugins --bin stillwave-plugin-tool -- keygen dev

# build the example, sign it, verify it
python scripts/package_plugin.py plugin-sdk/examples/hello dist/hello keys/dev.key keys/dev.pub

# load it through the real host and call an export
cargo run -p stillwave-plugins --bin stillwave-plugin-tool -- run dist/hello keys/dev.pub hello
```

The SDK is in `plugin-sdk/` (MIT). It is a separate Cargo workspace so it builds for
`wasm32-unknown-unknown` without pulling in the app's dependencies.
