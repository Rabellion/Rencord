@echo off
pwsh -NoProfile -ExecutionPolicy Bypass -File "%~dp0sign.ps1" "%~1"
exit /b %ERRORLEVEL%
