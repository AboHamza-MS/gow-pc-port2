@echo off
rem PC Port: Windows installer. Double-click or: install.cmd [-Iso "C:\path\God of War.iso"] [-InstallTools]
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
