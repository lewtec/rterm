#!/usr/bin/env bash
# Build a Release .app and wrap it in artifacts/rterm-<version>.dmg.
# VERSION comes from the environment, or from an exact git tag.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

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

derived="$root/build/DerivedData"
stage="$root/build/dmg"
artifacts="$root/artifacts"
app_name="rterm"
dmg="$artifacts/${app_name}-${version}.dmg"

rm -rf "$stage"
mkdir -p "$stage" "$artifacts" "$derived"

xcodegen generate

xcodebuild \
  -project rterm.xcodeproj \
  -scheme rterm \
  -configuration Release \
  -destination 'generic/platform=macOS' \
  -derivedDataPath "$derived" \
  -skipPackagePluginValidation \
  MARKETING_VERSION="$version" \
  CURRENT_PROJECT_VERSION="$version" \
  CODE_SIGN_IDENTITY="-" \
  CODE_SIGNING_REQUIRED=NO \
  ARCHS="arm64 x86_64" \
  ONLY_ACTIVE_ARCH=NO \
  build

app="$derived/Build/Products/Release/${app_name}.app"
if [[ ! -d "$app" ]]; then
  echo "package-dmg: missing $app" >&2
  exit 1
fi

built_version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
if [[ "$built_version" != "$version" ]]; then
  echo "package-dmg: Info.plist version $built_version != $version" >&2
  exit 1
fi

cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"

rm -f "$dmg"
hdiutil create \
  -volname "$app_name" \
  -srcfolder "$stage" \
  -ov \
  -format UDZO \
  "$dmg"

echo "package-dmg: $dmg"
