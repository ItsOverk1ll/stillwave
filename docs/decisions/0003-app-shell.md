# ADR 0003: App shell: Tauri 2

Status: accepted (Phase 0)

## Context

One codebase must target Windows, Linux, Raspberry Pi, Android and iPhone with a small footprint.

## Decision

Use Tauri 2 for desktop and mobile.

## Consequences

Small binaries and memory (about 40 MB resident for the Phase 0 shell on Windows). UI runs in the system WebView, which differs per platform and must be tested on each. Native audio is done in Rust and Swift/Kotlin plugins, not the WebView.
