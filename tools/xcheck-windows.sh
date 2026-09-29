#!/usr/bin/env bash
# Type-checks the Windows host from Linux/macOS (no linking, no MSVC needed).
#   tools/xcheck-windows.sh [extra cargo args]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
export CC_x86_64_pc_windows_msvc="$here/xcheck/fake-cl.sh"
export CXX_x86_64_pc_windows_msvc="$here/xcheck/fake-cl.sh"
export AR_x86_64_pc_windows_msvc="$(command -v llvm-lib || command -v llvm-lib-18)"
# tauri's generate_context! needs the UI build output to exist.
if [ ! -f "$root/ui/dist/index.html" ]; then
  mkdir -p "$root/ui/dist"
  echo '<!doctype html><title>Limbo</title>' > "$root/ui/dist/index.html"
fi
cd "$root"
cargo check --target x86_64-pc-windows-msvc -p limbo "$@"
