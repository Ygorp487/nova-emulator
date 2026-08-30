param(
  [ValidateSet('eco','balanced','performance')]
  [string]$Profile = 'balanced'
)

$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$SdkRoot = Join-Path $RepoRoot 'engine\runtime\sdk'
$AvdHome = Join-Path $RepoRoot 'engine\runtime\avd'
$Emulator = Join-Path $SdkRoot 'emulator\emulator.exe'
$Adb = Join-Path $SdkRoot 'platform-tools\adb.exe'
$AvdConfig = Join-Path $AvdHome 'NOVA.avd\config.ini'

$env:ANDROID_SDK_ROOT = $SdkRoot
$env:ANDROID_HOME = $SdkRoot
$env:ANDROID_AVD_HOME = $AvdHome

if (-not (Test-Path $Emulator)) { throw "Android Emulator runtime not found: $Emulator" }
if (-not (Test-Path $Adb)) { throw "ADB not found: $Adb" }
if (-not (Test-Path $AvdConfig)) { throw "NOVA AVD not found: $AvdConfig" }

$settings = switch ($Profile) {
  'eco' { @{ Cpu = 2; Ram = 2048; Gpu = 'auto' } }
  'performance' { @{ Cpu = 4; Ram = 6144; Gpu = 'host' } }
  default { @{ Cpu = 4; Ram = 4096; Gpu = 'host' } }
}

$existing = & $Adb -s emulator-5554 get-state 2>$null
if ($LASTEXITCODE -eq 0 -and $existing.Trim() -eq 'device') {
  Write-Host '[NOVA] Android já está em execução.' -ForegroundColor Green
  exit 0
}

$accelOutput = & $Emulator -accel-check 2>&1 | Out-String
if ($LASTEXITCODE -ne 0 -or $accelOutput -notmatch '(?i)usable') {
  throw "Aceleração de hardware indisponível. Resultado: $accelOutput"
}

$args = @(
  '-avd', 'NOVA',
  '-port', '5554',
  '-accel', 'on',
  '-gpu', $settings.Gpu,
  '-cores', $settings.Cpu,
  '-memory', $settings.Ram,
  '-no-boot-anim',
  '-netdelay', 'none',
  '-netspeed', 'full'
)

Write-Host "[NOVA] Iniciando perfil ${Profile}: $($settings.Cpu) cores / $($settings.Ram) MB / GPU $($settings.Gpu)" -ForegroundColor Cyan
Start-Process -FilePath $Emulator -ArgumentList $args -WorkingDirectory $RepoRoot
