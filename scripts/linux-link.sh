#!/usr/bin/env bash
# conda-forge gcc appends -rpath $PREFIX/lib in its specs file. That path is
# the build machine. Rebuild link_command without it, then call the real driver.
set -euo pipefail

cc="${CC:-cc}"
case "$cc" in
  *linux-link.sh) cc=cc ;;
esac

spec=$("$cc" -print-file-name=conda.specs)
if [[ ! -f "$spec" ]]; then
  echo "linux-link: $cc has no conda.specs ($spec)" >&2
  exit 1
fi

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

awk '
  /^\*link_command:/ { keep=1 }
  keep && /^\*/ && !/^\*link_command:/ { exit }
  keep { print }
' "$spec" | sed -E 's/ %\{!static:-rpath [^ ]+ -rpath-link [^ ]+\}//' > "$tmp"

if grep -q rpath "$tmp"; then
  echo "linux-link: rpath clause remains in $spec" >&2
  exit 1
fi

"$cc" -specs="$tmp" "$@"
