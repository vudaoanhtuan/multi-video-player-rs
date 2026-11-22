# Builds the FFmpeg that this project uses, on Windows. The project does not
# use an FFmpeg of the system. On macOS, scripts/build-ffmpeg.sh does this work.
#
# Results, in the folder ffmpeg-build\ of the project:
#   prefix\                static libraries and headers. The app links to them.
#                          Only what the app uses: the MKV reader, the common
#                          camera video decoders, Direct3D 11 hardware decode,
#                          and the pixel format converter. No audio, no
#                          network, no external libraries. The license of this
#                          build is LGPL.
#   tools\bin\ffmpeg.exe   a small ffmpeg command. The tests make their test
#                          videos with it. It is not part of the app.
#
# The script does nothing when the results are up to date. .cargo/config.toml
# points cargo to prefix\ (FFMPEG_DIR).
#
# FFmpeg has a Unix build system (a configure script and make). This script
# runs it in the shell of MSYS2, with the Microsoft C compiler, so the
# libraries fit a Rust program for the "msvc" target.
#
# Requirements:
#   - Visual Studio or the Visual Studio Build Tools, with "Desktop development with C++"
#   - MSYS2 in C:\msys64 (winget install MSYS2.MSYS2), or MSYS2_ROOT set to its folder
#   - Git for Windows
#   - LLVM (winget install LLVM.LLVM). FFmpeg does not need it, but cargo does.
#
# If PowerShell refuses to run the script, use:
#   powershell -ExecutionPolicy Bypass -File scripts\build-ffmpeg.ps1

. (Join-Path $PSScriptRoot 'windows-env.ps1')

# Keep the version and the decoders equal to scripts/build-ffmpeg.sh.
$ffmpegTag = 'n7.1.5'
# The commit of the tag. The build stops if the download is a different commit.
$ffmpegCommit = '3a0867c2bfda4a4d4309ca1a8cbdc6175e67f587'
$decoders = 'h264,hevc,mjpeg,mpeg4,mpeg2video,vp8,vp9,av1'
$parsers = 'h264,hevc,mjpeg,mpeg4video,mpegvideo,vp8,vp9,av1'
# Direct3D 11 needs the two forms of each name: the decoders offer the
# Direct3D 11 frame format only when the form with no "2" is also built.
$hwaccels = 'h264_d3d11va,hevc_d3d11va,mpeg2_d3d11va,vp9_d3d11va,av1_d3d11va,' +
    'h264_d3d11va2,hevc_d3d11va2,mpeg2_d3d11va2,vp9_d3d11va2,av1_d3d11va2'

$root = Split-Path $PSScriptRoot -Parent
$work = Join-Path $root 'ffmpeg-build'
$src = Join-Path $work 'src'
$prefix = Join-Path $work 'prefix'
$tool = Join-Path $work 'tools\bin\ffmpeg.exe'
$stamp = Join-Path $work '.built'
# Change the number when the configure options below change.
$want = "1 $ffmpegCommit $decoders windows-msvc"

if ((Test-Path $stamp) -and ((Get-Content $stamp -Raw).Trim() -eq $want) -and
    (Test-Path (Join-Path $prefix 'lib\libavcodec.a')) -and (Test-Path $tool)) {
    Write-Host "FFmpeg $ffmpegTag is already built: $work"
    return
}
if ($root -match "[\s']") {
    throw "The FFmpeg build system cannot work in a folder with a space or a quote in its path: $root"
}

Import-MsvcEnvironment
$msys = Get-Msys2Root
if (-not (Get-Command git.exe -ErrorAction SilentlyContinue)) {
    throw 'Git for Windows is not installed, or git is not in PATH.'
}

# The shell of MSYS2 uses the PATH of Windows, so it finds the compiler.
$env:MSYS2_PATH_TYPE = 'inherit'
$env:MSYSTEM = 'MSYS'
# Stay in the current folder at the start of the shell.
$env:CHERE_INVOKING = '1'
# A shell of Git for Windows or MSYS2 that started PowerShell leaves its old
# PATH in this variable, and the shell of MSYS2 would use that old PATH.
Remove-Item env:ORIGINAL_PATH -ErrorAction SilentlyContinue
$bash = Join-Path $msys 'usr\bin\bash.exe'

# make and nasm build FFmpeg. diffutils gives cmp, which the configure script uses.
$missing = @('make', 'nasm', 'cmp') | Where-Object { -not (Test-Path (Join-Path $msys "usr\bin\$_.exe")) }
if ($missing) {
    Write-Host 'Installing make, nasm, and diffutils in MSYS2...'
    & $bash -lc 'pacman -S --needed --noconfirm make nasm diffutils'
    if ($LASTEXITCODE -ne 0) {
        throw 'Cannot install the MSYS2 packages. Run this in the MSYS2 shell: pacman -S make nasm diffutils'
    }
}

# The path of a folder in the form that the shell of MSYS2 uses, for example /c/work.
function ConvertTo-MsysPath([string]$path) {
    $result = & $bash -lc "cygpath -u '$path'"
    if ($LASTEXITCODE -ne 0 -or -not $result) {
        throw "Cannot convert the path $path"
    }
    return $result.Trim()
}

