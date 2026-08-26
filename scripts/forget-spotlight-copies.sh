#!/usr/bin/env bash
# Drop every rterm.app except /Applications/rterm.app from Launch Services
# and from disk so Spotlight stops listing build copies.
set -euo pipefail

keep="/Applications/rterm.app"
lsregister="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"

forget() {
  local path="$1"
  [[ -n "$path" && "$path" != "$keep" ]] || return 0
  "$lsregister" -u "$path" 2>/dev/null || true
  # .noindex trees stay on disk; Spotlight ignores the suffix.
  if [[ "$path" == *".noindex"* ]]; then
    return 0
  fi
  if [[ -e "$path" || -L "$path" ]]; then
    rm -rf "$path"
  fi
}

shopt -s nullglob
while IFS= read -r path; do
  forget "$path"
done < <(mdfind 'kMDItemCFBundleIdentifier == "br.tec.lew.rterm"' || true)

forget "$HOME/.Trash/rterm.app"
forget "/Volumes/rterm/rterm.app"

for app in \
  "$HOME/Library/Developer/Xcode/DerivedData"/rterm-*/Build/Products/*/rterm.app \
  "$(cd "$(dirname "$0")/.." && pwd)"/build/DerivedData/Build/Products/*/rterm.app \
  "$(cd "$(dirname "$0")/.." && pwd)"/build/DerivedData.noindex/Build/Products/*/rterm.app
do
  forget "$app"
done

"$lsregister" -f "$keep" 2>/dev/null || true
