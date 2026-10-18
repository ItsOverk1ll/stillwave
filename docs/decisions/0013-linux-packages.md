# ADR 0013: Linux packaging

Status: accepted (Phase 0)

## Context

Linux users expect their distro's format.

## Decision

`.deb` (Debian, Ubuntu, Raspberry Pi OS arm64), `.rpm` (Fedora), an AUR PKGBUILD (draft in `packaging/arch/`), and AppImage or Flatpak as a catch-all.

## Consequences

Several formats to test. Tauri produces deb, rpm and AppImage from one config.
