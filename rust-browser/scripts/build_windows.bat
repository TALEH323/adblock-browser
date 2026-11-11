@echo off
setlocal enabledelayedexpansion

cd /d "%~dp0\.."
echo =^> Building Rust Release binary for Windows (MSVC + WebView2)...
cargo build --release
if errorlevel 1 exit /b 1

set "DIST_DIR=dist\windows\AdBlockBrowser"
if exist "%DIST_DIR%" rmdir /s /q "%DIST_DIR%"
mkdir "%DIST_DIR%"

copy /Y "target\release\adblock-browser.exe" "%DIST_DIR%\AdBlockBrowser.exe" >nul
echo =^> Ready: %DIST_DIR%\AdBlockBrowser.exe
echo Not: Windows 10/11 sistemlerde Microsoft Edge WebView2 Runtime otomatik kullanilir.
endlocal
