#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Builds an UNSIGNED iPhone .ipa that SideStore can re-sign. macOS only.
# SPIKE: not yet proven. See docs/ios-unsigned-builds.md for the approach and fallbacks.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/apps/stillwave"

pnpm install --frozen-lockfile

# The Xcode project is generated on macOS (Tauri cannot create it on Windows/Linux).
[ -d src-tauri/gen/apple ] || pnpm tauri ios init --ci

# Let Tauri build the web assets and the Rust static library. Its own export step
# wants a signing identity, so a failure here is expected and tolerated; the
# archive below is what we ship.
pnpm tauri ios build --debug --target aarch64 || echo "tauri ios build stopped (expected without signing)"

proj="$(find src-tauri/gen/apple -maxdepth 1 -name '*.xcodeproj' | head -n1)"
scheme="$(basename "$proj" .xcodeproj)_iOS"
xcodebuild archive \
  -project "$proj" -scheme "$scheme" -configuration debug \
  -destination 'generic/platform=iOS' \
  -archivePath "$root/build/Stillwave.xcarchive" \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY=""

mkdir -p "$root/build/ipa/Payload" "$root/dist"
rm -rf "$root"/build/ipa/Payload/*.app
cp -R "$root"/build/Stillwave.xcarchive/Products/Applications/*.app "$root/build/ipa/Payload/"
(cd "$root/build/ipa" && zip -qry "$root/dist/Stillwave-unsigned.ipa" Payload)
echo "wrote dist/Stillwave-unsigned.ipa"
