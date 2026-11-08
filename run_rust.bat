@echo off
title AppSwitcher Native Rust (Direct2D 120Hz)
cd /d "%~dp0\rust"
if not exist "target\release\appswitcher.exe" (
    echo Building release binary...
    set RUSTUP_HOME=D:\Rust\.rustup
    set CARGO_HOME=D:\Rust\.cargo
    set PATH=D:\Rust\.cargo\bin;%PATH%
    cargo build --release
)
echo Starting AppSwitcher Native Rust v2.0...
target\release\appswitcher.exe
pause

