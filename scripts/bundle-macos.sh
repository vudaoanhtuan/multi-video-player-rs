#!/bin/bash
# Builds "dist/Multi Video Player.app": one executable with FFmpeg inside.
# The app uses only libraries that are part of macOS. Open it with a double-click.
#
# Optional environment variables:
#   BUNDLE_ID                  default: local.multi-video-player
#   MACOSX_DEPLOYMENT_TARGET   oldest macOS that can run the app, default: 12.0
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="Multi Video Player"
EXECUTABLE="multi-video-player"
BUNDLE_ID="${BUNDLE_ID:-local.multi-video-player}"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"

"$ROOT/scripts/build-ffmpeg.sh"

# cargo reads .cargo/config.toml from the current folder. That file points
# cargo to the FFmpeg of this project.
cd "$ROOT"
cargo build --release
BINARY="$ROOT/target/release/$EXECUTABLE"

# The executable must use only libraries that are part of macOS.
EXTERNAL="$(otool -L "$BINARY" | tail -n +2 | awk '{print $1}' | grep -vE '^(/System/Library/|/usr/lib/)' || true)"
if [ -n "$EXTERNAL" ]; then
    echo "error: the executable needs libraries that are not part of macOS:" >&2
    echo "$EXTERNAL" >&2
    exit 1
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
APP="$ROOT/dist/$APP_NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/$EXECUTABLE"
strip -x "$APP/Contents/MacOS/$EXECUTABLE"

# The Finder icon: draw assets/app-icon/app-icon.svg at each size that macOS
# uses, then pack the pictures into one .icns file.
ICONSET="$ROOT/target/AppIcon.iconset"
rm -rf "$ICONSET"
cargo run --quiet --release --example app_iconset -- "$ICONSET"
iconutil --convert icns "$ICONSET" --output "$APP/Contents/Resources/AppIcon.icns"

cat >"$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>$APP_NAME</string>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleExecutable</key><string>$EXECUTABLE</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.video</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
    <key>NSDocumentsFolderUsageDescription</key><string>Multi Video Player reads the camera recordings in the folder that you open.</string>
    <key>NSDesktopFolderUsageDescription</key><string>Multi Video Player reads the camera recordings in the folder that you open.</string>
    <key>NSDownloadsFolderUsageDescription</key><string>Multi Video Player reads the camera recordings in the folder that you open.</string>
    <key>NSRemovableVolumesUsageDescription</key><string>Multi Video Player reads the camera recordings in the folder that you open.</string>
    <key>NSNetworkVolumesUsageDescription</key><string>Multi Video Player reads the camera recordings in the folder that you open.</string>
</dict>
</plist>
PLIST

# A local signature with no developer identity. It is sufficient to run the
# app on this Mac.
codesign --force --sign - "$APP"

echo
echo "Built: $APP ($(du -sh "$APP" | cut -f1))"
echo "Open it with: open \"$APP\""
