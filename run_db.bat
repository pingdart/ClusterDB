@echo off
title PingDart Core DB Engine
echo ======================================================
echo    Starting PingDart Core Database Engine
echo    Port: 4650 ^| Host: 0.0.0.0
echo    User: admin ^| Pass: admin123
echo ======================================================
cd /d "%~dp0"

:: Clear any stale process holding the database file lock
taskkill /F /IM pingdart-core.exe >nul 2>&1
timeout /t 1 /nobreak >nul

if exist "target\debug\pingdart-core.exe" (
    target\debug\pingdart-core.exe
) else (
    cargo run --bin pingdart-core
)
pause
