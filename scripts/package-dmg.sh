#!/usr/bin/env bash
# Build a Release .app and wrap it in artifacts/rterm-<version>.dmg.
# VERSION comes from the environment, or from an exact git tag.
# Runs on macOS. Linux releases ship the GoReleaser binary instead.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if [[ "$(uname)" != Darwin ]]; then
  echo "package-dmg: run this on macOS" >&2
  exit 1
fi

version="${VERSION:-}"
if [[ -z "$version" ]]; then
  if tag="$(git describe --tags --exact-match 2>/dev/null)"; then
    version="$tag"
  fi
fi
if [[ -z "$version" ]]; then
  echo "package-dmg: set VERSION or run on an exact git tag" >&2
  exit 1
fi

stage="$root/build/dmg"
artifacts="$root/artifacts"
app_name="rterm"
app="$stage/${app_name}.app"
dmg="$artifacts/${app_name}-${version}.dmg"

rm -rf "$stage"
mkdir -p "$stage" "$artifacts" "$app/Contents/MacOS" "$app/Contents/Resources"

export RTERM_VERSION="$version"
bash scripts/cargo.sh build --locked --release
cp "$root/target/release/rterm" "$app/Contents/MacOS/rterm"
chmod +x "$app/Contents/MacOS/rterm"

sed "s/__VERSION__/${version}/g" packaging/Info.plist > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"

iconset="$stage/AppIcon.iconset"
mkdir -p "$iconset"
src_icon="$root/Assets.xcassets/AppIcon.appiconset"
cp "$src_icon/icon_16x16.png" "$iconset/icon_16x16.png"
cp "$src_icon/icon_16x16@2x.png" "$iconset/icon_16x16@2x.png"
cp "$src_icon/icon_32x32.png" "$iconset/icon_32x32.png"
cp "$src_icon/icon_32x32@2x.png" "$iconset/icon_32x32@2x.png"
cp "$src_icon/icon_128x128.png" "$iconset/icon_128x128.png"
cp "$src_icon/icon_128x128@2x.png" "$iconset/icon_128x128@2x.png"
cp "$src_icon/icon_256x256.png" "$iconset/icon_256x256.png"
cp "$src_icon/icon_256x256@2x.png" "$iconset/icon_256x256@2x.png"
cp "$src_icon/icon_512x512.png" "$iconset/icon_512x512.png"
cp "$src_icon/icon_512x512@2x.png" "$iconset/icon_512x512@2x.png"
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"

ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create \
  -volname "$app_name" \
  -srcfolder "$stage" \
  -ov \
  -format UDZO \
  "$dmg"

echo "package-dmg: $dmg"
