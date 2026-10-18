# Unsigned iPhone builds (spike)

**Status: not proven.** This was written on Windows, where iOS cannot be built. Everything below is the plan plus what is known; the first macOS CI run is the real test. Record the outcome here.

## Goal

An `.ipa` with no code signature that SideStore re-signs with the user's own Apple ID. The Apple Developer Programme is not required.

## Why it is not just `tauri ios build`

`tauri ios build` drives `xcodebuild` and then exports an archive, which expects a signing identity and provisioning profile. Without one it stops at the export step.

## Approach in `scripts/build-ios-unsigned.sh`

1. `tauri ios init` on macOS generates the Xcode project (`src-tauri/gen/apple`). It cannot run on Windows or Linux, so `gen/apple` is not committed.
2. `tauri ios build` builds the web assets and the Rust static library. Its export step is allowed to fail.
3. `xcodebuild archive` with `CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY=""` produces a `.xcarchive` containing an unsigned `.app`.
4. Copy the `.app` into `Payload/` and zip it as `Stillwave-unsigned.ipa`.

## First CI run findings (macOS 15, Xcode 16.4)

- `tauri ios init --ci` works on the runner and generates the project. **Proven.**
- Plain `xcodebuild archive` fails: the project's build phase runs `tauri ios xcode-script`, which needs `.tauri/cli-options-server.json` from a running `tauri ios build`. **Proven; this is the risk listed below.**
- `tauri ios build` fails with "Signing requires a development team" when no team is set.
- Current fix (untested): run `tauri ios build` and force signing off with an `XCODE_XCCONFIG_FILE` override; take the `.xcarchive` it leaves behind. See `scripts/build-ios-unsigned.sh`.

## Known risks

- The Xcode project's build phase runs `tauri ios xcode-script`, which may need options that only the `tauri ios build` process provides when invoked from plain `xcodebuild`. If so, build the Rust library first and point the project at it, or run the archive through `tauri ios build` with signing overrides.
- Free-account signing by SideStore expires after 7 days (SideStore refreshes it while the device and the SideStore server are reachable).

- Sideloaded apps cannot use some entitlements (for example certain background or push capabilities). Background audio is expected to work; confirm on a device in Phase 2.
- The plugin host cannot use wasmtime's JIT on iOS; see plugins.md.

## Fallbacks if archiving without signing fails

1. Sign with an ad-hoc identity in CI (`CODE_SIGN_IDENTITY="-"`), then strip the signature (`rm -rf Payload/*.app/_CodeSignature`, `codesign --remove-signature`) before zipping.
2. Use `ldid` (as AltStore tooling does) to produce a fakesigned binary.

## To record after the first CI run

- Did `xcodebuild archive` succeed unsigned? Which fallback, if any, was needed?
- Does `plutil -p Payload/Stillwave.app/Info.plist` show `UIBackgroundModes`?
- Does SideStore install it on a real device?
