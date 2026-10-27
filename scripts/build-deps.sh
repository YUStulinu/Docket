#!/usr/bin/env bash
# Builds Leptonica and Tesseract as static libraries (no image codecs, no network, no training
# tools: OpenCV decodes the images and hands Tesseract raw pixels).
#   scripts/build-deps.sh host                 for this machine, into native/third_party/install/host
#   scripts/build-deps.sh android <abi>        with the NDK (arm64-v8a, x86_64), into .../android-<abi>
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
tp="$root/native/third_party"
target="${1:-host}"
abi="${2:-}"
[ -d "$tp/tesseract" ] || bash "$root/scripts/fetch-deps.sh"

common=(-DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON)
case "$target" in
  host)
    prefix="$tp/install/host"
    case "$(uname -s)" in
      MINGW* | MSYS* | CYGWIN*) gen=(-G "Visual Studio 17 2022" -A x64 -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL) ;;
      *) gen=(-G Ninja) ;;
    esac
    ;;
  android)
    [ -n "$abi" ] || { echo "usage: build-deps.sh android <abi>" >&2; exit 2; }
    sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
    ndk="${ANDROID_NDK_HOME:-$(ls -d "$sdk"/ndk/* 2>/dev/null | sort -V | tail -1)}"
    [ -d "$ndk" ] || { echo "set ANDROID_HOME (or ANDROID_NDK_HOME)" >&2; exit 1; }
    prefix="$tp/install/android-$abi"
    gen=(-G Ninja -DCMAKE_TOOLCHAIN_FILE="$ndk/build/cmake/android.toolchain.cmake" -DANDROID_ABI="$abi" -DANDROID_PLATFORM=android-26)
    ;;
  *) echo "unknown target $target" >&2; exit 2 ;;
esac

build() { # name source-dir extra-args...
  local name="$1" src="$2"
  shift 2
  local dir="$tp/build/$name-${target}${abi:+-$abi}"
  cmake -S "$src" -B "$dir" "${gen[@]}" "${common[@]}" -DCMAKE_INSTALL_PREFIX="$prefix" "$@"
  cmake --build "$dir" --config Release --parallel
  cmake --install "$dir" --config Release
}

build leptonica "$tp/leptonica" -DSW_BUILD=OFF -DBUILD_PROG=OFF -DENABLE_ZLIB=OFF -DENABLE_PNG=OFF -DENABLE_GIF=OFF \
  -DENABLE_JPEG=OFF -DENABLE_TIFF=OFF -DENABLE_WEBP=OFF -DENABLE_OPENJPEG=OFF
extra=()
if [ "$target" = android ]; then
  build cpu_features "$tp/cpu_features" -DBUILD_TESTING=OFF -DBUILD_EXECUTABLE=OFF -DENABLE_INSTALL=ON
  # Tesseract asks (by running a program, impossible when cross-compiling) whether Leptonica reads
  # TIFF: it does not.
  extra=(-DLEPT_TIFF_RESULT=1 -DLEPT_TIFF_RESULT__TRYRUN_OUTPUT= -DCpuFeaturesNdkCompat_DIR="$prefix/lib/cmake/CpuFeaturesNdkCompat")
fi
build tesseract "$tp/tesseract" -DSW_BUILD=OFF -DBUILD_TRAINING_TOOLS=OFF -DBUILD_TESTS=OFF -DDISABLE_ARCHIVE=ON \
  -DDISABLE_CURL=ON -DDISABLE_TIFF=ON -DGRAPHICS_DISABLED=ON -DOPENMP_BUILD=OFF -DENABLE_NATIVE=OFF \
  -DINSTALL_CONFIGS=OFF -DENABLE_CCACHE=OFF -DCMAKE_PREFIX_PATH="$prefix" -DLeptonica_DIR="$prefix/lib/cmake/leptonica" "${extra[@]}"
echo "installed into $prefix"
