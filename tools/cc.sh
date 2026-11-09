#!/bin/sh
# Linker/C-compiler shim. Uses the system C compiler when there is one;
# otherwise (inside the Craft Anything sandbox) falls back to `zig cc`.
if [ -z "$FORCE_ZIG" ] && command -v cc >/dev/null 2>&1; then exec cc "$@"; fi
ZIG="${ZIG:-}"
if [ -z "$ZIG" ]; then
  for d in "${XDG_DATA_HOME:-$HOME/.local/share}"/rust/zig-*/ "$HOME"/.var/app/ai.storyteller.craft-anything/data/rust/zig-*/; do
    [ -z "$ZIG" ] && [ -x "$d/zig" ] && ZIG="$d/zig"
  done
fi
command -v "$ZIG" >/dev/null 2>&1 || ZIG=zig
export ZIG_GLOBAL_CACHE_DIR="${ZIG_GLOBAL_CACHE_DIR:-${XDG_CACHE_HOME:-/tmp}/zig}"
export ZIG_LOCAL_CACHE_DIR="$ZIG_GLOBAL_CACHE_DIR"
for a in "$@"; do
  shift
  case "$a" in
    -lgcc_s) set -- "$@" -lunwind ;;
    -Wl,-O1|--target=*|-Wl,-melf_x86_64|-Wl,--no-undefined-version|-Wl,-znostart-stop-gc|-Wl,-z,nostart-stop-gc) ;;
    *) set -- "$@" "$a" ;;
  esac
done
exec "$ZIG" cc -target x86_64-linux-gnu.2.34 "$@"
