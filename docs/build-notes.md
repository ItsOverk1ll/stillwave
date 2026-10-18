# Build notes and spike findings

## Windows development machine

Set up during Phase 0: Node LTS and pnpm, JDK 17 (Temurin), Android SDK (platform 35, build-tools 35.0.0) and NDK 27.2.12479018, and the Rust targets for Android and `wasm32-unknown-unknown`. `ANDROID_HOME`, `NDK_HOME` and `JAVA_HOME` are set as user environment variables.

## Android (spike result)

- Rust cross-compiles to `aarch64-linux-android` locally, including bundled SQLite with the NDK clang. **Verified.**
- `pnpm tauri android build` then fails on Windows when it symlinks `libstillwave_lib.so` into `jniLibs`: creating symlinks needs Windows Developer Mode (or the symlink privilege). This is a system setting, so it was **not** changed. Enable Developer Mode to build APKs locally; CI builds them on Linux. CI (Linux) builds the debug-signed arm64 APK successfully (34 MB). **It has not been installed or run on any device.**
- `gen/android` is committed, except machine-specific generated files (`tauri.settings.gradle` contains an absolute path and is git-ignored; Tauri regenerates it).

## Linux arm64

CI uses GitHub's native `ubuntu-24.04-arm` runner rather than cross-compiling. It is free only for public repositories. If the repository were private, the fallbacks are paid minutes or cross-compiling with `cross`/QEMU. **Unverified until CI runs.**

## Fixtures

`scripts/gen_fixtures.py` writes WAV with the Python standard library only. FLAC and ALAC need `ffmpeg` on PATH; it is not installed on the development machine, so only WAV fixtures were generated and checked. CI that needs FLAC/ALAC must install ffmpeg.

## Icons

The app currently uses Tauri's default icons. A real icon set is still to do.

## First CI results (2026-10-18)

After fixing four problems found on the first run (a Linux-only doc lint, the Android SDK action, `NDK_HOME` via the wrong context, and iOS signing), CI and Build are green: Rust tests on Ubuntu and Windows, frontend, plugin SDK with a CI-signed example, Windows NSIS+MSI, Linux x64 deb/rpm/AppImage, Linux arm64 deb (native runner), Android APK, unsigned iOS ipa. Installers were not downloaded or run.
