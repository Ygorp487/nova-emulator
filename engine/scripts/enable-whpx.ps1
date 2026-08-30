$ErrorActionPreference = 'Stop'

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw 'Este script precisa ser executado como administrador.'
}

Write-Host '[NOVA] Ativando Windows Hypervisor Platform...' -ForegroundColor Cyan
& dism.exe /Online /Enable-Feature /FeatureName:HypervisorPlatform /All /NoRestart
if ($LASTEXITCODE -ne 0) {
  throw "DISM falhou ao ativar HypervisorPlatform (código $LASTEXITCODE)."
}

Write-Host '[NOVA] Recurso ativado. Reinicie o Windows antes de iniciar o Android.' -ForegroundColor Green
