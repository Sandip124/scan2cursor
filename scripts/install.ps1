# Windows one-step installer: builds (or uses) the binary, makes a new token, auto-starts at logon.
$ErrorActionPreference = 'Stop'
$dir = Split-Path -Parent $PSScriptRoot
Set-Location $dir
$bin = Join-Path $dir 'scan2cursor.exe'

# Build from source when the Rust toolchain is here; otherwise use a prebuilt binary placed in this folder.
if ((Test-Path (Join-Path $dir 'src')) -and (Get-Command cargo -ErrorAction SilentlyContinue)) {
  cargo build --release
  if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
  Stop-ScheduledTask -TaskName 'Scan2Cursor' -ErrorAction SilentlyContinue   # release the old .exe before overwriting
  Copy-Item (Join-Path $dir 'target\release\scan2cursor.exe') $bin -Force
} elseif (-not (Test-Path $bin)) {
  throw "No scan2cursor.exe found. Put the prebuilt binary in $dir, or install Rust (https://rustup.rs) and re-run."
}
& $bin new-token --no-restart

$action   = New-ScheduledTaskAction -Execute $bin -WorkingDirectory $dir
$trigger  = New-ScheduledTaskTrigger -AtLogOn
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
            -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
Register-ScheduledTask -TaskName 'Scan2Cursor' -Action $action -Trigger $trigger -Settings $settings -Force | Out-Null

# Firewall rule (works only if run as admin; otherwise click "Allow" on the Windows prompt).
try { New-NetFirewallRule -DisplayName 'Scan2Cursor' -Direction Inbound -Protocol TCP -LocalPort 8443 -Action Allow -Profile Private -ErrorAction Stop | Out-Null } catch {}

Stop-ScheduledTask -TaskName 'Scan2Cursor' -ErrorAction SilentlyContinue
Start-ScheduledTask -TaskName 'Scan2Cursor'
Start-Sleep -Seconds 2
Start-Process 'http://localhost:8443/'
Write-Host 'Done. Scan the QR code on the page that just opened with your phone.'
