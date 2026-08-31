param(
  [ValidateSet('eco','balanced','performance')]
  [string]$Profile = 'balanced',
  [string]$RuntimeRoot = ''
)

$ErrorActionPreference = 'Stop'
if ([string]::IsNullOrWhiteSpace($RuntimeRoot)) {
  $RuntimeRoot = Join-Path $env:LOCALAPPDATA 'NOVA\Runtime'
}

$SdkRoot = Join-Path $RuntimeRoot 'sdk'
$AvdHome = Join-Path $RuntimeRoot 'avd'
$AvdDir = Join-Path $AvdHome 'NOVA.avd'
$AvdIni = Join-Path $AvdHome 'NOVA.ini'
$Emulator = Join-Path $SdkRoot 'emulator\emulator.exe'
$Adb = Join-Path $SdkRoot 'platform-tools\adb.exe'
$AvdConfig = Join-Path $AvdDir 'config.ini'
$LogRoot = Join-Path $RuntimeRoot 'logs'
$LogFile = Join-Path $LogRoot 'engine-last-start.log'
$EmulatorStdout = Join-Path $LogRoot 'emulator-stdout.log'
$EmulatorStderr = Join-Path $LogRoot 'emulator-stderr.log'

$env:ANDROID_SDK_ROOT = $SdkRoot
$env:ANDROID_HOME = $SdkRoot
$env:ANDROID_AVD_HOME = $AvdHome

New-Item -ItemType Directory -Force -Path $LogRoot | Out-Null

if (-not (Test-Path $Emulator)) { throw "Android Emulator runtime not found: $Emulator" }
if (-not (Test-Path $Adb)) { throw "ADB not found: $Adb" }
if (-not (Test-Path $AvdConfig)) { throw "NOVA AVD not found: $AvdConfig" }

function Repair-AvdDescriptor {
  $target = 'android-35'
  if (Test-Path $AvdIni) {
    $existingTarget = Get-Content $AvdIni -ErrorAction SilentlyContinue | Where-Object { $_ -match '^target=' } | Select-Object -First 1
    if ($existingTarget) { $target = ($existingTarget -replace '^target=', '').Trim() }
  }

  $desired = @(
    'avd.ini.encoding=UTF-8',
    "path=$AvdDir",
    'path.rel=NOVA.avd',
    "target=$target"
  )

  $needsRepair = $true
  if (Test-Path $AvdIni) {
    $current = Get-Content $AvdIni -ErrorAction SilentlyContinue
    $pathLine = $current | Where-Object { $_ -match '^path=' } | Select-Object -First 1
    if ($pathLine -and (($pathLine -replace '^path=', '').Trim() -eq $AvdDir)) {
      $needsRepair = $false
    }
  }

  if ($needsRepair) {
    Write-Host '[NOVA] Reparando caminho interno do dispositivo virtual...' -ForegroundColor Yellow
    Set-Content -Path $AvdIni -Value $desired -Encoding ASCII
  }
}

# Native tools such as ADB legitimately write to stderr while a device is not online yet.
# Do not let Windows PowerShell promote those messages to terminating NativeCommandError records.
function Invoke-AdbSafe([string[]]$Arguments) {
  $oldPreference = $ErrorActionPreference
  try {
    $ErrorActionPreference = 'SilentlyContinue'
    $text = (& $Adb @Arguments 2>$null | Out-String).Trim()
    $code = $LASTEXITCODE
    return [pscustomobject]@{ ExitCode = $code; Text = $text }
  } finally {
    $ErrorActionPreference = $oldPreference
  }
}

Repair-AvdDescriptor

$settings = switch ($Profile) {
  'eco' { @{ Cpu = 2; Ram = 2048; Gpu = 'auto' } }
  'performance' { @{ Cpu = 4; Ram = 6144; Gpu = 'host' } }
  default { @{ Cpu = 4; Ram = 4096; Gpu = 'host' } }
}

function Test-DeviceOnline {
  # `adb -s emulator-5554 get-state` prints "device not found" before the Emulator exists.
  # `adb devices` is quiet in that normal state, so poll the device list instead.
  $result = Invoke-AdbSafe @('devices')
  if ($result.ExitCode -ne 0) { return $false }
  return [bool]($result.Text -match '(?m)^emulator-5554\s+device\s*$')
}

