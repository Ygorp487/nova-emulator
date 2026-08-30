param(
  [ValidateSet('eco','balanced','performance')]
  [string]$Profile = 'balanced',
  [string]$RuntimeRoot = ''
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($RuntimeRoot)) {
  $RuntimeRoot = Join-Path $env:LOCALAPPDATA 'NOVA Emulator\engine\runtime'
}

$SdkRoot = Join-Path $RuntimeRoot 'sdk'
$AvdHome = Join-Path $RuntimeRoot 'avd'
$Emulator = Join-Path $SdkRoot 'emulator\emulator.exe'
$Adb = Join-Path $SdkRoot 'platform-tools\adb.exe'
$AvdConfig = Join-Path $AvdHome 'NOVA.avd\config.ini'
$LogRoot = Join-Path $RuntimeRoot 'logs'
$LogFile = Join-Path $LogRoot 'engine-last-start.log'

$env:ANDROID_SDK_ROOT = $SdkRoot
$env:ANDROID_HOME = $SdkRoot
$env:ANDROID_AVD_HOME = $AvdHome

New-Item -ItemType Directory -Force -Path $LogRoot | Out-Null

if (-not (Test-Path $Emulator)) { throw "Android Emulator runtime not found: $Emulator" }
if (-not (Test-Path $Adb)) { throw "ADB not found: $Adb" }
if (-not (Test-Path $AvdConfig)) { throw "NOVA AVD not found: $AvdConfig" }

$settings = switch ($Profile) {
  'eco' { @{ Cpu = 2; Ram = 2048; Gpu = 'auto' } }
  'performance' { @{ Cpu = 4; Ram = 6144; Gpu = 'host' } }
  default { @{ Cpu = 4; Ram = 4096; Gpu = 'host' } }
}

function Test-DeviceOnline {
  $state = & $Adb -s emulator-5554 get-state 2>$null
  return ($LASTEXITCODE -eq 0 -and ($state | Out-String).Trim() -eq 'device')
}

function Test-BootComplete {
  if (-not (Test-DeviceOnline)) { return $false }
  $boot = & $Adb -s emulator-5554 shell getprop sys.boot_completed 2>$null
  return ($LASTEXITCODE -eq 0 -and ($boot | Out-String).Trim() -eq '1')
}

if (Test-BootComplete) {
  Write-Host '[NOVA] Android já está iniciado e pronto.' -ForegroundColor Green
  exit 0
}

$accelOutput = & $Emulator -accel-check 2>&1 | Out-String
if ($LASTEXITCODE -ne 0 -or $accelOutput -notmatch '(?i)usable') {
  throw "Aceleração de hardware indisponível. Resultado: $accelOutput"
}

& $Adb start-server | Out-Null

$args = @(
  '-avd', 'NOVA',
  '-port', '5554',
  '-accel', 'on',
  '-gpu', $settings.Gpu,
  '-cores', $settings.Cpu,
  '-memory', $settings.Ram,
  '-no-boot-anim',
  '-no-audio',
  '-netdelay', 'none',
  '-netspeed', 'full'
)

"[$(Get-Date -Format o)] NOVA start profile=$Profile cpu=$($settings.Cpu) ram=$($settings.Ram) gpu=$($settings.Gpu) runtime=$RuntimeRoot" | Set-Content $LogFile -Encoding UTF8
Write-Host "[NOVA] Iniciando perfil ${Profile}: $($settings.Cpu) cores / $($settings.Ram) MB / GPU $($settings.Gpu)" -ForegroundColor Cyan

$process = Start-Process -FilePath $Emulator -ArgumentList $args -WorkingDirectory $RuntimeRoot -PassThru
"PID=$($process.Id)" | Add-Content $LogFile -Encoding UTF8

Write-Host '[NOVA] Aguardando ADB...' -ForegroundColor DarkGray
$deadline = (Get-Date).AddSeconds(90)
while ((Get-Date) -lt $deadline) {
  if ($process.HasExited) {
    throw "Emulator encerrou durante a inicialização (código $($process.ExitCode))."
  }
  if (Test-DeviceOnline) { break }
  Start-Sleep -Milliseconds 750
}

if (-not (Test-DeviceOnline)) {
  throw 'Android Emulator abriu, mas o ADB não ficou online em 90 segundos.'
}

Write-Host '[NOVA] ADB conectado. Aguardando Android concluir o boot...' -ForegroundColor DarkGray
$bootDeadline = (Get-Date).AddMinutes(3)
while ((Get-Date) -lt $bootDeadline) {
  if (Test-BootComplete) {
    & $Adb -s emulator-5554 shell input keyevent 82 2>$null | Out-Null
    $androidVersion = (& $Adb -s emulator-5554 shell getprop ro.build.version.release 2>$null | Out-String).Trim()
    $abi = (& $Adb -s emulator-5554 shell getprop ro.product.cpu.abi 2>$null | Out-String).Trim()
    "boot_complete=1 android=$androidVersion abi=$abi" | Add-Content $LogFile -Encoding UTF8
    Write-Host "[NOVA] Android $androidVersion ($abi) pronto." -ForegroundColor Green
    exit 0
  }
  Start-Sleep -Seconds 1
}

throw 'ADB conectou, mas o Android não concluiu o boot dentro de 3 minutos.'
