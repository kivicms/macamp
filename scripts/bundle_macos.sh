#!/usr/bin/env bash
# Builds a signed, self-contained MacAmp.app in dist/.
#
# MACAMP_UNIVERSAL=1        build arm64 + x86_64 into one universal binary
# MACAMP_SIGN_IDENTITY=...  Developer ID identity (default: ad-hoc "-")
# MACAMP_BUILD_NUMBER=...   CFBundleVersion override (default: git commit count)
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bundle_macos.sh only runs on macOS" >&2
  exit 1
fi

VERSION="$(awk -F'"' '/^version = "/ { print $2; exit }' Cargo.toml)"
if [[ -z "$VERSION" ]]; then
  echo "could not read version from Cargo.toml" >&2
  exit 1
fi
BUILD_NUMBER="${MACAMP_BUILD_NUMBER:-$(git rev-list --count HEAD 2>/dev/null || echo 1)}"
SIGN_IDENTITY="${MACAMP_SIGN_IDENTITY:--}"

if [[ "${MACAMP_UNIVERSAL:-0}" == "1" ]]; then
  if ! command -v rustup >/dev/null 2>&1; then
    echo "MACAMP_UNIVERSAL=1 requires rustup (rustup target add)" >&2
    exit 1
  fi
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  mkdir -p target/universal
  lipo -create \
    target/aarch64-apple-darwin/release/macamp \
    target/x86_64-apple-darwin/release/macamp \
    -output target/universal/MacAmp
  BINARY="target/universal/MacAmp"
  ARCH_LABEL="universal"
else
  cargo build --release
  BINARY="target/release/macamp"
  ARCH_LABEL="$(uname -m)"
fi

APP="dist/MacAmp.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

ditto "$BINARY" "$APP/Contents/MacOS/MacAmp"
chmod +x "$APP/Contents/MacOS/MacAmp"
ditto assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
ditto assets/fonts/OFL.txt "$APP/Contents/Resources/Silkscreen-OFL.txt"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>MacAmp</string>
  <key>CFBundleExecutable</key>
  <string>MacAmp</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundleIdentifier</key>
  <string>com.macamp.MacAmp</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>MacAmp</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${BUILD_NUMBER}</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.music</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

xattr -cr "$APP" 2>/dev/null || true
if [[ "$SIGN_IDENTITY" == "-" ]]; then
  codesign --force --deep --sign - "$APP"
else
  codesign --force --deep --options runtime --timestamp --sign "$SIGN_IDENTITY" "$APP"
fi
echo "Built $APP ($VERSION, $ARCH_LABEL, build $BUILD_NUMBER)"
