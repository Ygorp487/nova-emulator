@echo off
setlocal
cd /d "%~dp0"
title NOVA Emulator - Instalar e Abrir
echo =============================================
echo   NOVA EMULATOR - INSTALAR E ABRIR
echo =============================================
echo.
echo Este assistente usa somente a versao RELEASE do NOVA.
echo Ele verifica dependencias, runtime Android e gera o instalador
echo sem abrir o Tauri/Vite em modo de desenvolvimento.
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\bootstrap-windows.ps1" -Mode build
set "EXITCODE=%ERRORLEVEL%"
if not "%EXITCODE%"=="0" (
  echo.
  echo [NOVA] A preparacao terminou com erro %EXITCODE%.
  echo Log do runtime:
  echo %%LOCALAPPDATA%%\NOVA\Runtime\logs\runtime-install.log
  pause
  exit /b %EXITCODE%
)

set "SETUP=%~dp0NOVA-BUILD\NOVA-Setup.exe"
if not exist "%SETUP%" (
  echo [NOVA] Instalador nao encontrado: %SETUP%
  pause
  exit /b 1
)

echo.
echo [NOVA] Abrindo instalador RELEASE...
start "" /wait "%SETUP%"

set "APP1=%LOCALAPPDATA%\NOVA Emulator\NOVA Emulator.exe"
set "APP2=%LOCALAPPDATA%\Programs\NOVA Emulator\NOVA Emulator.exe"
set "APP3=%PROGRAMFILES%\NOVA Emulator\NOVA Emulator.exe"
set "APP4=%PROGRAMFILES(X86)%\NOVA Emulator\NOVA Emulator.exe"

if exist "%APP1%" start "" "%APP1%" & exit /b 0
if exist "%APP2%" start "" "%APP2%" & exit /b 0
if exist "%APP3%" start "" "%APP3%" & exit /b 0
if exist "%APP4%" start "" "%APP4%" & exit /b 0

echo.
echo [NOVA] Instalacao concluida. Se o NOVA nao abriu automaticamente,
echo abra "NOVA Emulator" pelo Menu Iniciar.
pause
exit /b 0
