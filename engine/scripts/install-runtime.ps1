param(
  [ValidateRange(34, 36)]
  [int]$ApiLevel = 35,
  [ValidateSet('default','google_apis')]
  [string]$ImageFlavor = 'default',
  [string]$RuntimeRoot = '',
  [switch]$NoPause
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if ([string]::IsNullOrWhiteSpace($RuntimeRoot)) {
  $RuntimeRoot = Join-Path $env:LOCALAPPDATA 'NOVA\Runtime'
}

$SdkRoot = Join-Path $RuntimeRoot 'sdk'
$AvdHome = Join-Path $RuntimeRoot 'avd'
$Downloads = Join-Path $RuntimeRoot 'downloads'
$Logs = Join-Path $RuntimeRoot 'logs'
$LogFile = Join-Path $Logs 'runtime-install.log'
$PrivateJava = Join-Path $RuntimeRoot 'java17'

$ToolsVersion = '15859902'
$ToolsZip = Join-Path $Downloads 'commandlinetools-win.zip'
$ToolsUrl = "https://dl.google.com/android/repository/commandlinetools-win-$ToolsVersion`_latest.zip"
$ToolsSha256 = '90ae805d20434428bffcb699c290860f19bb5f66a67e6b330067e3de801fb04a'

# NOVA uses a controlled Emulator build instead of whatever sdkmanager calls latest.
# 36.2.12 contains a Windows software-rendering crash fix and is compatible with API 35.
$PinnedEmulatorVersion = '36.2.12'
$PinnedEmulatorBuild = '14214601'
$PinnedEmulatorZip = Join-Path $Downloads "emulator-windows_x64-$PinnedEmulatorBuild.zip"
$PinnedEmulatorUrl = "https://dl.google.com/android/repository/emulator-windows_x64-$PinnedEmulatorBuild.zip"
$PinnedEmulatorSha256 = 'b0cf63f996d41eb75b0154938ff4a2e0140eab2625b9784cdc9d03ffbb1900df'

New-Item -ItemType Directory -Force -Path $RuntimeRoot, $SdkRoot, $AvdHome, $Downloads, $Logs | Out-Null
Start-Transcript -Path $LogFile -Append | Out-Null

try {
  $env:ANDROID_SDK_ROOT = $SdkRoot
  $env:ANDROID_HOME = $SdkRoot
  $env:ANDROID_AVD_HOME = $AvdHome

  function Invoke-Download([string]$Uri, [string]$OutFile, [int]$Attempts = 3) {
    for ($try = 1; $try -le $Attempts; $try++) {
      try {
        Write-Host ("[NOVA] Download {0}/{1}: {2}" -f $try, $Attempts, $Uri) -ForegroundColor DarkGray
        Invoke-WebRequest -Uri $Uri -OutFile $OutFile -UseBasicParsing
        if ((Test-Path $OutFile) -and (Get-Item $OutFile).Length -gt 0) { return }
      } catch {
        if ($try -eq $Attempts) { throw }
        Write-Host '[NOVA] Download falhou. Tentando novamente em 2 segundos...' -ForegroundColor Yellow
        Start-Sleep -Seconds 2
      }
    }
    throw "Não foi possível baixar $Uri"
  }

  function Get-JavaMajor([string]$JavaExe) {
    try {
      $text = (& $JavaExe -version 2>&1 | Out-String)
      if ($text -match 'version\s+"(?<major>\d+)') { return [int]$Matches.major }
      if ($text -match 'openjdk\s+(?<major>\d+)') { return [int]$Matches.major }
    } catch {}
    return 0
  }

  function Find-OrInstall-Java17 {
    $privateExe = Join-Path $PrivateJava 'bin\java.exe'
    if ((Test-Path $privateExe) -and (Get-JavaMajor $privateExe) -ge 17) { return $privateExe }

    $existing = Get-Command java.exe -ErrorAction SilentlyContinue
    if ($existing -and (Get-JavaMajor $existing.Source) -ge 17) { return $existing.Source }

    Write-Host '[NOVA] Java 17+ não encontrado. Baixando runtime Java privado do NOVA...' -ForegroundColor Cyan
    $javaZip = Join-Path $Downloads 'java17.zip'
    $javaExtract = Join-Path $Downloads 'java17-extracted'
    Remove-Item $javaZip -Force -ErrorAction SilentlyContinue
    Remove-Item $javaExtract -Recurse -Force -ErrorAction SilentlyContinue
    Invoke-Download 'https://api.adoptium.net/v3/binary/latest/17/ga/windows/x64/jdk/hotspot/normal/eclipse' $javaZip
    Expand-Archive -Path $javaZip -DestinationPath $javaExtract -Force

    $jdkFolder = Get-ChildItem $javaExtract -Directory | Select-Object -First 1
    if (-not $jdkFolder) { throw 'O Java foi baixado, mas o conteúdo do pacote não foi localizado.' }

    Remove-Item $PrivateJava -Recurse -Force -ErrorAction SilentlyContinue
    Move-Item $jdkFolder.FullName $PrivateJava
    if (-not (Test-Path $privateExe)) { throw "java.exe não encontrado em $privateExe" }
    return $privateExe
  }

  function Set-IniValue([string]$Path, [string]$Key, [string]$Value) {
    $lines = if (Test-Path $Path) { Get-Content $Path } else { @() }
    $pattern = '^' + [regex]::Escape($Key) + '='
    $found = $false
    $updated = foreach ($line in $lines) {
      if ($line -match $pattern) {
        $found = $true
        "$Key=$Value"
      } else { $line }
    }
    if (-not $found) { $updated += "$Key=$Value" }
    Set-Content -Path $Path -Value $updated -Encoding ASCII
  }

  function Get-EmulatorRevision([string]$EmulatorDir) {
    $source = Join-Path $EmulatorDir 'source.properties'
    if (-not (Test-Path $source)) { return '' }
    $line = Get-Content $source -ErrorAction SilentlyContinue |
      Where-Object { $_ -match '^Pkg\.Revision\s*=' } |
      Select-Object -First 1
    if (-not $line) { return '' }
    return (($line -split '=', 2)[1]).Trim()
  }

  function Stop-NovaEmulatorProcesses([string]$EmulatorDir) {
    $runtimePrefix = $RuntimeRoot
    try {
      $items = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {
        if ($_.ProcessId -eq $PID) { return $false }
        if ($_.Name -notin @('emulator.exe','qemu-system-x86_64.exe')) { return $false }

        $pathMatch = $false
        $cmdMatch = $false
        if (-not [string]::IsNullOrWhiteSpace($_.ExecutablePath)) {
          $pathMatch = $_.ExecutablePath.StartsWith($runtimePrefix, [System.StringComparison]::OrdinalIgnoreCase)
        }
        if (-not [string]::IsNullOrWhiteSpace($_.CommandLine)) {
          $cmdMatch = $_.CommandLine.IndexOf($runtimePrefix, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
        }
        return $pathMatch -or $cmdMatch
      })
      foreach ($item in $items) {
        Write-Host ("[NOVA] Fechando processo antigo {0} (PID {1})..." -f $item.Name, $item.ProcessId) -ForegroundColor Yellow
        Stop-Process -Id $item.ProcessId -Force -ErrorAction SilentlyContinue
      }
    } catch {}
    Start-Sleep -Milliseconds 800
  }

  function Ensure-PinnedEmulator {
    $emulatorDir = Join-Path $SdkRoot 'emulator'
    $current = Get-EmulatorRevision $emulatorDir
    if ($current -eq $PinnedEmulatorVersion) {
      Write-Host "[OK] Android Emulator fixado em $PinnedEmulatorVersion." -ForegroundColor Green
      return
    }

    Write-Host ''
    Write-Host "[NOVA] Ajustando Android Emulator para versão compatível $PinnedEmulatorVersion..." -ForegroundColor Cyan
    if ($current) {
      Write-Host "[NOVA] Versão atual: $current -> $PinnedEmulatorVersion" -ForegroundColor Yellow
    }

    $needDownload = $true
    if (Test-Path $PinnedEmulatorZip) {
      try {
        $existingHash = (Get-FileHash -Path $PinnedEmulatorZip -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($existingHash -eq $PinnedEmulatorSha256) { $needDownload = $false }
      } catch {}
    }
    if ($needDownload) {
      Remove-Item $PinnedEmulatorZip -Force -ErrorAction SilentlyContinue
      Invoke-Download $PinnedEmulatorUrl $PinnedEmulatorZip 3
    }

    $actualHash = (Get-FileHash -Path $PinnedEmulatorZip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $PinnedEmulatorSha256) {
      Remove-Item $PinnedEmulatorZip -Force -ErrorAction SilentlyContinue
      throw "Checksum inválido do Emulator $PinnedEmulatorVersion. Esperado $PinnedEmulatorSha256, recebido $actualHash."
    }

    $extract = Join-Path $Downloads "emulator-$PinnedEmulatorVersion-extracted"
    Remove-Item $extract -Recurse -Force -ErrorAction SilentlyContinue
    Expand-Archive -Path $PinnedEmulatorZip -DestinationPath $extract -Force
    $newDir = Join-Path $extract 'emulator'
    $newExe = Join-Path $newDir 'emulator.exe'
    if (-not (Test-Path $newExe)) {
      throw "Pacote do Emulator foi extraído, mas emulator.exe não foi encontrado em $newDir"
    }

    $newRevision = Get-EmulatorRevision $newDir
    if ($newRevision -and $newRevision -ne $PinnedEmulatorVersion) {
      throw "Pacote baixado informou versão $newRevision, esperada $PinnedEmulatorVersion."
    }

    Stop-NovaEmulatorProcesses $emulatorDir

    $backup = Join-Path $SdkRoot 'emulator-nova-backup'
    Remove-Item $backup -Recurse -Force -ErrorAction SilentlyContinue
    $oldPackageXml = $null
    if (Test-Path (Join-Path $emulatorDir 'package.xml')) {
      $oldPackageXml = Get-Content (Join-Path $emulatorDir 'package.xml') -Raw -ErrorAction SilentlyContinue
    }

    try {
      if (Test-Path $emulatorDir) {
        Move-Item $emulatorDir $backup -Force
      }
      Move-Item $newDir $emulatorDir -Force

      if ($oldPackageXml) {
        $replacement = '<revision><major>36</major><minor>2</minor><micro>12</micro></revision>'
        $updatedPackage = [regex]::Replace(
          $oldPackageXml,
          '<revision>\s*<major>\d+</major>\s*<minor>\d+</minor>\s*<micro>\d+</micro>\s*</revision>',
          $replacement,
          [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
        )
        Set-Content -Path (Join-Path $emulatorDir 'package.xml') -Value $updatedPackage -Encoding UTF8
      }

      $installedRevision = Get-EmulatorRevision $emulatorDir
      if ($installedRevision -ne $PinnedEmulatorVersion) {
        throw "Emulator substituído, mas source.properties informa '$installedRevision'."
      }

      Remove-Item $backup -Recurse -Force -ErrorAction SilentlyContinue
      Remove-Item $extract -Recurse -Force -ErrorAction SilentlyContinue
      Remove-Item (Join-Path $RuntimeRoot 'boot-ok.marker') -Force -ErrorAction SilentlyContinue
      Remove-Item (Join-Path $AvdHome 'NOVA.avd\snapshots') -Recurse -Force -ErrorAction SilentlyContinue
      Set-Content -Path (Join-Path $RuntimeRoot 'emulator-version.txt') -Value $PinnedEmulatorVersion -Encoding ASCII
      Write-Host "[OK] Android Emulator $PinnedEmulatorVersion instalado como runtime controlado do NOVA." -ForegroundColor Green
    } catch {
      Write-Host '[NOVA] Falha ao trocar Emulator. Restaurando versão anterior...' -ForegroundColor Red
      Remove-Item $emulatorDir -Recurse -Force -ErrorAction SilentlyContinue
      if (Test-Path $backup) { Move-Item $backup $emulatorDir -Force }
      throw
    }
  }

  $JavaExe = Find-OrInstall-Java17
  $env:JAVA_HOME = Split-Path -Parent (Split-Path -Parent $JavaExe)
  $env:PATH = "$env:JAVA_HOME\bin;$env:PATH"
  Write-Host "[NOVA] Java: $JavaExe" -ForegroundColor Green
  Write-Host "[NOVA] Runtime: $RuntimeRoot" -ForegroundColor DarkGray

  $SdkManager = Join-Path $SdkRoot 'cmdline-tools\latest\bin\sdkmanager.bat'
  if (-not (Test-Path $SdkManager)) {
    Write-Host '[NOVA] Baixando Android Command Line Tools oficiais...' -ForegroundColor Cyan
    Remove-Item $ToolsZip -Force -ErrorAction SilentlyContinue
    Invoke-Download $ToolsUrl $ToolsZip

    $actualHash = (Get-FileHash -Path $ToolsZip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $ToolsSha256) {
      Remove-Item $ToolsZip -Force -ErrorAction SilentlyContinue
      throw "Checksum inválido das Command Line Tools. Esperado $ToolsSha256, recebido $actualHash."
    }

    $extractRoot = Join-Path $Downloads 'cmdline-tools-extracted'
    Remove-Item $extractRoot -Recurse -Force -ErrorAction SilentlyContinue
    Expand-Archive -Path $ToolsZip -DestinationPath $extractRoot -Force
    $latest = Join-Path $SdkRoot 'cmdline-tools\latest'
    Remove-Item $latest -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $latest | Out-Null
    Copy-Item (Join-Path $extractRoot 'cmdline-tools\*') $latest -Recurse -Force
  }

  $Package = "system-images;android-$ApiLevel;$ImageFlavor;x86_64"
  $Emulator = Join-Path $SdkRoot 'emulator\emulator.exe'
  $Adb = Join-Path $SdkRoot 'platform-tools\adb.exe'
  $AvdManager = Join-Path $SdkRoot 'cmdline-tools\latest\bin\avdmanager.bat'
  $AvdConfig = Join-Path $AvdHome 'NOVA.avd\config.ini'

  $runtimeComplete = (Test-Path $Emulator) -and (Test-Path $Adb) -and (Test-Path $AvdConfig)
  if (-not $runtimeComplete) {
    Write-Host ''
    Write-Host '[NOVA] Primeira instalação: o Android SDK mostrará as licenças oficiais.' -ForegroundColor Yellow
    Write-Host '[NOVA] Aceite as licenças para continuar. Isso só precisa ser feito uma vez.' -ForegroundColor Yellow
    Write-Host ''
    & $SdkManager "--sdk_root=$SdkRoot" --licenses
    if ($LASTEXITCODE -ne 0) { throw "Licenças do Android SDK não foram concluídas (código $LASTEXITCODE)." }

    Write-Host '[NOVA] Instalando ADB, Emulator base e Android 15 x86_64...' -ForegroundColor Cyan
    & $SdkManager "--sdk_root=$SdkRoot" 'platform-tools' 'emulator' $Package
    if ($LASTEXITCODE -ne 0) { throw "sdkmanager falhou (código $LASTEXITCODE). Consulte $LogFile" }
  } else {
    Write-Host '[OK] SDK/ADB/AVD já presentes. Verificando versão controlada do Emulator...' -ForegroundColor Green
  }

  Ensure-PinnedEmulator
  $Emulator = Join-Path $SdkRoot 'emulator\emulator.exe'

  if (-not (Test-Path $AvdConfig)) {
    Write-Host '[NOVA] Criando dispositivo virtual NOVA...' -ForegroundColor Cyan
    'no' | & $AvdManager create avd --name NOVA --package $Package --device pixel_7 --force
    if ($LASTEXITCODE -ne 0) { throw "avdmanager falhou (código $LASTEXITCODE). Consulte $LogFile" }
  }

  if (-not (Test-Path $AvdConfig)) { throw "AVD criado, mas config.ini não foi encontrado em $AvdConfig" }

  Set-IniValue $AvdConfig 'hw.keyboard' 'yes'
  Set-IniValue $AvdConfig 'hw.gpu.enabled' 'yes'
  Set-IniValue $AvdConfig 'hw.gpu.mode' 'auto'
  Set-IniValue $AvdConfig 'hw.ramSize' '3072'
  Set-IniValue $AvdConfig 'hw.lcd.width' '720'
  Set-IniValue $AvdConfig 'hw.lcd.height' '1280'
  Set-IniValue $AvdConfig 'hw.lcd.density' '320'
  Set-IniValue $AvdConfig 'disk.dataPartition.size' '12G'
  Set-IniValue $AvdConfig 'fastboot.forceColdBoot' 'no'
  Set-IniValue $AvdConfig 'fastboot.forceFastBoot' 'no'

  if (-not (Test-Path $Emulator) -or -not (Test-Path $Adb)) {
    throw 'O Android SDK terminou, mas Emulator ou ADB continuam ausentes.'
  }

  Write-Host '[NOVA] Verificando aceleração de hardware (diagnóstico, sem bloquear)...' -ForegroundColor Cyan
  $accelOutput = & $Emulator -accel-check 2>&1 | Out-String
  $accelExit = $LASTEXITCODE
  Write-Host $accelOutput
  "emulator_version=$PinnedEmulatorVersion`naccel_check_exit=$accelExit`n$accelOutput" | Add-Content $LogFile -Encoding UTF8
  if ($accelExit -eq 0) {
    Write-Host '[OK] Hipervisor detectado pelo Android Emulator.' -ForegroundColor Green
  } else {
    Write-Host '[NOVA] accel-check não confirmou a aceleração. O engine fará o teste real no boot.' -ForegroundColor Yellow
  }

  Write-Host ''
  Write-Host '[OK] Runtime NOVA instalado/verificado com sucesso.' -ForegroundColor Green
  Write-Host "[NOVA] Emulator controlado: $PinnedEmulatorVersion"
  Write-Host "[NOVA] SDK: $SdkRoot"
  Write-Host "[NOVA] AVD: $AvdHome\NOVA.avd"
  Write-Host "[NOVA] Log: $LogFile" -ForegroundColor DarkGray
} catch {
  Write-Host ''
  Write-Host "[ERRO NOVA] $($_.Exception.Message)" -ForegroundColor Red
  Write-Host "[NOVA] Log completo: $LogFile" -ForegroundColor Yellow
  throw
} finally {
  try { Stop-Transcript | Out-Null } catch {}
  if (-not $NoPause) { Read-Host 'Pressione Enter para fechar' | Out-Null }
}
