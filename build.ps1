# Builds Reyer (Rust) in release mode and assembles a portable folder in dist\Reyer.
# Shaders, design tokens and the window icon are embedded in the executable; only libmpv-2.dll
# and the runtime images (assets\backgrounds, assets\art) are copied next to it.
# With -Installer it also builds dist\Reyer-<version>-windows-x64-setup.exe (needs Inno Setup 6, iscc.exe).
# Run from the project folder; requires the Rust toolchain (cargo) on PATH.
param([switch]$Installer)
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo was not found. Install Rust from https://rustup.rs."
}
if (-not (Test-Path "libmpv-2.dll")) {
    throw "libmpv-2.dll is missing. See README.md, 'Getting libmpv'."
}

& cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$out = "dist\Reyer"
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory -Path $out | Out-Null

Copy-Item "target\release\reyer.exe" "$out\Reyer.exe"
Copy-Item "libmpv-2.dll" $out
foreach ($dir in "backgrounds", "art") {
    New-Item -ItemType Directory -Path "$out\assets\$dir" -Force | Out-Null
    Copy-Item "assets\$dir\*" "$out\assets\$dir" -Recurse
}
New-Item -ItemType Directory -Path "$out\icons" -Force | Out-Null
Copy-Item "assets\icon\reyer_video.ico", "assets\icon\reyer_audio.ico" "$out\icons"
Copy-Item "LICENSE" $out

Write-Host "Built $out\Reyer.exe"

if ($Installer) {
    $iscc = (Get-Command iscc -ErrorAction SilentlyContinue).Source
    if (-not $iscc) {
        $iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") |
            Where-Object { Test-Path $_ } | Select-Object -First 1
    }
    if (-not $iscc) { throw "Inno Setup 6 was not found. Install it from https://jrsoftware.org/isdl.php." }
    $version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
    & $iscc "/DAppVersion=$version" "installer\Reyer.iss"
    if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
}
