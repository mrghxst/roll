# A throwaway Immich for the round-trip test (PROMPT 9.3), on Windows.
#
# Everything lives in its own WSL distro ("roll-immich") under
# %LOCALAPPDATA%\RollImmichTest: Ubuntu, Docker, Immich and its data. Other
# distros are not touched. `remove` unregisters the distro and deletes the
# folder, which removes all of it.
#
#   powershell -ExecutionPolicy Bypass -File e2e/immich/server.ps1 setup    # once: distro, Docker, Immich, admin, API key
#   powershell -ExecutionPolicy Bypass -File e2e/immich/server.ps1 start    # after a reboot or stop
#   powershell -ExecutionPolicy Bypass -File e2e/immich/server.ps1 stop
#   powershell -ExecutionPolicy Bypass -File e2e/immich/server.ps1 status
#   powershell -ExecutionPolicy Bypass -File e2e/immich/server.ps1 remove   # everything gone
#
# The admin password and API key are generated and kept in
# credentials.json in that folder, never in the repository. Immich listens
# on http://localhost:2283 only. Machine learning is left out (not needed
# for sync, and several GB).

param([ValidateSet('setup', 'start', 'stop', 'status', 'remove')][string]$Action = 'status')

$ErrorActionPreference = 'Stop'
$Name = 'roll-immich'
$Dir = Join-Path $env:LOCALAPPDATA 'RollImmichTest'
$Version = 'v3.3.1'
$Url = 'http://localhost:2283'
$Creds = Join-Path $Dir 'credentials.json'
$KeepAlive = Join-Path $Dir 'keepalive.pid'

function Linux([string]$script) {
  # Root inside the throwaway distro; no password involved. The script goes
  # through a file: Windows PowerShell adds a BOM and CRLFs to piped text
  # and mangles quotes in native arguments.
  $file = Join-Path $Dir 'step.sh'
  [IO.File]::WriteAllText($file, ($script -replace "`r", ''), (New-Object Text.UTF8Encoding $false))
  $linux = '/mnt/' + $file.Substring(0, 1).ToLower() + $file.Substring(2).Replace('\', '/')
  wsl.exe -d $Name -u root -- bash $linux
  $code = $LASTEXITCODE
  Remove-Item $file
  if ($code -ne 0) { throw "Command failed in $Name (exit $code)" }
}

function Exists { (wsl.exe -l -q) -replace "`0", '' -contains $Name }

function Wait-Immich {
  for ($i = 0; $i -lt 120; $i++) {
    try {
      $r = Invoke-RestMethod "$Url/api/server/ping" -TimeoutSec 3
      if ($r.res -eq 'pong') { return }
    } catch {}
    Start-Sleep -Seconds 3
  }
  throw "Immich did not answer at $Url"
}

function Start-Server {
  # A WSL distro stops when nothing runs in it; a hidden sleep keeps it up.
  if (-not ((Test-Path $KeepAlive) -and (Get-Process -Id (Get-Content $KeepAlive) -ErrorAction SilentlyContinue))) {
    $p = Start-Process wsl.exe -ArgumentList '-d', $Name, '-u', 'root', '--exec', '/usr/bin/sleep', 'infinity' -WindowStyle Hidden -PassThru
    Set-Content $KeepAlive $p.Id
  }
  Linux 'until docker info >/dev/null 2>&1; do sleep 1; done; cd /opt/immich && docker compose up -d immich-server'
  Wait-Immich
}

function Stop-Server {
  if (Exists) {
    try { Linux 'cd /opt/immich && docker compose stop' } catch {}
    wsl.exe --terminate $Name | Out-Null
  }
  if (Test-Path $KeepAlive) {
    Stop-Process -Id (Get-Content $KeepAlive) -ErrorAction SilentlyContinue
    Remove-Item $KeepAlive
  }
}

function New-Secret { -join ((48..57) + (65..90) + (97..122) | Get-Random -Count 32 | ForEach-Object { [char]$_ }) }

switch ($Action) {
  'setup' {
    if (Exists) { throw "$Name exists already; run remove first" }
    New-Item -ItemType Directory -Force $Dir | Out-Null
    wsl.exe --install Ubuntu-24.04 --name $Name --location (Join-Path $Dir 'wsl') --no-launch --web-download
    if ($LASTEXITCODE -ne 0) { throw 'wsl --install failed' }
    $db = New-Secret
    Linux @"
set -e
printf '[boot]\nsystemd=true\n[interop]\nappendWindowsPath=false\n' > /etc/wsl.conf
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq docker.io docker-compose-v2 curl >/dev/null
mkdir -p /opt/immich/library /opt/immich/postgres
cd /opt/immich
curl -fsSL -o docker-compose.yml https://github.com/immich-app/immich/releases/download/$Version/docker-compose.yml
cat > .env <<EOF
UPLOAD_LOCATION=/opt/immich/library
DB_DATA_LOCATION=/opt/immich/postgres
IMMICH_VERSION=$Version
DB_PASSWORD=$db
DB_USERNAME=postgres
DB_DATABASE_NAME=immich
EOF
"@
    # systemd starts Docker from now on.
    wsl.exe --terminate $Name | Out-Null
    Start-Server

    $email = 'admin@roll.test'
    $password = New-Secret
    Invoke-RestMethod "$Url/api/auth/admin-sign-up" -Method Post -ContentType 'application/json' `
      -Body (@{ email = $email; password = $password; name = 'Roll test' } | ConvertTo-Json) | Out-Null
    $token = (Invoke-RestMethod "$Url/api/auth/login" -Method Post -ContentType 'application/json' `
        -Body (@{ email = $email; password = $password } | ConvertTo-Json)).accessToken
    $key = (Invoke-RestMethod "$Url/api/api-keys" -Method Post -ContentType 'application/json' `
        -Headers @{ Authorization = "Bearer $token" } -Body (@{ name = 'Roll round trip'; permissions = @('all') } | ConvertTo-Json)).secret
    @{ url = $Url; email = $email; password = $password; api_key = $key } | ConvertTo-Json | Set-Content -Encoding utf8 $Creds
    Write-Output "Immich $Version runs at $Url. Credentials: $Creds"
  }
  'start' { Start-Server; Write-Output "Immich runs at $Url" }
  'stop' { Stop-Server; Write-Output 'Stopped' }
  'status' {
    if (-not (Exists)) { Write-Output 'Not set up'; break }
    try { Invoke-RestMethod "$Url/api/server/version" -TimeoutSec 3 | ConvertTo-Json -Compress } catch { Write-Output 'Set up, not running' }
  }
  'remove' {
    Stop-Server
    if (Exists) { wsl.exe --unregister $Name | Out-Null }
    if (Test-Path $Dir) { Remove-Item -Recurse -Force $Dir }
    Write-Output 'Removed: the distro, Docker, Immich and its data'
  }
}
