# ADR 0007: Plugins: signed WebAssembly with permissions

Status: accepted (Phase 0)

## Context

Optional and risky features must not bloat or endanger the core, and plugins must never touch playback.

## Decision

Plugins are WASM modules with a TOML permissions manifest, signed with ed25519 (official tier needs the Stillwave key). wasmtime runs them at full speed on desktop and Android; iOS needs an interpreter (wasmtime's Pulley is the candidate, unverified). Fuel and memory limits apply. See docs/plugins.md.

## Consequences

Strong sandbox and clear consent. The `wasmtime-host` cargo feature keeps the JIT out of the iOS build.
