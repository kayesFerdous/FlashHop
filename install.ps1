# FlashHop installer for Windows 10/11 (x64). Downloads the latest GitHub Release.
#   irm https://raw.githubusercontent.com/kayesFerdous/FlashHop/main/install.ps1 | iex
# Uninstall:
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/kayesFerdous/FlashHop/main/install.ps1))) -Uninstall
param([switch]$Uninstall)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue' # Invoke-WebRequest is very slow with the progress bar

$url = if ($env:FLASHHOP_URL) { $env:FLASHHOP_URL } else { 'https://github.com/kayesFerdous/FlashHop/releases/latest/download' }
$dir = Join-Path $env:LOCALAPPDATA 'Programs\FlashHop'
$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'FlashHop.lnk'

Get-Process flashhop -ErrorAction SilentlyContinue | Stop-Process -Force # the .exe is locked while it runs

if ($Uninstall) {
    Remove-Item -Recurse -Force $dir, $lnk -ErrorAction SilentlyContinue
    Write-Host 'FlashHop removed. Settings are kept in ~\.config\flashhop.'
    return
}

$zip = Join-Path ([IO.Path]::GetTempPath()) "flashhop-$PID.zip"
Write-Host 'Downloading FlashHop for Windows...'
Invoke-WebRequest "$url/flashhop-windows-x86_64.zip" -OutFile $zip -UseBasicParsing
New-Item -ItemType Directory -Force $dir | Out-Null
Expand-Archive $zip $dir -Force
Remove-Item $zip
Get-ChildItem $dir | Unblock-File

$s = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
$s.TargetPath = Join-Path $dir 'flashhop.exe'
$s.IconLocation = Join-Path $dir 'flashhop.ico'
$s.Save()

Write-Host "Installed to $dir and added to the Start menu."
Write-Host 'On first run, click Allow when Windows Firewall asks (port 47821).'
