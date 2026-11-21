#!/usr/bin/env bash
# Builds a drag-to-Applications DMG for macOS.
#
# MACAMP_UNIVERSAL=1        universal arm64 + x86_64 build
# MACAMP_SKIP_BUILD=1       reuse the existing dist/MacAmp.app
# MACAMP_SIGN_IDENTITY=...  Developer ID identity passed to bundle_macos.sh
# MACAMP_NOTARY_PROFILE=... notarytool keychain profile; notarizes and staples
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "package_dmg.sh only runs on macOS" >&2
  exit 1
fi

VERSION="$(awk -F'"' '/^version = "/ { print $2; exit }' Cargo.toml)"
if [[ -z "$VERSION" ]]; then
  echo "could not read version from Cargo.toml" >&2
  exit 1
fi

if [[ "${MACAMP_SKIP_BUILD:-0}" != "1" ]]; then
  ./scripts/bundle_macos.sh
fi

APP="dist/MacAmp.app"
if [[ ! -d "$APP" ]]; then
  echo "$APP not found; run without MACAMP_SKIP_BUILD=1" >&2
  exit 1
fi

if [[ "${MACAMP_UNIVERSAL:-0}" == "1" ]]; then
  ARCH_LABEL="universal"
else
  ARCH_LABEL="$(uname -m)"
fi
DMG_NAME="MacAmp-${VERSION}-${ARCH_LABEL}.dmg"
DMG="dist/${DMG_NAME}"
STAGE="dist/dmg-root"
MOUNT="$PWD/dist/.dmg-mount"

rm -rf "$STAGE" "$MOUNT"
mkdir -p "$STAGE" "$MOUNT"

ditto "$APP" "$STAGE/MacAmp.app"
ln -s /Applications "$STAGE/Applications"

cat > "$STAGE/Install MacAmp.command" <<'COMMAND'
#!/bin/bash
# Copies MacAmp to /Applications and removes the quarantine flag so that
# the ad-hoc signed build launches without the "damaged" Gatekeeper error.
set -euo pipefail
cd "$(dirname "$0")"

if [[ ! -d "MacAmp.app" ]]; then
  echo "MacAmp.app не найден рядом с этим скриптом." >&2
  exit 1
fi

DEST="/Applications/MacAmp.app"
echo "Устанавливаю MacAmp в $DEST ..."
if [[ -d "$DEST" ]]; then
  echo "Удаляю предыдущую версию."
  rm -rf "$DEST"
fi
/usr/bin/ditto "MacAmp.app" "$DEST"
xattr -dr com.apple.quarantine "$DEST" 2>/dev/null || true
echo "Готово. Открываю MacAmp."
open "$DEST"
COMMAND
chmod +x "$STAGE/Install MacAmp.command"

cat > "$STAGE/README.txt" <<EOF
MacAmp ${VERSION}
=============

Установка
---------
1. Перетащите MacAmp.app в папку Applications.
2. Откройте MacAmp из Applications.

Быстрый вариант: двойной клик по "Install MacAmp.command" (если macOS не даёт
запустить — правый клик по файлу → Открыть). Скрипт сам скопирует приложение
в Applications, снимет карантин и запустит его.

Если macOS пишет "MacAmp повреждено" / "не может быть открыто"
--------------------------------------------------------------
Приложение подписано ad-hoc и не нотаризовано Apple, поэтому скачанный из
интернета файл блокируется Gatekeeper. Снимите карантин в Терминале:

    xattr -dr com.apple.quarantine /Applications/MacAmp.app
    open /Applications/MacAmp.app

Это стандартная команда для неподписанных сборок; она не отключает защиту
системы и касается только MacAmp.

Данные приложения
-----------------
~/Library/Application Support/com.macamp.MacAmp/
EOF

rm -f "$DMG"
hdiutil create \
  -volname "MacAmp ${VERSION}" \
  -srcfolder "$STAGE" \
  -fs HFS+ \
  -format UDZO \
  -imagekey zlib-level=9 \
  -ov "$DMG" >/dev/null

cleanup() {
  if mount | grep -q "on ${MOUNT} "; then
    hdiutil detach "$MOUNT" -quiet >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

hdiutil attach "$DMG" -nobrowse -readonly -mountpoint "$MOUNT" >/dev/null
test -d "$MOUNT/MacAmp.app"
test -L "$MOUNT/Applications"
test -x "$MOUNT/Install MacAmp.command"
codesign --verify --deep --strict "$MOUNT/MacAmp.app"
hdiutil detach "$MOUNT" -quiet >/dev/null
trap - EXIT
rmdir "$MOUNT"

if [[ -n "${MACAMP_NOTARY_PROFILE:-}" ]]; then
  if [[ "${MACAMP_SIGN_IDENTITY:--}" == "-" ]]; then
    echo "notarization requires MACAMP_SIGN_IDENTITY (Developer ID)" >&2
    exit 1
  fi
  xcrun notarytool submit "$DMG" \
    --keychain-profile "$MACAMP_NOTARY_PROFILE" --wait
  xcrun stapler staple "$DMG"
  xcrun stapler validate "$DMG"
fi

(
  cd dist
  shasum -a 256 "$DMG_NAME" > "${DMG_NAME}.sha256"
)
echo "Built $DMG"
echo "SHA-256: $(cut -d' ' -f1 "dist/${DMG_NAME}.sha256")"
