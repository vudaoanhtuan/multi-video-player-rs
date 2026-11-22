# Shared by the Windows scripts. Do not run it directly.
#
# Import-MsvcEnvironment puts the Microsoft C compiler (cl, link, lib) and the
# Windows SDK in the environment of this PowerShell process.
# Get-Msys2Root returns the folder of MSYS2.

$ErrorActionPreference = 'Stop'

function Import-MsvcEnvironment {
    if (Get-Command cl.exe -ErrorAction SilentlyContinue) {
        return
    }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) {
        throw 'Visual Studio or the Visual Studio Build Tools are not installed. Install the "Desktop development with C++" workload.'
    }
    $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if (-not $vs) {
        throw 'Visual Studio has no C++ compiler. Install the "Desktop development with C++" workload.'
    }
    $vcvars = Join-Path $vs 'VC\Auxiliary\Build\vcvars64.bat'
    # Run the setup file of Visual Studio in cmd, then copy its environment.
    $lines = cmd /c "`"$vcvars`" >nul 2>&1 && set"
    foreach ($line in $lines) {
        $name, $value = $line -split '=', 2
        if ($name -and $null -ne $value) {
            Set-Item -Path "env:$name" -Value $value
        }
    }
    if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
        throw "Cannot set up the Microsoft C compiler with $vcvars"
    }
}

function Get-Msys2Root {
    $candidates = @($env:MSYS2_ROOT, 'C:\msys64') | Where-Object { $_ }
    foreach ($root in $candidates) {
        if (Test-Path (Join-Path $root 'usr\bin\bash.exe')) {
            return $root
        }
    }
    throw 'MSYS2 is not installed. Install it (winget install MSYS2.MSYS2), or set MSYS2_ROOT to its folder.'
}

# Stops with a message when cargo cannot find libclang. The Rust bindings of
# FFmpeg (the crate ffmpeg-sys-next) need it at build time.
function Assert-Libclang {
    if ($env:LIBCLANG_PATH) {
        return
    }
    $found = Get-ChildItem -Path "$env:ProgramFiles\LLVM\bin\libclang.dll", "${env:ProgramFiles(x86)}\Microsoft Visual Studio\*\*\VC\Tools\Llvm\x64\bin\libclang.dll", "$env:ProgramFiles\Microsoft Visual Studio\*\*\VC\Tools\Llvm\x64\bin\libclang.dll" -ErrorAction SilentlyContinue
    if (-not $found) {
        throw 'LLVM (libclang) is not installed. Install it (winget install LLVM.LLVM), or set LIBCLANG_PATH to the folder of libclang.dll.'
    }
}
