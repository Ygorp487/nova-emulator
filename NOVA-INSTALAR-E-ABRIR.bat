@echo off
setlocal
cd /d "%~dp0"
title NOVA Emulator - Instalar e Abrir
echo =============================================
echo   NOVA EMULATOR - INSTALAR E ABRIR
echo =============================================
echo.
echo Este assistente vai verificar e instalar automaticamente:
echo - Node.js LTS
echo - Rust / Cargo
echo - Visual C++ Build Tools
echo - WebView2 Runtime
echo - Dependencias npm do NOVA
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\bootstrap-windows.ps1" -Mode run
set "EXITCODE=%ERRORLEVEL%"
if not "%EXITCODE%"=="0" (
  echo.
  echo [NOVA] O processo terminou com erro %EXITCODE%.
  echo Tire uma foto desta tela e envie para o ChatGPT se precisar.
  pause
)
exit /b %EXITCODE%
