@echo off
rem NEC-5 test pack for wireant: double-click, or drag your NEC-5 program onto this file.
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-tests.ps1" %*
echo.
pause
