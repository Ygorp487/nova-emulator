@echo off
setlocal
cd /d "%~dp0"
title NOVA Emulator - Gerar EXE
echo =============================================
echo   NOVA EMULATOR - GERAR INSTALADOR EXE
echo =============================================
echo.
echo O NOVA vai verificar/instalar as dependencias e depois compilar.
echo O arquivo final sera salvo em NOVA-BUILD\NOVA-Setup.exe
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\bootstrap-windows.ps1" -Mode build
set "EXITCODE=%ERRORLEVEL%"
if not "%EXITCODE%"=="0" (
  echo.
  echo [NOVA] A compilacao terminou com erro %EXITCODE%.
  echo Tire uma foto desta tela e envie para o ChatGPT se precisar.
  pause
) else (
  echo.
  echo [NOVA] Instalador gerado em NOVA-BUILD\NOVA-Setup.exe
  pause
)
exit /b %EXITCODE%
