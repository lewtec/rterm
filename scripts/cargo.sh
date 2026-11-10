#!/usr/bin/env bash
# Run cargo with a compiler rustc can actually invoke.
# On this host, `cc` is a shim. conda gcc from mise.toml is the compiler.
set -euo pipefail
if [[ "$(uname)" == Linux ]]; then
  export CC="${CC:-gcc}"
  export AR="${AR:-ar}"
  root="$(cd "$(dirname "$0")/.." && pwd)"
  # Local Linux links only. GoReleaser uses zig and must not inherit this.
  export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="${CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER:-$root/scripts/linux-link.sh}"
fi
exec cargo "$@"