# Runs a command of the FFmpeg build system in a folder, in the shell of
# MSYS2. The output goes to a log file. Shows the end of the log if the
# command fails.
function Invoke-BuildStep([string]$folder, [string]$log, [string]$command) {
    $folderPath = ConvertTo-MsysPath $folder
    $logPath = ConvertTo-MsysPath $log
    & $bash -lc "cd '$folderPath' && $command > '$logPath' 2>&1"
    if ($LASTEXITCODE -ne 0) {
        Get-Content $log -Tail 30 | Write-Host
        throw "The FFmpeg build failed. The full log is in $log"
    }
}

New-Item -ItemType Directory -Force $work | Out-Null
Remove-Item $stamp -Force -ErrorAction SilentlyContinue
$have = ''
if (Test-Path (Join-Path $src '.git')) {
    $have = (git -C $src rev-parse HEAD | Out-String).Trim()
}
if ($have -ne $ffmpegCommit) {
    if (Test-Path $src) {
        Remove-Item $src -Recurse -Force
    }
    Write-Host "Downloading FFmpeg $ffmpegTag..."
    # Git for Windows can change line ends to CR LF. The FFmpeg scripts need LF.
    git -c advice.detachedHead=false -c core.autocrlf=false clone --quiet --depth 1 --branch $ffmpegTag https://github.com/FFmpeg/FFmpeg.git $src
    if ($LASTEXITCODE -ne 0) {
        throw 'Cannot download the FFmpeg source.'
    }
    $got = (git -C $src rev-parse HEAD | Out-String).Trim()
    if ($got -ne $ffmpegCommit) {
        throw "FFmpeg $ffmpegTag is commit $got, expected $ffmpegCommit"
    }
}

$configure = "'$(ConvertTo-MsysPath $src)/configure'"
$jobs = $env:NUMBER_OF_PROCESSORS
# The Microsoft compiler, with the C runtime library linked statically (-MT),
# the same as the Rust program (see .cargo/config.toml).
$compiler = '--toolchain=msvc --extra-cflags=-MT'

# 1. The libraries for the app.
Write-Host 'Building the FFmpeg libraries (about 10 minutes)...'
$buildLibs = Join-Path $work 'build-libs'
foreach ($folder in $prefix, $buildLibs) {
    if (Test-Path $folder) {
        Remove-Item $folder -Recurse -Force
    }
}
New-Item -ItemType Directory -Force $buildLibs | Out-Null
$libOptions = @(
    "--prefix='$(ConvertTo-MsysPath $prefix)'",
    $compiler,
    '--enable-static --disable-shared',
    '--disable-programs --disable-doc --disable-debug',
    '--disable-autodetect --disable-network',
    '--disable-avdevice --disable-avfilter --disable-swresample --disable-postproc',
    '--disable-everything',
    '--enable-d3d11va',
    '--enable-protocol=file',
    '--enable-demuxer=matroska',
    "--enable-decoder=$decoders",
    "--enable-parser=$parsers",
    "--enable-hwaccel=$hwaccels"
) -join ' '
Invoke-BuildStep $buildLibs (Join-Path $work 'configure-libs.log') "$configure $libOptions"
Invoke-BuildStep $buildLibs (Join-Path $work 'make-libs.log') "make -j$jobs"
Invoke-BuildStep $buildLibs (Join-Path $work 'make-libs-install.log') "make -j$jobs install"

# 2. The ffmpeg command for the tests: a test picture source, the H.264
#    encoder of Windows (Media Foundation), and the MKV writer.
Write-Host 'Building the ffmpeg command for the tests (about 5 minutes)...'
$buildTools = Join-Path $work 'build-tools'
foreach ($folder in (Join-Path $work 'tools'), $buildTools) {
    if (Test-Path $folder) {
        Remove-Item $folder -Recurse -Force
    }
}
New-Item -ItemType Directory -Force $buildTools, (Split-Path $tool -Parent) | Out-Null
$toolOptions = @(
    $compiler,
    '--enable-static --disable-shared',
    '--disable-ffplay --disable-ffprobe --disable-doc --disable-debug',
    '--disable-autodetect --disable-network',
    '--disable-everything',
    '--enable-mediafoundation',
    '--enable-indev=lavfi',
    '--enable-filter=testsrc2,scale,format',
    '--enable-decoder=wrapped_avframe,rawvideo',
    '--enable-encoder=h264_mf,mpeg4',
    '--enable-muxer=matroska',
    '--enable-protocol=file'
) -join ' '
Invoke-BuildStep $buildTools (Join-Path $work 'configure-tools.log') "$configure $toolOptions"
Invoke-BuildStep $buildTools (Join-Path $work 'make-tools.log') "make -j$jobs ffmpeg.exe"
Copy-Item (Join-Path $buildTools 'ffmpeg.exe') $tool -Force

# cargo keeps a copy of the libraries and does not see new libraries. Remove
# the copy, so the next cargo command links the new libraries.
if ((Get-Command cargo.exe -ErrorAction SilentlyContinue) -and (Test-Path (Join-Path $root 'target'))) {
    Push-Location $root
    try {
        cargo clean --quiet -p ffmpeg-sys-next
        cargo clean --quiet --release -p ffmpeg-sys-next
    }
    finally {
        Pop-Location
    }
}

Set-Content -Path $stamp -Value $want -Encoding ascii
Write-Host "FFmpeg $ffmpegTag is built: $work"

try {
    Assert-Libclang
}
catch {
    Write-Warning $_.Exception.Message
}
