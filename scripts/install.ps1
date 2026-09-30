# Installs gnomish-relay from the latest GitHub Release, and runs setup (SPEC.md 11.3).
#   irm https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.ps1 | iex
# With arguments for setup, after --autostart (SPEC.md 11.4):
#   & ([scriptblock]::Create((irm https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts/install.ps1))) --timeways
# With -Wsl (or --wsl), the desktop app runs in WSL2, with the Linux sandbox (SPEC.md 11.5).
# The WSL2 path is experimental until it passes the manual plan of SPEC 11.5 on a real PC,
# so without the flag the installer keeps the Windows desktop app and asks nothing.
param(
    [switch] $NoWsl,
    # The player asks for WSL2; the RunOnce entry after the restart of `wsl --install` sets it too.
    [switch] $Wsl,
    [Parameter(ValueFromRemainingArguments = $true)] [string[]] $SetupArgs = @()
)
$ErrorActionPreference = "Stop"
# The progress bar of Windows PowerShell 5.1 makes downloads many times slower.
$ProgressPreference = "SilentlyContinue"

$url = if ($env:GNOMISH_URL) { $env:GNOMISH_URL } else { "https://github.com/eserilev/gnomish-relay/releases/latest/download" }
$scripts = if ($env:GNOMISH_SCRIPTS) { $env:GNOMISH_SCRIPTS } else { "https://raw.githubusercontent.com/eserilev/gnomish-relay/main/scripts" }
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
Write-Host "Installed $bin\gnomish-relay.exe"

if ($SetupArgs -contains "--wsl") { $Wsl = $true }
if ($SetupArgs -contains "--no-wsl") { $NoWsl = $true }
$SetupArgs = @($SetupArgs | Where-Object { $_ -ne "--no-wsl" -and $_ -ne "--wsl" })
if ($env:GNOMISH_NO_WSL -eq "1") { $NoWsl = $true }
if (-not $Wsl) { $NoWsl = $true }
if ($NoWsl) {
    & "$bin\gnomish-relay.exe" setup --autostart @SetupArgs
    return
}

# The name, the kernel, and the user of the default distro. The text of
# `wsl.exe --status` changes with the language of Windows; this does not.
function Get-Distro {
    $ErrorActionPreference = "Continue"
    if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) { return $null }
    $out = & wsl.exe --exec sh -c 'echo "$WSL_DISTRO_NAME"; uname -r; id -u' 2>$null
    if ($LASTEXITCODE -ne 0 -or @($out).Count -lt 3) { return $null }
    return @{ Name = $out[0].Trim(); Kernel = $out[1].Trim(); Uid = $out[2].Trim() }
}

$distro = Get-Distro
if (-not $distro) {
    Write-Host "Installing WSL2. Windows asks for admin rights."
    Start-Process wsl.exe -ArgumentList "--install" -Verb RunAs -Wait
    $distro = Get-Distro
}
if (-not $distro) {
    # The installer runs again at the next sign-in, with the same arguments.
    Invoke-WebRequest -UseBasicParsing "$scripts/install.ps1" -OutFile "$bin\install.ps1"
    $again = ($SetupArgs | ForEach-Object { "`"$_`"" }) -join " "
    $command = "powershell.exe -NoProfile -ExecutionPolicy Bypass -NoExit -File `"$bin\install.ps1`" -Wsl $again"
    $runOnce = "HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce"
    New-Item -Force -Path $runOnce | Out-Null
    Set-ItemProperty -Path $runOnce -Name "Gnomish Relay setup" -Value $command
    Write-Host "Restart Windows to finish. The installer continues after you sign in."
    return
}
$d = $distro.Name
if ($distro.Kernel -notmatch "WSL2") {
    Write-Host "Your Linux runs on WSL1, which has no sandbox. Run: wsl --set-version $d 2"
    exit 1
}
if ($distro.Uid -eq "0") {
    Write-Host "Set up your Linux user first: open $d from the Start menu, pick a user name and password, then run this installer again."
    exit 1
}

# As root through wsl.exe, so the player types no password.
& wsl.exe -d $d -u root --exec sh -c 'command -v bwrap >/dev/null || { command -v apt-get >/dev/null && apt-get update -qq && apt-get install -y -qq bubblewrap; }'
if ($LASTEXITCODE -ne 0) {
    Write-Host "Install bubblewrap in $d with its package manager, then run gnomish-relay restart in $d."
}

# A login shell puts ~/.local/bin on PATH, where the native installer of Claude Code puts it.
& wsl.exe -d $d --exec bash -lc 'command -v claude >/dev/null'
if ($LASTEXITCODE -ne 0) {
    & wsl.exe -d $d --exec bash -lc 'curl -fsSL https://claude.ai/install.sh | bash'
    Write-Host "Log in to Claude, then type /exit."
    & wsl.exe -d $d --exec bash -lc 'claude'
}

# WSL passes a variable to Linux only when WSLENV names it.
foreach ($var in "GNOMISH_URL", "GNOMISH_SCRIPTS") {
    if (Test-Path "env:$var") { $env:WSLENV = "$var`:$env:WSLENV" }
}
& wsl.exe -d $d --exec bash -lc 'curl -fsSL "$0" | sh -s -- "$@"' "$scripts/install.sh" @SetupArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "The desktop app runs in WSL2 ($d)."
