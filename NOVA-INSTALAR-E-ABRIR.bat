@echo off
setlocal
cd /d "%~dp0"
title NOVA Emulator - Instalar e Abrir
echo =============================================
echo   NOVA EMULATOR - INSTALAR E ABRIR
echo =============================================
echo.
echo Este assistente vai verificar e instalar automaticamente:
echo - Node.js LTS / npm
echo - Rust / Cargo
echo - Visual C++ Build Tools
echo - WebView2 Runtime
echo - Runtime Android NOVA (Emulator/QEMU + ADB + AVD)
echo - Dependencias npm do NOVA
echo.
echo Na primeira instalacao do Android, as licencas oficiais do SDK
echo serao exibidas no terminal para voce aceitar.
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\bootstrap-windows.ps1" -Mode run
set "EXITCODE=%ERRORLEVEL%"
if not "%EXITCODE%"=="0" (
  echo.
  echo [NOVA] O processo terminou com erro %EXITCODE%.
  echo O log do runtime fica em:
  echo %%LOCALAPPDATA%%\NOVA Emulator\engine\runtime\logs\runtime-install.log
  echo Tire uma foto desta tela e envie para o ChatGPT se precisar.
  pause
)
exit /b %EXITCODE%
