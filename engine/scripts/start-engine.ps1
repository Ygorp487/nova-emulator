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

function Read-EmulatorDetails {
  $stderr = if (Test-Path $EmulatorStderr) { Get-Content $EmulatorStderr -Raw -ErrorAction SilentlyContinue } else { '' }
  $stdout = if (Test-Path $EmulatorStdout) { Get-Content $EmulatorStdout -Raw -ErrorAction SilentlyContinue } else { '' }
  $details = (($stderr + "`n" + $stdout).Trim())
  if ([string]::IsNullOrWhiteSpace($details)) { return 'Sem saída adicional do Android Emulator.' }
  return $details
}

function Test-DeviceOnline {
  $result = Invoke-AdbSafe @('devices')
  if ($result.ExitCode -ne 0) { return $false }
  return [bool]($result.Text -match '(?m)^emulator-5554\s+device\s*$')
}

function Test-BootComplete {
  if (-not (Test-DeviceOnline)) { return $false }
  $result = Invoke-AdbSafe @('-s','emulator-5554','shell','getprop','sys.boot_completed')
  return ($result.ExitCode -eq 0 -and $result.Text.Trim() -eq '1')
}

function Test-EmulatorChildRunning {
  foreach ($name in @('qemu-system-x86_64','emulator')) {
    $items = @(Get-Process -Name $name -ErrorAction SilentlyContinue)
    foreach ($item in $items) {
      try {
        $path = $item.Path
        if ($path -and $path.StartsWith($RuntimeRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
          return $true
        }
      } catch {}
    }
  }
  return $false
}

Repair-AvdDescriptor

# Confirm that the Android Emulator can actually see the migrated AVD before booting it.
$oldPreference = $ErrorActionPreference
try {
  $ErrorActionPreference = 'SilentlyContinue'
  $knownAvds = (& $Emulator -list-avds 2>$null | Out-String)
} finally {
  $ErrorActionPreference = $oldPreference
}
if ($knownAvds -notmatch '(?m)^NOVA\s*$') {
  throw "O Android Emulator não reconheceu o AVD NOVA em $AvdHome. Use Reparar ambiente para recriar o dispositivo virtual."
}

$settings = switch ($Profile) {
  'eco' { @{ Cpu = 2; Ram = 2048; Gpu = 'auto' } }
  'performance' { @{ Cpu = 4; Ram = 6144; Gpu = 'host' } }
  default { @{ Cpu = 4; Ram = 4096; Gpu = 'host' } }
}

if (Test-BootComplete) {
  Write-Host '[NOVA] Android já está iniciado e pronto.' -ForegroundColor Green
  exit 0
}

# accel-check is diagnostic only. The real launch with -accel on is authoritative.
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
  Write-Host '[NOVA] accel-check inconclusivo. O boot real será usado como teste.' -ForegroundColor Yellow
}

$null = Invoke-AdbSafe @('start-server')

$args = @(
  '-avd', 'NOVA',
  '-port', '5554',
  '-accel', 'on',
  '-gpu', $settings.Gpu,
  '-cores', $settings.Cpu,
  '-memory', $settings.Ram,
  '-no-metrics',
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
"launcher_pid=$($process.Id)" | Add-Content $LogFile -Encoding UTF8

Write-Host '[NOVA] Aguardando Android/ADB...' -ForegroundColor DarkGray
$deadline = (Get-Date).AddSeconds(90)
$launcherExitSeenAt = $null

while ((Get-Date) -lt $deadline) {
  if (Test-DeviceOnline) { break }

  $process.Refresh()
  if ($process.HasExited) {
    if ($null -eq $launcherExitSeenAt) {
      $launcherExitSeenAt = Get-Date
      $exitCode = $null
      try { $exitCode = $process.ExitCode } catch {}
      "launcher_exited code=$exitCode" | Add-Content $LogFile -Encoding UTF8
      Write-Host '[NOVA] Launcher do Emulator encerrou; aguardando possível processo QEMU filho...' -ForegroundColor DarkGray
    }

    # emulator.exe may hand off to qemu-system-x86_64.exe. Give the child/ADB time to appear.
    $graceExpired = ((Get-Date) - $launcherExitSeenAt).TotalSeconds -ge 15
    if ($graceExpired -and (-not (Test-EmulatorChildRunning))) {
      $details = Read-EmulatorDetails
      $details | Add-Content $LogFile -Encoding UTF8
      throw "Android Emulator não permaneceu em execução. $details"
    }
  }

  Start-Sleep -Milliseconds 750
}

if (-not (Test-DeviceOnline)) {
  $details = Read-EmulatorDetails
  $details | Add-Content $LogFile -Encoding UTF8
  throw "Android Emulator foi iniciado, mas o ADB não ficou online em 90 segundos. $details"
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
