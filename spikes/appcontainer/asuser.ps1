$ErrorActionPreference = "Stop"
$pw = "Sp1ke-" + [guid]::NewGuid().ToString("N").Substring(0, 12) + "!a"
net user acspike $pw /add | Out-Null
$exe = (Resolve-Path "spikes/appcontainer/target/debug/acspike.exe").Path
$dir = Split-Path $exe
icacls $dir /grant "acspike:(OI)(CI)F" | Out-Null
$work = "C:\acspike-work"
New-Item -ItemType Directory -Force $work | Out-Null
icacls $work /grant "acspike:(OI)(CI)F" | Out-Null
$cred = New-Object System.Management.Automation.PSCredential("$env:COMPUTERNAME\acspike", (ConvertTo-SecureString $pw -AsPlainText -Force))
$p = Start-Process -FilePath $exe -ArgumentList "outer" -Credential $cred -LoadUserProfile -WorkingDirectory $work -RedirectStandardOutput "$work\out.txt" -RedirectStandardError "$work\err.txt" -Wait -PassThru -NoNewWindow
"exit: " + $p.ExitCode
Get-Content "$work\out.txt"
Get-Content "$work\err.txt"
