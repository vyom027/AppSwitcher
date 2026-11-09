@echo off
title AppSwitcher Native Rust (Direct2D 120Hz)
cd /d "%~dp0\rust"

set RUSTUP_HOME=D:\Rust\.rustup
set CARGO_HOME=D:\Rust\.cargo
set PATH=D:\Rust\.cargo\bin;%PATH%

echo [build] Compiling AppSwitcher Native Rust v2.0 (release mode)...
cargo build --release
if errorlevel 1 (
    echo [error] Compilation failed.
    pause
    exit /b 1
)

echo.
echo ========================================================
echo  Launching AppSwitcher Native Rust (Direct2D 120Hz)
echo ========================================================
target\release\appswitcher.exe
pause
