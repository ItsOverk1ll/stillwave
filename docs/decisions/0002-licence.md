# ADR 0002: Licences: GPL-3.0, AGPL-3.0, MIT

Status: accepted (Phase 0)

## Context

The project is open source and wants improvements to the app and core to stay open, while letting anyone write plugins without licence friction.

## Decision

GPL-3.0-only for the apps and core crates, AGPL-3.0-only for the account server (so hosted forks share changes), MIT for the plugin SDK. **Pending final confirmation.** Licence files sit next to each part (`LICENSE`, `crates/`, `apps/`, `server/`, `plugin-sdk/`) and every source file carries an SPDX header, so changing is a mechanical edit.

## Consequences

Plugin authors can use any licence for their plugin. The Tauri app links GPL crates, so the whole app is GPL.
