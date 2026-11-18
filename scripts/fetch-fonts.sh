#!/usr/bin/env bash
# Downloads the fonts the Table kit builds with (docs/development.md, "Fonts") from their upstream
# sources, checks each file's SHA-256, and lays them out as KITION_FONTS_DIR expects:
#
#   scripts/fetch-fonts.sh <folder>
#   export KITION_FONTS_DIR=<folder>
#
# A file already there with the right SHA-256 is kept. Needs curl, base64 and shasum or sha256sum.
set -euo pipefail
dir="${1:?usage: scripts/fetch-fonts.sh <folder>}"

google=https://github.com/google/fonts/raw/45e3d5269e4a17d823f6d84470bc084dc5ae46f6
emoji=https://github.com/googlefonts/noto-emoji/raw/1ffdd21391dd1f25c081fa93a9dea0c7c029442b
# googlesource serves a file's bytes base64-encoded with ?format=TEXT.
android=https://android.googlesource.com/platform/frameworks/base/+/cac6285b48e88e118f69992d5c97d56163b6274c/data/fonts

# <path under the folder> <sha256> <url> [base64]
files=(
  "fonts/inter/Inter-Variable.ttf 29160a80ff49ddcab2c97711247e08b1fab27a484a329ce8b813d820dc559031 $google/ofl/inter/Inter%5Bopsz%2Cwght%5D.ttf"
  "fonts/inter/OFL.txt 5b9321a4298cfeb6b34354164a1c3afc3db114569984c502b9b35d988fd58c57 $google/ofl/inter/OFL.txt"
  "fonts/droid-sans-fallback/DroidSansFallbackFull.ttf acb6440a713d880a13a21b468ba7cd43f5a2b2934972e51be791c880730777b8 $android/DroidSansFallbackFull.ttf?format=TEXT base64"
  "fonts/droid-sans-fallback/NOTICE 38751245389e1e23f73e6f5384b5cbe7fa972cc4410c5adc9c04b082a0b9561a $android/NOTICE?format=TEXT base64"
  "fonts/noto-color-emoji/Noto-COLRv1.ttf b8e25ea68db82f9e4d0aee921f4420be2be39887bd5c893a2ad98710531f9d0c $emoji/2D/fonts/Noto-COLRv1.ttf"
  "fonts/noto-color-emoji/OFL.txt 6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2 $emoji/2D/fonts/LICENSE"
)

# From standard input, so no path is printed (Git Bash's sha256sum escapes a path with backslashes).
sum() { if command -v sha256sum >/dev/null; then sha256sum < "$1"; else shasum -a 256 < "$1"; fi | cut -d ' ' -f 1; }

for entry in "${files[@]}"; do
  read -r path sha url encoding <<<"$entry"
  target="$dir/$path"
  if [[ -f "$target" && "$(sum "$target")" == "$sha" ]]; then
    continue
  fi
  mkdir -p "$(dirname "$target")"
  if [[ "${encoding:-}" == base64 ]]; then
    curl -fsSL "$url" | base64 -d > "$target.part"
  else
    curl -fsSL -o "$target.part" "$url"
  fi
  got="$(sum "$target.part")"
  if [[ "$got" != "$sha" ]]; then
    rm -f "$target.part"
    echo "error: $path from $url has SHA-256 $got, expected $sha" >&2
    exit 1
  fi
  mv "$target.part" "$target"
  echo "fetched $path"
done
echo "fonts in $dir: export KITION_FONTS_DIR=$dir"
