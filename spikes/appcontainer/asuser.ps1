$ErrorActionPreference = "Continue"
$pw = "Sp1ke-" + [guid]::NewGuid().ToString("N").Substring(0, 12) + "!a"
$secure = ConvertTo-SecureString $pw -AsPlainText -Force
New-LocalUser -Name "acspike" -Password $secure -PasswordNeverExpires -AccountNeverExpires
Get-LocalUser acspike | Format-List Name, Enabled, SID
$exe = (Resolve-Path "spikes/appcontainer/target/debug/acspike.exe").Path
$dir = Split-Path $exe
icacls $dir /grant "acspike:(OI)(CI)F" /T /Q
$work = "C:\acspike-work"
New-Item -ItemType Directory -Force $work | Out-Null
icacls $work /grant "acspike:(OI)(CI)F" /Q
$cred = New-Object System.Management.Automation.PSCredential("acspike", $secure)
try {
  $p = Start-Process -FilePath $exe -ArgumentList "outer" -Credential $cred -WorkingDirectory $work -RedirectStandardOutput "$work\out.txt" -RedirectStandardError "$work\err.txt" -Wait -PassThru
  "exit: " + $p.ExitCode
} catch {
  "start failed: $_"
  $cmd = "`"$exe`" outer > `"$work\out.txt`" 2> `"$work\err.txt`""
  schtasks /create /tn acspike /ru acspike /rp $pw /sc once /st 23:59 /tr "cmd /c $cmd" /f
  schtasks /run /tn acspike
  Start-Sleep -Seconds 60
  schtasks /query /tn acspike /v /fo list | Select-String "Status|Last Result"
}
Get-Content "$work\out.txt"
Get-Content "$work\err.txt"
