@echo off
setlocal
cd /d "%~dp0"
title NOVA Emulator - Gerar EXE
echo =============================================
echo   NOVA EMULATOR - GERAR INSTALADOR EXE
echo =============================================
echo.
echo O NOVA vai verificar/instalar automaticamente:
echo - Node.js / npm
echo - Rust / Cargo
echo - Visual C++ Build Tools
echo - WebView2 Runtime
echo - Runtime Android NOVA (Emulator/QEMU + ADB + AVD)
echo - Dependencias do projeto
echo.
echo Na primeira instalacao do Android, as licencas oficiais do SDK
echo serao exibidas no terminal para voce aceitar.
echo.
echo Depois disso o instalador sera compilado em:
echo NOVA-BUILD\NOVA-Setup.exe
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\bootstrap-windows.ps1" -Mode build
set "EXITCODE=%ERRORLEVEL%"
if not "%EXITCODE%"=="0" (
  echo.
  echo [NOVA] A compilacao terminou com erro %EXITCODE%.
  echo O log do runtime fica em:
  echo %%LOCALAPPDATA%%\NOVA Emulator\engine\runtime\logs\runtime-install.log
  echo Tire uma foto desta tela e envie para o ChatGPT se precisar.
  pause
) else (
  echo.
  echo [NOVA] Instalador gerado em NOVA-BUILD\NOVA-Setup.exe
  echo [NOVA] Runtime Android verificado neste computador.
  pause
)
exit /b %EXITCODE%
