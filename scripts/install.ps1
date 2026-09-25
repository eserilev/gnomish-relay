# Installs gnomish-relay from the latest GitHub Release, and runs setup (SPEC.md 11.3).
#   irm https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.ps1 | iex
$ErrorActionPreference = "Stop"
# The progress bar of Windows PowerShell 5.1 makes downloads many times slower.
$ProgressPreference = "SilentlyContinue"

$url = if ($env:GNOMISH_URL) { $env:GNOMISH_URL } else { "https://github.com/eserilev/gnomish-relay/releases/latest/download" }
$name = "gnomish-relay-x86_64-pc-windows-msvc.zip"
$tmp = New-Item -ItemType Directory -Path (Join-Path $env:TEMP ([guid]::NewGuid()))

Invoke-WebRequest -UseBasicParsing "$url/$name" -OutFile "$tmp\$name"
Invoke-WebRequest -UseBasicParsing "$url/$name.sha256" -OutFile "$tmp\$name.sha256"
$want = (Get-Content "$tmp\$name.sha256").Split(" ")[0]
$have = (Get-FileHash "$tmp\$name" -Algorithm SHA256).Hash.ToLower()
if ($want -ne $have) { throw "the download does not match its SHA-256 sum" }

$bin = Join-Path $env:LOCALAPPDATA "gnomish-relay\bin"
New-Item -ItemType Directory -Force -Path $bin | Out-Null
Expand-Archive "$tmp\$name" -DestinationPath $bin -Force
Remove-Item -Recurse -Force $tmp

$path = [Environment]::GetEnvironmentVariable("Path", "User")
if (($path -split ";") -notcontains $bin) {
    [Environment]::SetEnvironmentVariable("Path", "$path;$bin", "User")
}
# The new user PATH reaches only new terminals. This one gets it too.
if (($env:Path -split ";") -notcontains $bin) { $env:Path = "$env:Path;$bin" }
Write-Host "installed $bin\gnomish-relay.exe"
& "$bin\gnomish-relay.exe" setup --autostart
