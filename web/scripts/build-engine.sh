#!/bin/sh
# Builds the Table engine (bindings/wasm) to WebAssembly into public/engine, where the editor loads
# it from its own package. Needs wasm-pack, and a C compiler that targets wasm32 for SQLite (which
# is compiled into the engine): Homebrew's LLVM (`brew install llvm`), or CC_wasm32_unknown_unknown
# and AR_wasm32_unknown_unknown. Apple's clang cannot target wasm32.
set -eu
web="$(cd "$(dirname "$0")/.." && pwd)"
repo="$(cd "$web/.." && pwd)"
if [ -z "${CC_wasm32_unknown_unknown:-}" ]; then
  llvm="${HOMEBREW_PREFIX:-/opt/homebrew}/opt/llvm/bin"
  if [ -x "$llvm/clang" ]; then
    export CC_wasm32_unknown_unknown="$llvm/clang" AR_wasm32_unknown_unknown="$llvm/llvm-ar"
  elif ! clang --print-targets 2>/dev/null | grep -q wasm32; then
    echo "build-engine: SQLite needs a C compiler for wasm32: install LLVM (brew install llvm) or set CC_wasm32_unknown_unknown and AR_wasm32_unknown_unknown" >&2
    exit 1
  fi
fi
if toolchain="$(rustc --print sysroot 2>/dev/null)" && [ -d "$toolchain/lib" ]; then
  export DYLD_FALLBACK_LIBRARY_PATH="$toolchain/lib${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"
fi
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo/target}"
cd "$repo"
wasm-pack build bindings/wasm --target web --release --no-typescript --out-dir "$web/public/engine"
rm -f "$web/public/engine/.gitignore" "$web/public/engine/package.json"
