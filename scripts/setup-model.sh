#!/usr/bin/env bash
# Downloads the language model into models/ and checks its SHA-256: Qwen2.5-3B-Instruct, Q4_K_M
# (1.9 GB, Apache 2.0). The app copies it to the phone; the desktop tools serve it with llama-server.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
name="Qwen2.5-3B-Instruct-Q4_K_M.gguf"
sha="9c9f56a391a3abbd5b89d0245bf6106081bcc3173119d4229235dd9d23253f94"
url="https://huggingface.co/bartowski/Qwen2.5-3B-Instruct-GGUF/resolve/main/$name"
mkdir -p "$root/models"
file="$root/models/$name"
if [ -f "$file" ] && echo "$sha  $file" | sha256sum -c - >/dev/null 2>&1; then
  echo "$name is already in models/"
  exit 0
fi
echo "downloading $url (1.9 GB)"
curl -fL --retry 3 -C - -o "$file.part" "$url"
echo "$sha  $file.part" | sha256sum -c -
mv "$file.part" "$file"
echo "saved $file"
