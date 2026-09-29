#!/usr/bin/env bash
# Stand-in C compiler for `cargo check --target x86_64-pc-windows-msvc` on
# Linux/macOS: build scripts (SQLite, etc.) need *an* object file, but `check`
# never links, so an empty object is enough. Preprocessor probes go to clang-cl.
set -e
CLANG_CL="${CLANG_CL:-$(command -v clang-cl || command -v clang-cl-18 || true)}"
for a in "$@"; do
  case "$a" in -E|/E|-EP|/EP) exec "$CLANG_CL" --target=x86_64-pc-windows-msvc "$@";; esac
done
out=""
for a in "$@"; do
  case "$a" in -Fo*) out="${a#-Fo}";; /Fo*) out="${a#/Fo}";; esac
done
if [ -z "$out" ]; then
  prev=""
  for a in "$@"; do [ "$prev" = "-o" ] && out="$a"; prev="$a"; done
fi
[ -z "$out" ] && exit 0
tmp="$(mktemp -d)"; echo "" > "$tmp/empty.c"
clang --target=x86_64-pc-windows-msvc -c "$tmp/empty.c" -o "$out"
rm -rf "$tmp"
