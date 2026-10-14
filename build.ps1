# Builds Reyer (Rust) in release mode and assembles a portable folder in dist\Reyer.
# Shaders, design tokens and the window icon are embedded in the executable; only libmpv-2.dll
# and the runtime images (assets\backgrounds, assets\art) are copied next to it.
# Run from the project folder; requires the Rust toolchain (cargo) on PATH.
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
Copy-Item "LICENSE" $out

Write-Host "Built $out\Reyer.exe"
