#!/usr/bin/env bash
# Fetches a pinned llama.cpp release (llama-server and its libraries) into tools/llama.cpp, and
# checks its SHA-256. On the desktop the evaluation (tools/docket_tools/evaluate.py) and the docket
# CLI use it; on the phone llama.cpp is built into the app from source instead.
#
#   scripts/setup-llama.sh            the CPU build (runs everywhere, slower)
#   scripts/setup-llama.sh vulkan     GPU through Vulkan (NVIDIA, AMD, Intel; Windows and Linux)
#   scripts/setup-llama.sh cuda       NVIDIA through CUDA 12.4 (Windows; includes the CUDA runtime)
#
# A llama-server already on the PATH works too (or point DOCKET_LLAMA_SERVER at one).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dest="$root/tools/llama.cpp"
tag="b11429"
variant="${1:-cpu}"
base="https://github.com/ggml-org/llama.cpp/releases/download/$tag"

case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) os=win ;;
  Linux) os=linux ;;
  Darwin) os=mac ;;
  *) echo "unsupported system: $(uname -s)" >&2; exit 1 ;;
esac

# file sha256 pairs to download for each system and variant
files=()
case "$os-$variant" in
  win-cpu) files=("llama-$tag-bin-win-cpu-x64.zip 1283323272b04cd07905816a597a0da810918102de958f4ff6f7bbaa70ed2efe") ;;
  win-vulkan) files=("llama-$tag-bin-win-vulkan-x64.zip 1bfe78ad9168b79fa02bf67f6af9f5e17a966d824d77238517f7bef12ac73b36") ;;
  win-cuda) files=(
    "llama-$tag-bin-win-cuda-12.4-x64.zip dd6df685c1024e6aa55ca55ad35038692d5e284d4dc895e7a646c3245d3d14ff"
    "cudart-llama-bin-win-cuda-12.4-x64.zip 8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6"
  ) ;;
  linux-cpu) files=("llama-$tag-bin-ubuntu-x64.tar.gz f6d25dde8f51133143d1453da4fd5f73b145127177612a283bf7995957af3392") ;;
  linux-vulkan) files=("llama-$tag-bin-ubuntu-vulkan-x64.tar.gz 632c4e98feba2b94407a2130e3133e0c3aefb0ea1ab41337e926d8bfafdd0b74") ;;
  mac-cpu | mac-metal) files=("llama-$tag-bin-macos-arm64.tar.gz 740288ec6887be94280a5dfa25b5e23a78285cab104519e6c7e218904ee82459") ;;
  *) echo "no $variant build for $os (try cpu or vulkan)" >&2; exit 1 ;;
esac

exe="llama-server"
[ "$os" = win ] && exe="llama-server.exe"
if [ -f "$dest/$exe" ]; then
  echo "llama.cpp is already in $dest (delete it to fetch $tag $variant)"
  exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/out"
for entry in "${files[@]}"; do
  read -r name sha <<<"$entry"
  echo "downloading $name"
  curl -fL --progress-bar -o "$tmp/$name" "$base/$name"
  echo "$sha  $tmp/$name" | sha256sum -c -
  case "$name" in
    *.zip) unzip -q -o "$tmp/$name" -d "$tmp/out" ;;
    *) tar -xzf "$tmp/$name" -C "$tmp/out" ;;
  esac
done

# The archives keep the binaries in different folders: take the one that has llama-server.
server="$(find "$tmp/out" -name "$exe" -type f | head -n 1)"
[ -n "$server" ] || { echo "llama-server is not in the archive" >&2; exit 1; }
mkdir -p "$dest"
cp -a "$(dirname "$server")"/. "$dest/"
# The CUDA runtime comes as a second archive with the DLLs at its top level.
[ "$variant" = cuda ] && find "$tmp/out" -maxdepth 1 -name 'cu*.dll' -exec cp {} "$dest/" \;
chmod +x "$dest/$exe" 2>/dev/null || true
echo "llama.cpp $tag ($variant) is in $dest"
