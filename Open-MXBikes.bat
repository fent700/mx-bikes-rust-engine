@echo off
rem Launches the Open-MXBikes rewrite (not the original mxbikes.exe).
rem Usage: Open-MXBikes.bat [--track club] [--bike 250xf] [--camera chase|onboard|side]
cd /d "%~dp0"
set EXE=target\x86_64-pc-windows-msvc\release\open-mxbikes.exe
if not exist "%EXE%" (
    echo Building Open-MXBikes ^(first run takes a few minutes^)...
    cargo build --release -p open-mxbikes || (pause & exit /b 1)
)
"%EXE%" %*
if errorlevel 1 pause
