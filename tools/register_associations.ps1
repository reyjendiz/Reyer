# Registers Reyer as a media player for the current user (no admin rights needed) and opens
# Windows "Default apps", where Reyer can then be chosen for video and audio.
# Windows 10/11 do not allow a program to silently make itself the default; this is the supported way.
#
#   powershell -ExecutionPolicy Bypass -File tools\register_associations.ps1 [-Exe "C:\path\Reyer.exe"]
#   powershell -ExecutionPolicy Bypass -File tools\register_associations.ps1 -Unregister
param(
    [string]$Exe = (Join-Path (Split-Path $PSScriptRoot -Parent) "dist\Reyer\Reyer.exe"),
    [switch]$Unregister
)
$ErrorActionPreference = "Stop"

$video = ".mov .qt .m4v .mp4 .mkv .avi .wmv .webm .flv .ts .m2ts .mts .vob .3gp .3g2 .mpg .mpeg .m2v .ogv .asf .mxf .f4v .divx .rm .rmvb".Split(" ")
$audio = ".m4a .m4b .m4r .caf .aif .aiff .aifc .mp3 .flac .wav .aac .ogg .oga .opus .wma .ape .wv .ac3 .eac3 .dts .mka .alac .mp2 .tta".Split(" ")

$classes = "HKCU:\Software\Classes"
$capabilities = "HKCU:\Software\Reyer\Capabilities"

if ($Unregister) {
    foreach ($ext in $video + $audio) {
        $key = "$classes\$ext\OpenWithProgids"
        if (Test-Path $key) { Remove-ItemProperty -Path $key -Name "Reyer.Video", "Reyer.Audio" -ErrorAction SilentlyContinue }
    }
    Remove-Item "$classes\Reyer.Video", "$classes\Reyer.Audio", "HKCU:\Software\Reyer" -Recurse -ErrorAction SilentlyContinue
    Remove-ItemProperty "HKCU:\Software\RegisteredApplications" -Name "Reyer" -ErrorAction SilentlyContinue
    Write-Host "Reyer file associations removed."
    exit 0
}

if (-not (Test-Path $Exe)) { throw "Reyer.exe not found at $Exe (build it first or pass -Exe)." }
$exePath = (Resolve-Path $Exe).Path

function Set-ProgId($id, $name) {
    New-Item -Path "$classes\$id\shell\open\command" -Force | Out-Null
    Set-ItemProperty -Path "$classes\$id" -Name "(default)" -Value $name
    Set-ItemProperty -Path "$classes\$id" -Name "FriendlyTypeName" -Value $name
    New-Item -Path "$classes\$id\DefaultIcon" -Force | Out-Null
    Set-ItemProperty -Path "$classes\$id\DefaultIcon" -Name "(default)" -Value "`"$exePath`",0"
    Set-ItemProperty -Path "$classes\$id\shell\open\command" -Name "(default)" -Value "`"$exePath`" `"%1`""
}
Set-ProgId "Reyer.Video" "Video (Reyer)"
Set-ProgId "Reyer.Audio" "Audio (Reyer)"

New-Item -Path "$capabilities\FileAssociations" -Force | Out-Null
Set-ItemProperty -Path $capabilities -Name "ApplicationName" -Value "Reyer"
Set-ItemProperty -Path $capabilities -Name "ApplicationDescription" -Value "A small Liquid Glass video and audio player."
foreach ($ext in $video) {
    Set-ItemProperty -Path "$capabilities\FileAssociations" -Name $ext -Value "Reyer.Video"
    New-Item -Path "$classes\$ext\OpenWithProgids" -Force | Out-Null
    New-ItemProperty -Path "$classes\$ext\OpenWithProgids" -Name "Reyer.Video" -PropertyType String -Value "" -Force | Out-Null
}
foreach ($ext in $audio) {
    Set-ItemProperty -Path "$capabilities\FileAssociations" -Name $ext -Value "Reyer.Audio"
    New-Item -Path "$classes\$ext\OpenWithProgids" -Force | Out-Null
    New-ItemProperty -Path "$classes\$ext\OpenWithProgids" -Name "Reyer.Audio" -PropertyType String -Value "" -Force | Out-Null
}
New-Item -Path "HKCU:\Software\RegisteredApplications" -Force | Out-Null
Set-ItemProperty -Path "HKCU:\Software\RegisteredApplications" -Name "Reyer" -Value "Software\Reyer\Capabilities"

# let Explorer pick up the new handlers
Add-Type -Namespace Win32 -Name Shell -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int e, uint f, IntPtr a, IntPtr b);'
[Win32.Shell]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)

Write-Host "Reyer is registered for $($video.Count) video and $($audio.Count) audio types."
Write-Host "Opening Default apps: search for Reyer and set it for the types you want."
Start-Process "ms-settings:defaultapps-app?registeredAppUser=Reyer"
