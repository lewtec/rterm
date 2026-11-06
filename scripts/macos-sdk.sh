#!/usr/bin/env bash
# Darwin cross-links need an Apple SDK. GoReleaser points SDKROOT at this cache.
# On macOS the system SDK is enough.
set -euo pipefail

if [[ "$(uname)" == Darwin ]]; then
  exit 0
fi

root="${HOME}/.cache/rterm/MacOSX14.5.sdk"
if [[ -d "$root/System" || -d "$root/usr" ]]; then
  exit 0
fi

mkdir -p "${HOME}/.cache/rterm"
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
curl -fsSL -o "$tmp" \
  "https://github.com/joseluisq/macosx-sdks/releases/download/14.5/MacOSX14.5.sdk.tar.xz"
tar -C "${HOME}/.cache/rterm" -xJf "$tmp"

if [[ ! -d "$root" ]]; then
  found="$(find "${HOME}/.cache/rterm" -maxdepth 2 -type d -name 'MacOSX14*.sdk' -print -quit)"
  if [[ -n "$found" && "$found" != "$root" ]]; then
    mv "$found" "$root"
  fi
fi

test -d "$root"
