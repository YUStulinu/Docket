#!/usr/bin/env bash
# Downloads the pinned sources and data the native code builds from, checks their SHA-256, and
# unpacks them into native/third_party (ignored by git).
#   scripts/fetch-deps.sh            Leptonica, Tesseract, llama.cpp, the OCR models
#   scripts/fetch-deps.sh android    also the OpenCV Android SDK
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dest="$root/native/third_party"
mkdir -p "$dest/downloads" "$dest/tessdata"

fetch() { # url sha256 file
  local url="$1" sha="$2" file="$dest/downloads/$3"
  if [ ! -f "$file" ] || ! echo "$sha  $file" | sha256sum -c - >/dev/null 2>&1; then
    echo "downloading $url"
    curl -fL --retry 3 -o "$file.part" "$url"
    echo "$sha  $file.part" | sha256sum -c - >/dev/null || { echo "checksum mismatch: $url" >&2; rm -f "$file.part"; exit 1; }
    mv "$file.part" "$file"
  fi
}

unpack() { # file dir-inside-archive target
  local file="$dest/downloads/$1" target="$dest/$3"
  [ -d "$target" ] && return
  local tmp; tmp="$(mktemp -d)"
  case "$file" in
    *.zip) unzip -q "$file" -d "$tmp" ;;
    *) tar -xzf "$file" -C "$tmp" ;;
  esac
  mv "$tmp/$2" "$target"
  rm -rf "$tmp"
}

fetch https://github.com/DanBloomberg/leptonica/releases/download/1.87.0/leptonica-1.87.0.tar.gz \
  c73363397f96eb1295602bf44d708a994ad42046c791bf03ea0505d829bdb6a7 leptonica-1.87.0.tar.gz
unpack leptonica-1.87.0.tar.gz leptonica-1.87.0 leptonica

fetch https://github.com/tesseract-ocr/tesseract/archive/refs/tags/5.5.3.tar.gz \
  9218e62793116d42a9f6d14cd9348518b27f382096eea3d0f2d1a24616bb5884 tesseract-5.5.3.tar.gz
unpack tesseract-5.5.3.tar.gz tesseract-5.5.3 tesseract

fetch https://github.com/ggml-org/llama.cpp/archive/refs/tags/b11429.tar.gz \
  0487b2e58d0f87c50def234ebda5dd8c1ab6c7afbc440ebef0080203110db6d7 llama.cpp-b11429.tar.gz
unpack llama.cpp-b11429.tar.gz llama.cpp-b11429 llama.cpp

# Tesseract on Android needs Google's cpu_features (its ndk_compat library).
fetch https://github.com/google/cpu_features/archive/refs/tags/v0.11.0.tar.gz   ab2463f2d38fcaff1ce806be8e4c91333449931f5e02009d543b2569a3fa471a cpu_features-0.11.0.tar.gz
unpack cpu_features-0.11.0.tar.gz cpu_features-0.11.0 cpu_features

# Tesseract's LSTM models for Romanian and English ("fast": integer weights, the phone's choice).
tessdata=https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/87416418657359cb625c412a48b6e1d6d41c29bd
fetch "$tessdata/ron.traineddata" 9adfde6b51ba4b97efd10ea37c3070fd3fc2bad7815e81f5c3c198cd96216cc9 ron.traineddata
fetch "$tessdata/eng.traineddata" 7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2 eng.traineddata
cp "$dest/downloads/ron.traineddata" "$dest/downloads/eng.traineddata" "$dest/tessdata/"

# Open fonts for the synthetic documents (tools/): DejaVu Sans, Serif and Mono.
fetch https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-2.37.zip   7576310b219e04159d35ff61dd4a4ec4cdba4f35c00e002a136f00e96a908b0a dejavu-fonts-ttf-2.37.zip
unpack dejavu-fonts-ttf-2.37.zip dejavu-fonts-ttf-2.37/ttf fonts

if [ "${1:-}" = android ]; then
  fetch https://github.com/opencv/opencv/releases/download/4.12.0/opencv-4.12.0-android-sdk.zip \
    fd7f2332331b4eb8b67e55137281cfb16823c9399d90deb9cfa3476783b99e35 opencv-4.12.0-android-sdk.zip
  unpack opencv-4.12.0-android-sdk.zip OpenCV-android-sdk opencv-android
fi
echo "dependencies are in $dest"
