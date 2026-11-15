@echo off
rem Windows: install or uninstall Scan2Cursor. Double-click and choose, or: setup.bat install | uninstall
set ACTION=%1
if "%ACTION%"=="" (
  choice /C IU /M "Scan2Cursor: [I]nstall or [U]ninstall"
  if errorlevel 2 (set ACTION=uninstall) else (set ACTION=install)
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0setup.ps1" %ACTION%
pause
