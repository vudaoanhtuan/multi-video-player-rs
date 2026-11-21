#!/bin/bash
# Builds a small static FFmpeg for the app bundle.
#
# The result is in target/ffmpeg-static/prefix. It holds only what the app
# uses: the MKV reader, the common camera video decoders, VideoToolbox
# hardware decode, and the pixel format converter. No audio, no network,
# no external libraries. The license of this build is LGPL.
set -euo pipefail

FFMPEG_TAG="n7.1.5"
# The commit of the tag. The build stops if the download is a different commit.
FFMPEG_COMMIT="3a0867c2bfda4a4d4309ca1a8cbdc6175e67f587"
DECODERS="h264,hevc,mjpeg,mpeg4,mpeg2video,vp8,vp9,av1"
PARSERS="h264,hevc,mjpeg,mpeg4video,mpegvideo,vp8,vp9,av1"
HWACCELS="h264_videotoolbox,hevc_videotoolbox,mpeg4_videotoolbox,mpeg2_videotoolbox,vp9_videotoolbox,av1_videotoolbox"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$ROOT/target/ffmpeg-static"
PREFIX="$WORK/prefix"
STAMP="$PREFIX/.built"
WANT="$FFMPEG_COMMIT $DECODERS ${MACOSX_DEPLOYMENT_TARGET:-default}"

if [ -f "$STAMP" ] && [ "$(cat "$STAMP")" = "$WANT" ]; then
    echo "FFmpeg $FFMPEG_TAG is already built: $PREFIX"
    exit 0
fi

if [ "$(uname -m)" = "x86_64" ] && ! command -v nasm >/dev/null; then
    echo "error: an Intel Mac needs nasm to build FFmpeg. Install it with: brew install nasm" >&2
    exit 1
fi

mkdir -p "$WORK"
rm -rf "$WORK/src" "$PREFIX"
echo "Downloading FFmpeg $FFMPEG_TAG..."
git -c advice.detachedHead=false clone --quiet --depth 1 --branch "$FFMPEG_TAG" \
    https://github.com/FFmpeg/FFmpeg.git "$WORK/src"
GOT="$(git -C "$WORK/src" rev-parse HEAD)"
if [ "$GOT" != "$FFMPEG_COMMIT" ]; then
    echo "error: FFmpeg $FFMPEG_TAG is commit $GOT, expected $FFMPEG_COMMIT" >&2
    exit 1
fi

cd "$WORK/src"
echo "Configuring FFmpeg..."
./configure \
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
    >"$WORK/configure.log"

echo "Building FFmpeg (about 1 minute)..."
make -j"$(sysctl -n hw.ncpu)" >"$WORK/make.log" 2>&1 || {
    tail -30 "$WORK/make.log" >&2
    exit 1
}
make install >>"$WORK/make.log" 2>&1
echo "$WANT" >"$STAMP"
echo "FFmpeg $FFMPEG_TAG is built: $PREFIX"
