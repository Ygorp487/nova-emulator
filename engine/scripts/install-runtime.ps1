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
  $RuntimeRoot = Join-Path $env:LOCALAPPDATA 'NOVA Emulator\engine\runtime'
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
        Write-Host "[NOVA] Download falhou. Tentando novamente em 2 segundos..." -ForegroundColor Yellow
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

    Write-Host '[NOVA] Instalando Emulator/QEMU, ADB e Android x86_64...' -ForegroundColor Cyan
    & $SdkManager "--sdk_root=$SdkRoot" 'platform-tools' 'emulator' $Package
    if ($LASTEXITCODE -ne 0) { throw "sdkmanager falhou (código $LASTEXITCODE). Consulte $LogFile" }
  } else {
    Write-Host '[OK] Runtime Android já instalado. Download ignorado.' -ForegroundColor Green
  }

  if (-not (Test-Path $AvdConfig)) {
    Write-Host '[NOVA] Criando dispositivo virtual NOVA...' -ForegroundColor Cyan
    'no' | & $AvdManager create avd --name NOVA --package $Package --device pixel_7 --force
    if ($LASTEXITCODE -ne 0) { throw "avdmanager falhou (código $LASTEXITCODE). Consulte $LogFile" }
  }

  if (-not (Test-Path $AvdConfig)) { throw "AVD criado, mas config.ini não foi encontrado em $AvdConfig" }

  Set-IniValue $AvdConfig 'hw.keyboard' 'yes'
  Set-IniValue $AvdConfig 'hw.gpu.enabled' 'yes'
  Set-IniValue $AvdConfig 'hw.gpu.mode' 'host'
  Set-IniValue $AvdConfig 'hw.ramSize' '4096'
  Set-IniValue $AvdConfig 'disk.dataPartition.size' '16G'
  Set-IniValue $AvdConfig 'fastboot.forceColdBoot' 'no'
  Set-IniValue $AvdConfig 'fastboot.forceFastBoot' 'yes'

  if (-not (Test-Path $Emulator) -or -not (Test-Path $Adb)) {
    throw 'O Android SDK terminou, mas Emulator ou ADB continuam ausentes.'
  }

  Write-Host '[NOVA] Verificando aceleração de hardware...' -ForegroundColor Cyan
  $accelOutput = & $Emulator -accel-check 2>&1 | Out-String
  Write-Host $accelOutput

  if ($LASTEXITCODE -ne 0 -or $accelOutput -notmatch '(?i)usable') {
    Write-Host '[NOVA] O runtime foi instalado, mas WHPX ainda não está utilizável.' -ForegroundColor Yellow
    $EnableScript = Join-Path $PSScriptRoot 'enable-whpx.ps1'
    if (Test-Path $EnableScript) {
      $answer = Read-Host 'Ativar Windows Hypervisor Platform agora? (s/N)'
      if ($answer -match '^(s|sim|y|yes)$') {
        Start-Process powershell.exe -Verb RunAs -Wait -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',"`"$EnableScript`"")
        Write-Host '[NOVA] Se o Windows pedir reinicialização, reinicie o PC antes de iniciar o Android.' -ForegroundColor Yellow
      }
    }
  }

  Write-Host ''
  Write-Host '[OK] Runtime NOVA instalado/verificado com sucesso.' -ForegroundColor Green
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
