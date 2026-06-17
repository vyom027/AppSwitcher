@echo off
REM Build AppSwitcher.exe + installer.
REM Needs: pip install pyinstaller   and   Inno Setup 6 (ISCC.exe).

echo [0/2] Closing any running AppSwitcher...
taskkill /IM AppSwitcher.exe /F >nul 2>&1

echo [1/2] Building app with PyInstaller...
pyinstaller --noconfirm AppSwitcher.spec
if errorlevel 1 goto :pyfail

echo [2/2] Building installer with Inno Setup...
set "ISCC=C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
if not exist "%ISCC%" set "ISCC=C:\Program Files\Inno Setup 6\ISCC.exe"
if not exist "%ISCC%" goto :noinno

"%ISCC%" installer.iss
echo.
echo Done. Installer: Output\AppSwitcher-Setup.exe
goto :eof

:noinno
echo.
echo Inno Setup not found. Install it from https://jrsoftware.org/isdl.php
echo Portable app is ready in:  dist\AppSwitcher\
echo Zip that folder to share without an installer.
goto :eof

:pyfail
echo PyInstaller failed.
exit /b 1
