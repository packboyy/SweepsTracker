#!/bin/sh
# Builds "Sweeps Tracker.app" in dist/ with the app icon.
#
#   scripts/bundle-mac.sh            # build dist/Sweeps Tracker.app
#   scripts/bundle-mac.sh --install  # ...move it to /Applications (no copy left in dist/)
#
# The icon comes from assets/icon.png (rebuild that from the artwork with
# scripts/make-icon.swift).
set -eu
cd "$(dirname "$0")/.."

NAME="Sweeps Tracker"
APP="dist/$NAME.app"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
# Build number: how many commits there are (the same number the app shows, from build.rs).
BUILD=$(git rev-list --count HEAD 2>/dev/null || echo 0)

cargo build --release

# Icon set: every size macOS uses, at 1x and 2x, packed into AppIcon.icns.
ICONSET=target/AppIcon.iconset
rm -rf "$ICONSET"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z $size $size assets/icon.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double assets/icon.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o target/AppIcon.icns

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/sweeps-tracker "$APP/Contents/MacOS/"
cp target/AppIcon.icns "$APP/Contents/Resources/"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$NAME</string>
    <key>CFBundleDisplayName</key><string>$NAME</string>
    <key>CFBundleIdentifier</key><string>com.andy.sweepstracker</string>
    <key>CFBundleExecutable</key><string>sweeps-tracker</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$BUILD</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.finance</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Ad-hoc signature so macOS runs it locally without complaints about a damaged app.
codesign --force --deep --sign - "$APP" >/dev/null
echo "Built $APP"

if [ "${1:-}" = "--install" ]; then
    rm -rf "/Applications/$NAME.app"
    cp -R "$APP" /Applications/
    # Remove the build copy so Spotlight / Launchpad don't list the app twice.
    rm -rf "$APP"
    rmdir dist 2>/dev/null || true
    echo "Installed /Applications/$NAME.app"
fi