function Test-BootComplete {
  if (-not (Test-DeviceOnline)) { return $false }
  $result = Invoke-AdbSafe @('-s','emulator-5554','shell','getprop','sys.boot_completed')
  return ($result.ExitCode -eq 0 -and $result.Text.Trim() -eq '1')
}

if (Test-BootComplete) {
  Write-Host '[NOVA] Android já está iniciado e pronto.' -ForegroundColor Green
  exit 0
}

# accel-check is diagnostic only. The actual emulator launch with -accel on is authoritative.
$oldPreference = $ErrorActionPreference
try {
  $ErrorActionPreference = 'Continue'
  $accelOutput = (& $Emulator -accel-check 2>&1 | Out-String).Trim()
  $accelExit = $LASTEXITCODE
} finally {
  $ErrorActionPreference = $oldPreference
}
if ($accelExit -eq 0) {
  Write-Host '[NOVA] Aceleração detectada pelo Android Emulator.' -ForegroundColor Green
} else {
  Write-Host '[NOVA] accel-check foi inconclusivo. Tentando iniciar o Emulator para obter o diagnóstico real...' -ForegroundColor Yellow
  Write-Host $accelOutput -ForegroundColor DarkGray
}

$null = Invoke-AdbSafe @('start-server')

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
"avd_ini=$AvdIni`navd_path=$AvdDir`naccel_check_exit=$accelExit`n$accelOutput" | Add-Content $LogFile -Encoding UTF8
Remove-Item $EmulatorStdout,$EmulatorStderr -Force -ErrorAction SilentlyContinue
Write-Host "[NOVA] Iniciando perfil ${Profile}: $($settings.Cpu) cores / $($settings.Ram) MB / GPU $($settings.Gpu)" -ForegroundColor Cyan

$process = Start-Process -FilePath $Emulator -ArgumentList $args -WorkingDirectory $RuntimeRoot -RedirectStandardOutput $EmulatorStdout -RedirectStandardError $EmulatorStderr -PassThru
"PID=$($process.Id)" | Add-Content $LogFile -Encoding UTF8

Write-Host '[NOVA] Aguardando ADB...' -ForegroundColor DarkGray
$deadline = (Get-Date).AddSeconds(90)
while ((Get-Date) -lt $deadline) {
  if ($process.HasExited) {
    $stderr = if (Test-Path $EmulatorStderr) { Get-Content $EmulatorStderr -Raw -ErrorAction SilentlyContinue } else { '' }
    $stdout = if (Test-Path $EmulatorStdout) { Get-Content $EmulatorStdout -Raw -ErrorAction SilentlyContinue } else { '' }
    $details = (($stderr + "`n" + $stdout).Trim())
    if ([string]::IsNullOrWhiteSpace($details)) { $details = 'Sem saída adicional do Android Emulator.' }
    $details | Add-Content $LogFile -Encoding UTF8
    throw "Android Emulator encerrou durante a inicialização (código $($process.ExitCode)). $details"
  }
  if (Test-DeviceOnline) { break }
  Start-Sleep -Milliseconds 750
}

if (-not (Test-DeviceOnline)) {
  $stderr = if (Test-Path $EmulatorStderr) { Get-Content $EmulatorStderr -Raw -ErrorAction SilentlyContinue } else { '' }
  throw "Android Emulator abriu, mas o ADB não ficou online em 90 segundos. $stderr"
}

Write-Host '[NOVA] ADB conectado. Aguardando Android concluir o boot...' -ForegroundColor DarkGray
$bootDeadline = (Get-Date).AddMinutes(3)
while ((Get-Date) -lt $bootDeadline) {
  if (Test-BootComplete) {
    $null = Invoke-AdbSafe @('-s','emulator-5554','shell','input','keyevent','82')
    $androidVersion = (Invoke-AdbSafe @('-s','emulator-5554','shell','getprop','ro.build.version.release')).Text.Trim()
    $abi = (Invoke-AdbSafe @('-s','emulator-5554','shell','getprop','ro.product.cpu.abi')).Text.Trim()
    "boot_complete=1 android=$androidVersion abi=$abi" | Add-Content $LogFile -Encoding UTF8
    Write-Host "[NOVA] Android $androidVersion ($abi) pronto." -ForegroundColor Green
    exit 0
  }
  Start-Sleep -Seconds 1
}

throw 'ADB conectou, mas o Android não concluiu o boot dentro de 3 minutos.'
