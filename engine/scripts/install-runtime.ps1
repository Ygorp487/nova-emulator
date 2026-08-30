param(
  [ValidateRange(34, 36)]
  [int]$ApiLevel = 35,
  [ValidateSet('default','google_apis')]
  [string]$ImageFlavor = 'default'
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$RuntimeRoot = Join-Path $RepoRoot 'engine\runtime'
$SdkRoot = Join-Path $RuntimeRoot 'sdk'
$AvdHome = Join-Path $RuntimeRoot 'avd'
$Downloads = Join-Path $RuntimeRoot 'downloads'
$ToolsVersion = '15859902'
$ToolsZip = Join-Path $Downloads 'commandlinetools-win.zip'
$ToolsUrl = "https://dl.google.com/android/repository/commandlinetools-win-$ToolsVersion`_latest.zip"
$ToolsSha256 = '90ae805d20434428bffcb699c290860f19bb5f66a67e6b330067e3de801fb04a'

New-Item -ItemType Directory -Force -Path $RuntimeRoot, $SdkRoot, $AvdHome, $Downloads | Out-Null
$env:ANDROID_SDK_ROOT = $SdkRoot
$env:ANDROID_HOME = $SdkRoot
$env:ANDROID_AVD_HOME = $AvdHome

function Find-Java {
  $existing = Get-Command java.exe -ErrorAction SilentlyContinue
  if ($existing) { return $existing.Source }

  Write-Host '[NOVA] Java 17 não encontrado. Tentando instalar Microsoft OpenJDK 17 via winget...' -ForegroundColor Cyan
  $winget = Get-Command winget.exe -ErrorAction SilentlyContinue
  if (-not $winget) {
    throw 'Java 17 é necessário e o winget não está disponível. Instale Microsoft OpenJDK 17 e execute novamente.'
  }

  & $winget.Source install -e --id Microsoft.OpenJDK.17 --silent --accept-package-agreements --accept-source-agreements
  if ($LASTEXITCODE -ne 0) {
    throw "Falha ao instalar Java 17 via winget (código $LASTEXITCODE)."
  }

  $javaCandidates = @(
    Get-ChildItem "$env:ProgramFiles\Microsoft" -Directory -Filter 'jdk-17*' -ErrorAction SilentlyContinue |
      Sort-Object LastWriteTime -Descending |
      ForEach-Object { Join-Path $_.FullName 'bin\java.exe' }
  )
  $java = $javaCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
  if (-not $java) {
    throw 'Java 17 foi instalado, mas java.exe não foi localizado. Feche e abra o instalador novamente.'
  }
  return $java
}

function Set-IniValue([string]$Path, [string]$Key, [string]$Value) {
  $lines = if (Test-Path $Path) { Get-Content $Path } else { @() }
  $pattern = '^' + [regex]::Escape($Key) + '='
  $found = $false
  $updated = foreach ($line in $lines) {
    if ($line -match $pattern) {
      $found = $true
      "$Key=$Value"
    } else {
      $line
    }
  }
  if (-not $found) { $updated += "$Key=$Value" }
  Set-Content -Path $Path -Value $updated -Encoding ASCII
}

$JavaExe = Find-Java
$env:JAVA_HOME = Split-Path -Parent (Split-Path -Parent $JavaExe)
$env:PATH = "$env:JAVA_HOME\bin;$env:PATH"
Write-Host "[NOVA] Java: $JavaExe" -ForegroundColor DarkGray

$SdkManager = Join-Path $SdkRoot 'cmdline-tools\latest\bin\sdkmanager.bat'
if (-not (Test-Path $SdkManager)) {
  Write-Host '[NOVA] Baixando Android Command Line Tools oficiais...' -ForegroundColor Cyan
  Invoke-WebRequest -Uri $ToolsUrl -OutFile $ToolsZip

  $actualHash = (Get-FileHash -Path $ToolsZip -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($actualHash -ne $ToolsSha256) {
    Remove-Item $ToolsZip -Force -ErrorAction SilentlyContinue
    throw "Checksum inválido das Command Line Tools. Esperado $ToolsSha256, recebido $actualHash."
  }

  $extractRoot = Join-Path $Downloads 'cmdline-tools-extracted'
  Remove-Item $extractRoot -Recurse -Force -ErrorAction SilentlyContinue
  Expand-Archive -Path $ToolsZip -DestinationPath $extractRoot -Force
  $latest = Join-Path $SdkRoot 'cmdline-tools\latest'
  New-Item -ItemType Directory -Force -Path $latest | Out-Null
  Copy-Item (Join-Path $extractRoot 'cmdline-tools\*') $latest -Recurse -Force
}

$Package = "system-images;android-$ApiLevel;$ImageFlavor;x86_64"
Write-Host ''
Write-Host '[NOVA] O Android SDK exige que você leia/aceite as licenças antes do download.' -ForegroundColor Yellow
Write-Host '[NOVA] Responda aos prompts abaixo. O NOVA não aceita licenças em seu nome.' -ForegroundColor Yellow
Write-Host ''
& $SdkManager "--sdk_root=$SdkRoot" --licenses
if ($LASTEXITCODE -ne 0) { throw "Licenças não concluídas (código $LASTEXITCODE)." }

Write-Host '[NOVA] Instalando Emulator (QEMU), ADB e imagem Android x86_64...' -ForegroundColor Cyan
& $SdkManager "--sdk_root=$SdkRoot" 'platform-tools' 'emulator' $Package
if ($LASTEXITCODE -ne 0) { throw "sdkmanager falhou (código $LASTEXITCODE)." }

$AvdManager = Join-Path $SdkRoot 'cmdline-tools\latest\bin\avdmanager.bat'
$AvdConfig = Join-Path $AvdHome 'NOVA.avd\config.ini'
if (-not (Test-Path $AvdConfig)) {
  Write-Host '[NOVA] Criando AVD NOVA...' -ForegroundColor Cyan
  'no' | & $AvdManager create avd --name NOVA --package $Package --device pixel_7 --force
  if ($LASTEXITCODE -ne 0) { throw "avdmanager falhou (código $LASTEXITCODE)." }
}

if (-not (Test-Path $AvdConfig)) {
  throw "AVD foi criado, mas config.ini não foi encontrado em $AvdConfig"
}

Set-IniValue $AvdConfig 'hw.keyboard' 'yes'
Set-IniValue $AvdConfig 'hw.gpu.enabled' 'yes'
Set-IniValue $AvdConfig 'hw.gpu.mode' 'host'
Set-IniValue $AvdConfig 'hw.ramSize' '4096'
Set-IniValue $AvdConfig 'disk.dataPartition.size' '16G'
Set-IniValue $AvdConfig 'fastboot.forceColdBoot' 'no'
Set-IniValue $AvdConfig 'fastboot.forceFastBoot' 'yes'

$Emulator = Join-Path $SdkRoot 'emulator\emulator.exe'
Write-Host ''
Write-Host '[NOVA] Verificando aceleração...' -ForegroundColor Cyan
$accelOutput = & $Emulator -accel-check 2>&1 | Out-String
Write-Host $accelOutput

if ($LASTEXITCODE -ne 0 -or $accelOutput -notmatch '(?i)usable') {
  Write-Host '[NOVA] Windows Hypervisor Platform ainda não parece utilizável.' -ForegroundColor Yellow
  $answer = Read-Host 'Deseja abrir a ativação do WHPX como administrador agora? (s/N)'
  if ($answer -match '^(s|sim|y|yes)$') {
    $EnableScript = Join-Path $PSScriptRoot 'enable-whpx.ps1'
    Start-Process powershell.exe -Verb RunAs -Wait -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',"`"$EnableScript`"")
    Write-Host '[NOVA] Se o Windows pedir reinicialização, reinicie antes de iniciar o Android.' -ForegroundColor Yellow
  }
}

Write-Host ''
Write-Host '[NOVA] Runtime instalado com sucesso.' -ForegroundColor Green
Write-Host "[NOVA] SDK: $SdkRoot"
Write-Host "[NOVA] AVD: $AvdHome\NOVA.avd"
Write-Host '[NOVA] Volte ao aplicativo e clique em Atualizar.'
Read-Host 'Pressione Enter para fechar'
