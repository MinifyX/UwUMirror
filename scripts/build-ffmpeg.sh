#!/usr/bin/env bash
# Builds the smallest FFmpeg that plays AirPlay's sound, for the Windows and
# macOS downloads to carry, so nobody has to install FFmpeg themselves:
#
#   scripts/build-ffmpeg.sh windows-x64      in MSYS2's MINGW64 shell
#   scripts/build-ffmpeg.sh windows-arm64    in MSYS2's CLANGARM64 shell
#   scripts/build-ffmpeg.sh macos-universal  on a Mac with Xcode's tools
#
# Only libavcodec and libavutil, with three decoders: AAC (AAC-LC and the
# AAC-ELD of screen mirroring) and ALAC. Everything else is switched off, which
# takes libavcodec from 90 MB to about one. It is FFmpeg's own source from
# ffmpeg.org, checked by hash, under the LGPL 2.1: no GPL or non-free parts are
# enabled. The libraries land in apps/desktop/src-tauri/resources/ffmpeg/,
# which the app looks into before the system (crates/uwumirror-core/src/decode.rs).
#
# Linux needs none of this: the packages depend on the distribution's FFmpeg.

set -euo pipefail

FFMPEG_VERSION=9.0.2
FFMPEG_SHA256=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e

platform=${1:?usage: build-ffmpeg.sh windows-x64|windows-arm64|macos-universal}
root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/apps/desktop/src-tauri/resources/ffmpeg"
work="$root/target/ffmpeg"
mkdir -p "$work" "$out"

tarball="$work/ffmpeg-$FFMPEG_VERSION.tar.xz"
if [ ! -f "$tarball" ]; then
  curl -fsSL -o "$tarball.part" "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz"
  mv "$tarball.part" "$tarball"
fi
echo "$FFMPEG_SHA256  $tarball" | sha256sum -c - 2>/dev/null ||
  echo "$FFMPEG_SHA256  $tarball" | shasum -a 256 -c -

# What every build shares: nothing but the two libraries and three decoders,
# and nothing found on the build machine by accident.
common=(
  --disable-everything --disable-autodetect --disable-programs --disable-doc
  --disable-avdevice --disable-avformat --disable-avfilter --disable-swscale
  --disable-swresample --disable-network
  --enable-shared --disable-static
  --enable-decoder=aac,alac
)

# build <name> <configure arguments…>: one architecture, into $work/<name>.
build() {
  local name=$1
  shift
  local src="$work/src-$name" prefix="$work/$name"
  rm -rf "$src" "$prefix"
  mkdir -p "$src"
  tar -xJf "$tarball" -C "$src" --strip-components=1
  (cd "$src" && ./configure --prefix="$prefix" "${common[@]}" "$@" && make -j"$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)" && make install)
}

rm -f "$out"/*.dll "$out"/*.dylib

case "$platform" in
  windows-x64 | windows-arm64)
    # FFmpeg's own threads on Windows' API, and (with GCC) libgcc and
    # MinGW's winpthread, which GCC's runtime wants anyway, linked in: the
    # DLLs need nothing but what every Windows has. ARM is built by clang
    # (MSYS2 has no GCC for it), which links its runtime in by itself.
    if [ "$platform" = windows-x64 ]; then
      extra=(--extra-ldflags=-static-libgcc "--extra-libs=-Wl,-Bstatic -lwinpthread -Wl,-Bdynamic")
    else
      extra=(--cc=clang --arch=aarch64)
    fi
    build "$platform" --target-os=mingw32 --enable-w32threads --disable-pthreads "${extra[@]}"
    cp "$work/$platform"/bin/avcodec-*.dll "$work/$platform"/bin/avutil-*.dll "$out/"
    # Fail here rather than on someone's computer: only system DLLs.
    objdump=$(command -v objdump || command -v llvm-objdump)
    for dll in "$out"/*.dll; do
      "$objdump" -p "$dll" | sed -n 's/^\s*DLL Name: //p' | while read -r dep; do
        case "${dep,,}" in
          avutil-*.dll | kernel32.dll | msvcrt.dll | ucrtbase.dll | api-ms-win-*.dll | bcrypt.dll | advapi32.dll | user32.dll | ole32.dll | shell32.dll) ;;
          *)
            echo "$dll needs $dep, which Windows doesn't have" >&2
            exit 1
            ;;
        esac
      done
    done
    ;;
  macos-universal)
    # Both halves on one Mac, joined. @loader_path: libavcodec finds the
    # libavutil next to it, wherever the app is.
    min=-mmacosx-version-min=11.0
    build arm64 --arch=arm64 --install-name-dir=@loader_path \
      --extra-cflags="$min" --extra-ldflags="$min"
    build x86_64 --arch=x86_64 --enable-cross-compile --target-os=darwin \
      --cc="clang -arch x86_64" --install-name-dir=@loader_path \
      --extra-cflags="$min" --extra-ldflags="$min" --disable-x86asm
    for lib in avcodec avutil; do
      # The versioned name the install name points at, e.g. libavcodec.63.dylib.
      name=$(cd "$work/arm64/lib" && ls lib$lib.[0-9]*.dylib | awk -F. 'NF == 3')
      lipo -create "$work/arm64/lib/$name" "$work/x86_64/lib/$name" -output "$out/$name"
    done
    ;;
  *)
    echo "unknown platform: $platform" >&2
    exit 1
    ;;
esac

ls -la "$out"
