# Windows: install or uninstall Scan2Cursor. Started by setup.bat.
#   install     build (or use the .exe in this folder), new pairing token, auto-start at logon
#   uninstall   stop the server, remove auto-start and the firewall rule
param([ValidateSet('install', 'uninstall')][string]$Action = 'install')
$dir = Split-Path -Parent $PSScriptRoot
Set-Location $dir
$bin = Join-Path $dir 'scan2cursor.exe'

if ($Action -eq 'install') {
  $ErrorActionPreference = 'Stop'
  # Build from source when Rust is here; otherwise use a prebuilt binary placed in this folder.
  if ((Test-Path (Join-Path $dir 'src')) -and (Get-Command cargo -ErrorAction SilentlyContinue)) {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    if (Test-Path $bin) { & $bin stop | Out-Null }   # release the old .exe before overwriting
    Copy-Item (Join-Path $dir 'target\release\scan2cursor.exe') $bin -Force
  } elseif (-not (Test-Path $bin)) {
    throw "No scan2cursor.exe found. Put the prebuilt binary in $dir, or install Rust (https://rustup.rs) and re-run."
  }
  & $bin new-token --no-restart

  # The task only launches the server at logon; the server itself runs hidden in the background.
  $action   = New-ScheduledTaskAction -Execute $bin -WorkingDirectory $dir
  $trigger  = New-ScheduledTaskTrigger -AtLogOn
  $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero)
  Register-ScheduledTask -TaskName 'Scan2Cursor' -Action $action -Trigger $trigger -Settings $settings -Force | Out-Null

  # Firewall rule (works only if run as admin; otherwise click "Allow" on the Windows prompt).
  try { New-NetFirewallRule -DisplayName 'Scan2Cursor' -Direction Inbound -Protocol TCP -LocalPort 8443 -Action Allow -Profile Private -ErrorAction Stop | Out-Null } catch {}

  & $bin restart
  Start-Process 'http://localhost:8443/'
  Write-Host 'Done. Scan the QR code on the page that just opened with your phone.'
  return
}

if (Get-ScheduledTask -TaskName 'Scan2Cursor' -ErrorAction SilentlyContinue) {
  Stop-ScheduledTask -TaskName 'Scan2Cursor' -ErrorAction SilentlyContinue
  Unregister-ScheduledTask -TaskName 'Scan2Cursor' -Confirm:$false
  Write-Host 'Removed auto-start task'
}
# Stop anything still listening on the scanner port, plus the server started from this folder and its tunnel.
$pids = @()
$pids += Get-NetTCPConnection -LocalPort 8443 -State Listen -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess
$pids += Get-Process -Name 'scan2cursor' -ErrorAction SilentlyContinue |
         Where-Object { $_.Path -and $_.Path.StartsWith($dir) } | Select-Object -ExpandProperty Id
$pids += Get-CimInstance Win32_Process -Filter "Name='cloudflared.exe'" |
         Where-Object { $_.CommandLine -and $_.CommandLine.Contains('127.0.0.1:8443') } | Select-Object -ExpandProperty ProcessId
$pids | Sort-Object -Unique | ForEach-Object { Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue; Write-Host "Stopped process $_" }
try { Remove-NetFirewallRule -DisplayName 'Scan2Cursor' -ErrorAction Stop; Write-Host 'Removed firewall rule' } catch {}
Write-Host 'Uninstalled. You can now delete this folder.'
