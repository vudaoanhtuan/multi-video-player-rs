# Builds "dist\Multi Video Player.exe": one executable with FFmpeg inside.
# The app uses only libraries that are part of Windows. Open it with a double-click.
#
# If PowerShell refuses to run the script, use:
#   powershell -ExecutionPolicy Bypass -File scripts\bundle-windows.ps1

. (Join-Path $PSScriptRoot 'windows-env.ps1')

$root = Split-Path $PSScriptRoot -Parent
$appName = 'Multi Video Player'
$executable = 'multi-video-player.exe'

& (Join-Path $PSScriptRoot 'build-ffmpeg.ps1')
Assert-Libclang

# cargo reads .cargo/config.toml from the current folder. That file points
# cargo to the FFmpeg of this project.
Push-Location $root
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) {
        throw 'The cargo build failed.'
    }
}
finally {
    Pop-Location
}
$binary = Join-Path $root "target\release\$executable"

# The executable must use only libraries that are part of Windows. The C
# runtime library of Visual Studio is not a part of Windows.
Import-MsvcEnvironment
$libraries = dumpbin /nologo /dependents $binary | ForEach-Object { $_.Trim() } | Where-Object { $_ -match '\.dll$' }
$external = $libraries | Where-Object {
    ($_ -match '^(vcruntime|msvcp|vccorlib|concrt|vcomp)') -or
    (($_ -notmatch '^(api|ext)-ms-') -and -not (Test-Path (Join-Path $env:WINDIR "System32\$_")))
}
if ($external) {
    throw "The executable needs libraries that are not part of Windows: $($external -join ', ')"
}

$dist = Join-Path $root 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$app = Join-Path $dist "$appName.exe"
Copy-Item $binary $app -Force

$size = '{0:N0} MB' -f ((Get-Item $app).Length / 1MB)
Write-Host ''
Write-Host "Built: $app ($size)"
