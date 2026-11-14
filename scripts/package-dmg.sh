#!/usr/bin/env bash
# Build the universal rterm.app and wrap it in artifacts/rterm-<version>.dmg.
#
# Runs on Linux and macOS. The HFS+ volume, the UDIF container, and the Finder
# .DS_Store are written by packaging/dmg (pure Go), so there is no hdiutil or
# iconutil dependency. The mounted DMG opens a window with rterm.app on the
# left and an Applications drop link on the right.
#
# VERSION comes from the environment, or from an exact git tag.
#
#   scripts/package-dmg.sh              cross-build both Darwin slices, then pack
#   scripts/package-dmg.sh --from-dist  pack the slices GoReleaser left in dist/
#   scripts/package-dmg.sh --smoke      pack a stub app; tests the packager
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

mode="${1:-build}"
case "$mode" in
  build | --from-dist | --smoke) ;;
  *)
    echo "usage: $0 [--from-dist|--smoke]" >&2
    exit 2
    ;;
esac

version="${VERSION:-}"
if [[ "$mode" == "--smoke" ]]; then
  version="${version:-0.0.0-smoke}"
elif [[ -z "$version" ]]; then
  if tag="$(git describe --tags --exact-match 2>/dev/null)"; then
    version="$tag"
  fi
fi
if [[ -z "$version" ]]; then
  echo "package-dmg: set VERSION or run on an exact git tag" >&2
  exit 1
fi

artifacts="$root/artifacts"
stage="$root/build/dmg"
dmg="$artifacts/rterm-$version.dmg"
[[ "$mode" == "--smoke" ]] || mkdir -p "$artifacts"

arm64=""
amd64=""

if [[ "$mode" == "--smoke" ]]; then
  # The packager is exercised with a stub executable: the bundle and Finder
  # layout are what this mode validates, not the Rust binary. Keep the smoke
  # image out of artifacts/ so a release never picks it up.
  stage="$root/build/dmg-smoke"
  dmg="$root/build/rterm-$version.dmg"
  mkdir -p "$stage"
  stub="$root/build/rterm-stub"
  printf '#!/bin/sh\nexit 0\n' >"$stub"
  chmod +x "$stub"
  arm64="$stub"
elif [[ "$mode" == "--from-dist" ]]; then
  arm64="$root/dist/rterm_aarch64-apple-darwin/rterm"
  amd64="$root/dist/rterm_x86_64-apple-darwin/rterm"
  [[ -f "$arm64" ]] || arm64=""
  [[ -f "$amd64" ]] || amd64=""
  if [[ -z "$arm64" && -z "$amd64" ]]; then
    echo "package-dmg: no Darwin binaries in dist/; run goreleaser or drop --from-dist" >&2
    exit 1
  fi
else
  bash scripts/macos-sdk.sh
  export RTERM_VERSION="$version"
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    if [[ "$(uname)" == Darwin ]]; then
      cargo build --locked --release --target "$target"
    else
      SDKROOT="$HOME/.cache/rterm/MacOSX14.5.sdk" \
        MACOSX_DEPLOYMENT_TARGET=14.0 \
        cargo zigbuild --locked --release --target "$target"
    fi
  done
  arm64="$root/target/aarch64-apple-darwin/release/rterm"
  amd64="$root/target/x86_64-apple-darwin/release/rterm"
fi

flags=(
  -version "$version"
  -output "$dmg"
  -stage "$stage"
  -plist "$root/packaging/Info.plist"
  -icon "$root/packaging/icon.png"
)
[[ -n "$arm64" ]] && flags+=(-arm64 "$arm64")
[[ -n "$amd64" ]] && flags+=(-amd64 "$amd64")
[[ "$mode" == "--smoke" ]] && flags+=(-allow-non-macho)

if ! command -v go >/dev/null 2>&1; then
  echo "package-dmg: go not found; run 'mise run package' so mise puts Go on PATH" >&2
  exit 1
fi

go -C packaging/dmg run . "${flags[@]}"
echo "package-dmg: $dmg"
