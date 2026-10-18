#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
# Builds an UNSIGNED iPhone .ipa that SideStore can re-sign. macOS only.
# SPIKE: see docs/ios-unsigned-builds.md for what is proven and what is not.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/apps/stillwave"

pnpm install --frozen-lockfile

# The Xcode project is generated on macOS (Tauri cannot create it on Windows/Linux).
[ -d src-tauri/gen/apple ] || pnpm tauri ios init --ci

# Plain `xcodebuild` cannot be used: the project's build phase calls
# `tauri ios xcode-script`, which needs the options server that only a running
# `tauri ios build` provides. So Tauri drives xcodebuild. It calls `xcodebuild`
# by name, so a wrapper first on PATH adds command-line build settings (which a
# project cannot override) to switch code signing off.
mkdir -p "$root/build/shim" "$root/dist"
real_xcodebuild="$(command -v xcodebuild)"
cat > "$root/build/shim/xcodebuild" <<SHIM
#!/usr/bin/env bash
case " \$* " in
  *" -exportArchive "*) exec "$real_xcodebuild" "\$@" ;;
esac
exec "$real_xcodebuild" "\$@" CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY= DEVELOPMENT_TEAM=
SHIM
chmod +x "$root/build/shim/xcodebuild"
export PATH="$root/build/shim:$PATH"

# Tauri's own export step wants a signing identity and is allowed to fail; the
# unsigned .xcarchive it leaves behind is what we package.
pnpm tauri ios build --debug --target aarch64 || echo "tauri ios build stopped after archiving (export needs signing)"

archive="$(find src-tauri/gen/apple -name '*.xcarchive' -maxdepth 4 | head -n1)"
[ -n "$archive" ] || { echo "no .xcarchive was produced"; exit 1; }
app="$(find "$archive/Products/Applications" -maxdepth 1 -name '*.app' | head -n1)"

rm -rf "$root/build/ipa" && mkdir -p "$root/build/ipa/Payload"
cp -R "$app" "$root/build/ipa/Payload/"
rm -f "$root/dist/Stillwave-unsigned.ipa"
(cd "$root/build/ipa" && zip -qry "$root/dist/Stillwave-unsigned.ipa" Payload)
echo "wrote dist/Stillwave-unsigned.ipa"
plutil -p "$app/Info.plist" | grep -A2 UIBackgroundModes || echo "WARNING: UIBackgroundModes not found in Info.plist"
