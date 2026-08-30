$ErrorActionPreference = 'Stop'
$bootstrap = Join-Path $PSScriptRoot 'tools\bootstrap-windows.ps1'
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $bootstrap -Mode prepare
exit $LASTEXITCODE
