#!/bin/bash
# Builds the FFmpeg that this project uses. The project does not use an
# FFmpeg of the system (for example from Homebrew).
#
# Results, in the folder ffmpeg-build/ of the project:
#   prefix/            static libraries and headers. The app links to them.
#                      Only what the app uses: the MKV reader, the common
#                      camera video decoders, VideoToolbox hardware decode,
#                      and the pixel format converter. No audio, no network,
#                      no external libraries. The license of this build is LGPL.
#   tools/bin/ffmpeg   a small ffmpeg command. The tests make their test
#                      videos with it. It is not part of the app.
#
# The script does nothing when the results are up to date. .cargo/config.toml
# points cargo to prefix/ (FFMPEG_DIR).
set -euo pipefail

FFMPEG_TAG="n7.1.5"
# The commit of the tag. The build stops if the download is a different commit.
FFMPEG_COMMIT="3a0867c2bfda4a4d4309ca1a8cbdc6175e67f587"
DECODERS="h264,hevc,mjpeg,mpeg4,mpeg2video,vp8,vp9,av1"
PARSERS="h264,hevc,mjpeg,mpeg4video,mpegvideo,vp8,vp9,av1"
HWACCELS="h264_videotoolbox,hevc_videotoolbox,mpeg4_videotoolbox,mpeg2_videotoolbox,vp9_videotoolbox,av1_videotoolbox"
# The oldest macOS that can run the app. The same value is in .cargo/config.toml.
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$ROOT/ffmpeg-build"
SRC="$WORK/src"
PREFIX="$WORK/prefix"
TOOL="$WORK/tools/bin/ffmpeg"
STAMP="$WORK/.built"
# Change the number when the configure options below change.
WANT="3 $FFMPEG_COMMIT $DECODERS $MACOSX_DEPLOYMENT_TARGET"

if [ -f "$STAMP" ] && [ "$(cat "$STAMP")" = "$WANT" ] && [ -f "$PREFIX/lib/libavcodec.a" ] && [ -x "$TOOL" ]; then
    echo "FFmpeg $FFMPEG_TAG is already built: $WORK"
    exit 0
fi

if [ "$(uname -m)" = "x86_64" ] && ! command -v nasm >/dev/null; then
    echo "error: an Intel Mac needs nasm to build FFmpeg. Install nasm, then run this script again." >&2
    exit 1
fi

mkdir -p "$WORK"
rm -f "$STAMP"
if [ "$(git -C "$SRC" rev-parse HEAD 2>/dev/null || true)" != "$FFMPEG_COMMIT" ]; then
    rm -rf "$SRC"
    echo "Downloading FFmpeg $FFMPEG_TAG..."
    git -c advice.detachedHead=false clone --quiet --depth 1 --branch "$FFMPEG_TAG" \
        https://github.com/FFmpeg/FFmpeg.git "$SRC"
    GOT="$(git -C "$SRC" rev-parse HEAD)"
    if [ "$GOT" != "$FFMPEG_COMMIT" ]; then
        echo "error: FFmpeg $FFMPEG_TAG is commit $GOT, expected $FFMPEG_COMMIT" >&2
        exit 1
    fi
fi

JOBS="$(sysctl -n hw.ncpu)"
# Runs make in the current folder. Shows the end of the log if it fails.
run_make() {
    local log="$1"
    shift
    make -j"$JOBS" "$@" >"$log" 2>&1 || {
        tail -30 "$log" >&2
        exit 1
    }
}

# 1. The libraries for the app.
echo "Building the FFmpeg libraries (about 1 minute)..."
rm -rf "$PREFIX" "$WORK/build-libs"
mkdir -p "$WORK/build-libs"
cd "$WORK/build-libs"
"$SRC/configure" \
    --prefix="$PREFIX" \
    --enable-static --disable-shared --enable-pic \
    --disable-programs --disable-doc --disable-debug \
    --disable-autodetect --disable-network \
    --disable-avdevice --disable-avfilter --disable-swresample --disable-postproc \
    --disable-everything \
    --enable-videotoolbox --enable-zlib \
    --enable-protocol=file \
    --enable-demuxer=matroska \
    --enable-decoder="$DECODERS" \
    --enable-parser="$PARSERS" \
    --enable-hwaccel="$HWACCELS" \
    >"$WORK/configure-libs.log"
run_make "$WORK/make-libs.log"
run_make "$WORK/make-libs-install.log" install

# 2. The ffmpeg command for the tests: a test picture source, the H.264
#    encoder of macOS, and the MKV writer.
echo "Building the ffmpeg command for the tests (about 1 minute)..."
rm -rf "$WORK/tools" "$WORK/build-tools"
mkdir -p "$WORK/build-tools" "$WORK/tools/bin"
cd "$WORK/build-tools"
"$SRC/configure" \
    --enable-static --disable-shared \
    --disable-ffplay --disable-ffprobe --disable-doc --disable-debug \
    --disable-autodetect --disable-network \
    --disable-everything \
    --enable-videotoolbox \
    --enable-indev=lavfi \
    --enable-filter=testsrc2,scale,format \
    --enable-decoder=wrapped_avframe,rawvideo \
    --enable-encoder=h264_videotoolbox,mpeg4 \
    --enable-muxer=matroska \
    --enable-protocol=file \
    >"$WORK/configure-tools.log"
run_make "$WORK/make-tools.log" ffmpeg
cp ffmpeg "$TOOL"

echo "$WANT" >"$STAMP"
echo "FFmpeg $FFMPEG_TAG is built: $WORK"
