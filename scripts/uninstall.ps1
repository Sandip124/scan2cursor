# Windows uninstaller: stops the server and removes auto-start and the firewall rule.
$dir = Split-Path -Parent $PSScriptRoot
foreach ($t in 'Scan2Cursor','BarcodeBridge','BarcodeTyper') {   # current + older names
  if (Get-ScheduledTask -TaskName $t -ErrorAction SilentlyContinue) {
    Stop-ScheduledTask -TaskName $t -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $t -Confirm:$false
    Write-Host "Removed auto-start task $t"
  }
}
# Stop anything still listening on the scanner port, plus the server started from this folder (or an older Node one).
$pids = @()
$pids += Get-NetTCPConnection -LocalPort 8443 -State Listen -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess
$pids += Get-Process -Name 'scan2cursor','barcode-bridge' -ErrorAction SilentlyContinue |
         Where-Object { $_.Path -and $_.Path.StartsWith($dir) } | Select-Object -ExpandProperty Id
$pids += Get-CimInstance Win32_Process -Filter "Name='cloudflared.exe'" |
         Where-Object { $_.CommandLine -and $_.CommandLine.Contains('127.0.0.1:8443') } | Select-Object -ExpandProperty ProcessId
$pids += Get-CimInstance Win32_Process -Filter "Name='node.exe'" |
         Where-Object { $_.CommandLine -and $_.CommandLine.Contains($dir) } | Select-Object -ExpandProperty ProcessId
$pids | Sort-Object -Unique | ForEach-Object { Stop-Process -Id $_ -Force -ErrorAction SilentlyContinue; Write-Host "Stopped process $_" }
foreach ($r in 'Scan2Cursor','BarcodeBridge') { try { Remove-NetFirewallRule -DisplayName $r -ErrorAction Stop; Write-Host "Removed firewall rule $r" } catch {} }
Write-Host 'Uninstalled. You can now delete this folder.'
