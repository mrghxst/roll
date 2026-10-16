# Fetches the tools the Windows installer ships: ExifTool (metadata) and an
# LGPL build of FFmpeg (HEIC decoding, video frames and playback proxies).
# Versions and SHA-256 checksums are pinned; a mismatch stops the script.
# Also builds roll-ai once for ONNX Runtime's DirectML.dll, which ort puts
# in target/release (as a link into its download cache, so it is copied).
# Output: apps/desktop/src-tauri/bundle-tools/{exiftool,ffmpeg,DirectML.dll}
# (not in git).
#
#   pwsh scripts/fetch-tools.ps1
#   cd apps/desktop; pnpm tauri build --config src-tauri/tauri.bundle.json

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$tools = @(
  @{
    Name = 'exiftool'
    # https://exiftool.org/checksums.txt
    Url = 'https://sourceforge.net/projects/exiftool/files/exiftool-13.59_64.zip/download'
    Sha256 = '44b512b25af500724ba579d0a53c8fc5851628b692dd5e5d94ae4a15c2cba9ec'
  },
  @{
    Name = 'ffmpeg'
    # GitHub's digest of the release asset. BtbN prunes old autobuilds: when
    # this one is gone, pin a newer LGPL shared 8.x build.
    Url = 'https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-10-12-13-12/ffmpeg-n8.1.3-18-g46d899d7a0-win64-lgpl-shared-8.1.zip'
    Sha256 = 'fa628b9de20254fe7c80605a4c6af54d0bffdda0807363ee7557131c2b78e051'
  }
)

$root = Join-Path $PSScriptRoot '..\apps\desktop\src-tauri\bundle-tools'
$cache = Join-Path $root '.download'
New-Item -ItemType Directory -Force $cache | Out-Null

foreach ($t in $tools) {
  $zip = Join-Path $cache "$($t.Name).zip"
  if (-not (Test-Path $zip) -or (Get-FileHash $zip -Algorithm SHA256).Hash -ne $t.Sha256.ToUpper()) {
    Write-Host "Downloading $($t.Url)"
    # SourceForge sends a web page instead of the file to browsers.
    Invoke-WebRequest -Uri $t.Url -OutFile $zip -UserAgent 'curl/8.10' -MaximumRedirection 10
  }
  $hash = (Get-FileHash $zip -Algorithm SHA256).Hash
  if ($hash -ne $t.Sha256.ToUpper()) {
    Remove-Item $zip
    throw "Checksum mismatch for $($t.Name): got $hash"
  }
  $tmp = Join-Path $cache "$($t.Name)-x"
  if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
  Expand-Archive $zip -DestinationPath $tmp
  $dest = Join-Path $root $t.Name
  if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
  New-Item -ItemType Directory -Force $dest | Out-Null
  $top = Get-ChildItem $tmp | Select-Object -First 1
  if ($t.Name -eq 'exiftool') {
    # exiftool(-k).exe pauses for a key press; renamed it runs as a tool.
    Copy-Item -Recurse (Join-Path $top.FullName '*') $dest
    Rename-Item (Join-Path $dest 'exiftool(-k).exe') 'exiftool.exe'
  } else {
    Copy-Item (Join-Path $top.FullName 'bin\*') $dest -Exclude 'ffplay.exe'
    Copy-Item (Join-Path $top.FullName 'LICENSE.txt') (Join-Path $dest 'LICENSE.txt')
  }
  Remove-Item -Recurse -Force $tmp
  Write-Host "$($t.Name): $dest"
}

& (Join-Path $root 'exiftool\exiftool.exe') -ver
& (Join-Path $root 'ffmpeg\ffmpeg.exe') -hide_banner -version | Select-Object -First 1

# tauri-build copies resources into target/release; a resource that already
# lives there is copied onto itself, which Windows refuses (os error 32).
cargo build --release -p roll-ai
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
$dll = Get-Item (Join-Path $PSScriptRoot '..\target\release\DirectML.dll')
if ($dll.LinkTarget) { $dll = $dll.ResolveLinkTarget($true) }
Copy-Item $dll.FullName (Join-Path $root 'DirectML.dll') -Force
Write-Host "DirectML.dll: $(Join-Path $root 'DirectML.dll')"
