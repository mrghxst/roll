#!/usr/bin/env bash
# Fetches the tools the macOS and Linux builds ship: ExifTool (Perl, which
# both systems have) and, on Linux, an LGPL static build of FFmpeg. macOS
# has no LGPL FFmpeg build to ship, so it uses Homebrew's.
# Versions and SHA-256 checksums are pinned; a mismatch stops the script.
# Output: apps/desktop/src-tauri/bundle-tools/{exiftool,ffmpeg}/ (not in git).
#
#   scripts/fetch-tools.sh
#   cd apps/desktop; pnpm tauri build --config src-tauri/tauri.bundle.linux.json   (or .macos.json)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)/apps/desktop/src-tauri/bundle-tools"
cache="$root/.download"
mkdir -p "$cache"

sum256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

fetch() { # url sha256 file
  local url=$1 sha=$2 file="$cache/$3"
  if [ ! -f "$file" ] || [ "$(sum256 "$file")" != "$sha" ]; then
    echo "Downloading $url"
    curl -fsSL -A 'curl/8.10' -o "$file" "$url"
  fi
  local got
  got=$(sum256 "$file")
  if [ "$got" != "$sha" ]; then
    echo "Checksum mismatch for $url: $got" >&2
    exit 1
  fi
}

# https://exiftool.org/checksums.txt
fetch https://sourceforge.net/projects/exiftool/files/Image-ExifTool-13.59.tar.gz/download \
  668ea3acececb7235fbd0f4900e72d5f12c9b07e5c778fd36cb1e9b5828fd65a exiftool.tar.gz
rm -rf "$root/exiftool" && mkdir -p "$root/exiftool"
tar -xzf "$cache/exiftool.tar.gz" -C "$cache"
cp -R "$cache/Image-ExifTool-13.59/exiftool" "$cache/Image-ExifTool-13.59/lib" "$root/exiftool/"
cp "$cache/Image-ExifTool-13.59/README" "$root/exiftool/README.txt"
chmod 755 "$root/exiftool/exiftool"

if [ "${ROLL_TOOLS_OS:-$(uname -s)}" = Linux ]; then
  # GitHub's digest of the release asset. BtbN prunes old autobuilds: when
  # this one is gone, pin a newer LGPL 8.x build.
  fetch https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-10-12-13-12/ffmpeg-n8.1.3-18-g46d899d7a0-linux64-lgpl-8.1.tar.xz \
    0734ef5b56a90adaa6b4999b02c4df603e1fd29a73067839620efc7706117517 ffmpeg.tar.xz
  rm -rf "$root/ffmpeg" && mkdir -p "$root/ffmpeg"
  tar -xJf "$cache/ffmpeg.tar.xz" -C "$cache"
  dir=$(find "$cache" -maxdepth 1 -type d -name 'ffmpeg-n8.1*-linux64-lgpl-8.1' | head -n1)
  cp "$dir/bin/ffmpeg" "$root/ffmpeg/ffmpeg"
  cp "$dir/LICENSE.txt" "$root/ffmpeg/LICENSE.txt"
  chmod 755 "$root/ffmpeg/ffmpeg"
fi

echo "Tools in $root"
