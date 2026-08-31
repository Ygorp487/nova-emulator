param(
  [ValidateSet('prepare','run','build')]
  [string]$Mode = 'prepare'
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$RepoRoot = Split-Path -Parent $PSScriptRoot
$TempRoot = Join-Path $env:TEMP 'NOVA-Emulator-Setup'
$RuntimeRoot = Join-Path $env:LOCALAPPDATA 'NOVA\Runtime'
$LegacyRuntimeRoot = Join-Path $env:LOCALAPPDATA 'NOVA Emulator\engine\runtime'
$TauriTargetRoot = Join-Path $RepoRoot 'src-tauri\target'
New-Item -ItemType Directory -Force -Path $TempRoot | Out-Null

function Write-Step([string]$Text) {
  Write-Host "`n[NOVA] $Text" -ForegroundColor Cyan
}

function Refresh-Path {
  $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine')
  $user = [Environment]::GetEnvironmentVariable('Path', 'User')
  $extra = @(
    (Join-Path $env:USERPROFILE '.cargo\bin'),
    "$env:ProgramFiles\nodejs"
  ) -join ';'
  $env:PATH = "$machine;$user;$extra"
}

function Test-Admin {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = New-Object Security.Principal.WindowsPrincipal($identity)
  return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Test-VCTools {
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  if (-not (Test-Path $vswhere)) { return $false }
  $install = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
  return -not [string]::IsNullOrWhiteSpace(($install | Out-String))
}

function Test-WebView2 {
  $paths = @(
    "${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application",
    "$env:ProgramFiles\Microsoft\EdgeWebView\Application"
  )
  return [bool]($paths | Where-Object { Test-Path $_ } | Select-Object -First 1)
}

function Invoke-WingetInstall([string]$Id, [string]$Label, [string]$Override = '') {
  $winget = Get-Command winget.exe -ErrorAction SilentlyContinue
  if (-not $winget) { return $false }

  Write-Step "Instalando $Label..."
  $args = @('install','--id',$Id,'-e','--silent','--accept-package-agreements','--accept-source-agreements')
  if ($Override) { $args += @('--override', $Override) }
  & $winget.Source @args | Out-Host
  if ($LASTEXITCODE -eq 0) { return $true }

  Write-Host "[NOVA] winget não conseguiu instalar $Label. Tentando método alternativo..." -ForegroundColor Yellow
  return $false
}

function Install-Node {
  if (Get-Command node.exe -ErrorAction SilentlyContinue) { return }
  if (Invoke-WingetInstall 'OpenJS.NodeJS.LTS' 'Node.js LTS') { Refresh-Path; return }

  Write-Step 'Baixando Node.js LTS oficial...'
  $index = Invoke-RestMethod 'https://nodejs.org/dist/index.json'
  $release = $index | Where-Object { $_.lts } | Select-Object -First 1
  if (-not $release) { throw 'Não foi possível descobrir a versão LTS do Node.js.' }
  $version = $release.version
  $msi = Join-Path $TempRoot "node-$version-x64.msi"
  Invoke-WebRequest "https://nodejs.org/dist/$version/node-$version-x64.msi" -OutFile $msi -UseBasicParsing
  $p = Start-Process msiexec.exe -Wait -PassThru -ArgumentList @('/i',"`"$msi`"",'/qn','/norestart')
  if ($p.ExitCode -ne 0) { throw "Falha ao instalar Node.js (código $($p.ExitCode))." }
  Refresh-Path
}

function Install-Rust {
  if (Get-Command cargo.exe -ErrorAction SilentlyContinue) { return }
  if (Invoke-WingetInstall 'Rustlang.Rustup' 'Rust / rustup') {
    Refresh-Path
    if (Get-Command rustup.exe -ErrorAction SilentlyContinue) { rustup default stable | Out-Host }
    return
  }

  Write-Step 'Baixando rustup oficial...'
  $rustup = Join-Path $TempRoot 'rustup-init.exe'
  Invoke-WebRequest 'https://win.rustup.rs/x86_64' -OutFile $rustup
  $p = Start-Process $rustup -Wait -PassThru -ArgumentList @('-y','--default-toolchain','stable','--profile','minimal')
  if ($p.ExitCode -ne 0) { throw "Falha ao instalar Rust (código $($p.ExitCode))." }
  Refresh-Path
}

function Install-VCTools {
  if (Test-VCTools) { return }
  $override = '--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'
  if (Invoke-WingetInstall 'Microsoft.VisualStudio.2022.BuildTools' 'Visual C++ Build Tools' $override) { return }

  Write-Step 'Baixando Visual Studio Build Tools oficial...'
  $installer = Join-Path $TempRoot 'vs_BuildTools.exe'
  Invoke-WebRequest 'https://aka.ms/vs/17/release/vs_BuildTools.exe' -OutFile $installer
  $p = Start-Process $installer -Wait -PassThru -ArgumentList @('--wait','--passive','--norestart','--add','Microsoft.VisualStudio.Workload.VCTools','--includeRecommended')
  if ($p.ExitCode -notin @(0,3010)) { throw "Falha ao instalar Visual C++ Build Tools (código $($p.ExitCode))." }
}

function Install-WebView2 {
  if (Test-WebView2) { return }
  if (Invoke-WingetInstall 'Microsoft.EdgeWebView2Runtime' 'Microsoft Edge WebView2 Runtime') { return }

  Write-Step 'Baixando WebView2 Runtime oficial...'
  $installer = Join-Path $TempRoot 'MicrosoftEdgeWebview2Setup.exe'
  Invoke-WebRequest 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile $installer
  $p = Start-Process $installer -Wait -PassThru -ArgumentList @('/silent','/install')
  if ($p.ExitCode -ne 0) { throw "Falha ao instalar WebView2 Runtime (código $($p.ExitCode))." }
}

function Stop-NovaRuntimeProcesses {
  $targets = @()
  try {
    $targets = @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {
      if ($_.ProcessId -eq $PID) { return $false }
      $pathMatch = $false
      $commandMatch = $false
      if (-not [string]::IsNullOrWhiteSpace($_.ExecutablePath)) {
        $pathMatch = $_.ExecutablePath.StartsWith($LegacyRuntimeRoot, [System.StringComparison]::OrdinalIgnoreCase)
      }
      if (-not [string]::IsNullOrWhiteSpace($_.CommandLine)) {
        $commandMatch = $_.CommandLine.IndexOf($LegacyRuntimeRoot, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
      }
      $nameMatch = @('nova-emulator.exe','NOVA Emulator.exe') -contains $_.Name
      return $pathMatch -or $commandMatch -or $nameMatch
    })
  } catch {}

  foreach ($process in $targets) {
    try {
      Write-Host ("[NOVA] Fechando {0} (PID {1})..." -f $process.Name, $process.ProcessId) -ForegroundColor Yellow
      Stop-Process -Id $process.ProcessId -Force -ErrorAction Stop
    } catch {}
  }
  Start-Sleep -Milliseconds 700
}

function Stop-NovaBuildProcesses {
  Write-Host '[NOVA] Liberando processos antigos de compilação do NOVA...' -ForegroundColor DarkGray
  $allowedNames = @(
    'node.exe','cargo.exe','rustc.exe','rust-analyzer.exe','esbuild.exe',
    'nova-emulator.exe','NOVA Emulator.exe'
  )
  $targets = @()

  try {
    $targets = @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {
      if ($_.ProcessId -eq $PID) { return $false }
      if ($allowedNames -notcontains $_.Name) { return $false }

      $pathInTarget = $false
      $commandInRepo = $false
      if (-not [string]::IsNullOrWhiteSpace($_.ExecutablePath)) {
        $pathInTarget = $_.ExecutablePath.StartsWith($TauriTargetRoot, [System.StringComparison]::OrdinalIgnoreCase)
      }
      if (-not [string]::IsNullOrWhiteSpace($_.CommandLine)) {
        $commandInRepo = $_.CommandLine.IndexOf($RepoRoot, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
      }
      return $pathInTarget -or $commandInRepo
    })
  } catch {
    Write-Host '[NOVA] Não foi possível consultar processos de build via CIM.' -ForegroundColor Yellow
  }

  foreach ($process in $targets) {
    try {
      Write-Host ("[NOVA] Encerrando processo antigo {0} (PID {1})..." -f $process.Name, $process.ProcessId) -ForegroundColor Yellow
      Stop-Process -Id $process.ProcessId -Force -ErrorAction Stop
    } catch {
      Write-Host ("[NOVA] Processo PID {0} já encerrou ou não pôde ser fechado." -f $process.ProcessId) -ForegroundColor DarkGray
    }
  }
  Start-Sleep -Milliseconds 900
}

function Reset-NovaDebugArtifacts {
  $debugRoot = Join-Path $TauriTargetRoot 'debug'
  if (-not (Test-Path $debugRoot)) { return }

  Write-Host '[NOVA] Limpando artefatos temporários de desenvolvimento...' -ForegroundColor DarkGray
  $lastError = $null
  for ($attempt = 1; $attempt -le 5; $attempt++) {
    try {
      Remove-Item $debugRoot -Recurse -Force -ErrorAction Stop
      Write-Host '[OK] Cache Rust de desenvolvimento liberado.' -ForegroundColor Green
      return
    } catch {
      $lastError = $_.Exception.Message
      Stop-NovaBuildProcesses
      Start-Sleep -Milliseconds (500 * $attempt)
    }
  }
  throw "Não foi possível liberar src-tauri\target\debug após 5 tentativas. Detalhe: $lastError"
}

function Migrate-LegacyRuntime {
  if ((Test-Path $LegacyRuntimeRoot) -and (-not (Test-Path $RuntimeRoot))) {
    Write-Step 'Migrando runtime existente para caminho compatível'
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $RuntimeRoot) | Out-Null
    $lastError = $null
    for ($attempt = 1; $attempt -le 5; $attempt++) {
      try {
        Stop-NovaRuntimeProcesses
        Move-Item -Path $LegacyRuntimeRoot -Destination $RuntimeRoot -Force -ErrorAction Stop
        Write-Host '[OK] Runtime antigo migrado automaticamente.' -ForegroundColor Green
        return
      } catch {
        $lastError = $_.Exception.Message
        Start-Sleep -Seconds 2
      }
    }
    throw "Não foi possível liberar/migrar o runtime após 5 tentativas. Detalhe: $lastError"
  }
}

function Test-NovaRuntime {
  $emulator = Join-Path $RuntimeRoot 'sdk\emulator\emulator.exe'
  $adb = Join-Path $RuntimeRoot 'sdk\platform-tools\adb.exe'
  $avd = Join-Path $RuntimeRoot 'avd\NOVA.avd\config.ini'
  return (Test-Path $emulator) -and (Test-Path $adb) -and (Test-Path $avd)
}

function Ensure-NovaRuntime {
  Migrate-LegacyRuntime
  if (Test-NovaRuntime) {
    Write-Host '[OK] Runtime Android do NOVA já está instalado.' -ForegroundColor Green
    return
  }

  Write-Step 'Preparando runtime Android do NOVA'
  Write-Host '[NOVA] Na primeira vez, o Android SDK pedirá a aceitação das licenças oficiais.' -ForegroundColor Yellow
  $runtimeInstaller = Join-Path $RepoRoot 'engine\scripts\install-runtime.ps1'
  if (-not (Test-Path $runtimeInstaller)) { throw "Instalador do runtime não encontrado: $runtimeInstaller" }
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $runtimeInstaller -RuntimeRoot $RuntimeRoot -NoPause
  if ($LASTEXITCODE -ne 0) { throw "A instalação do runtime falhou (código $LASTEXITCODE)." }
  if (-not (Test-NovaRuntime)) { throw 'O instalador terminou, mas o runtime NOVA continua incompleto.' }
  Write-Host '[OK] Runtime Android pronto.' -ForegroundColor Green
}

Refresh-Path
$needsElevation = (-not (Get-Command node.exe -ErrorAction SilentlyContinue)) -or (-not (Test-VCTools)) -or (-not (Test-WebView2))
if ($needsElevation -and -not (Test-Admin)) {
  Write-Host '[NOVA] Algumas dependências do Windows precisam de permissão de administrador.' -ForegroundColor Yellow
  $args = "-NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Mode $Mode"
  $process = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -ArgumentList $args
  exit $process.ExitCode
}

Write-Step 'Verificando dependências do Windows'
Install-Node
Install-Rust
Install-VCTools
Install-WebView2
Refresh-Path

if (-not (Get-Command node.exe -ErrorAction SilentlyContinue)) { throw 'Node.js não foi encontrado após a instalação.' }
if (-not (Get-Command npm.cmd -ErrorAction SilentlyContinue)) { throw 'npm não foi encontrado após a instalação.' }
if (-not (Get-Command cargo.exe -ErrorAction SilentlyContinue)) { throw 'Cargo/Rust não foi encontrado após a instalação.' }
if (-not (Test-VCTools)) { throw 'Visual C++ Build Tools não foi detectado após a instalação.' }

Write-Host "[OK] Node:  $(node --version)" -ForegroundColor Green
Write-Host "[OK] npm:   $(npm.cmd --version)" -ForegroundColor Green
Write-Host "[OK] Rust:  $(rustc --version)" -ForegroundColor Green
Write-Host '[OK] C++ Build Tools detectado' -ForegroundColor Green
if (Test-WebView2) { Write-Host '[OK] WebView2 detectado' -ForegroundColor Green }

Ensure-NovaRuntime
Stop-NovaBuildProcesses
Reset-NovaDebugArtifacts

Push-Location $RepoRoot
try {
  Write-Step 'Instalando/atualizando dependências do NOVA'
  & npm.cmd install
  if ($LASTEXITCODE -ne 0) { throw "npm install falhou (código $LASTEXITCODE)." }

  if ($Mode -eq 'prepare') {
    Write-Step 'Validando backend Rust'
    Push-Location (Join-Path $RepoRoot 'src-tauri')
    try {
      & cargo.exe check
      if ($LASTEXITCODE -ne 0) { throw "cargo check falhou (código $LASTEXITCODE)." }
    } finally { Pop-Location }
    Write-Host "`n[NOVA] Tudo pronto, incluindo o runtime Android." -ForegroundColor Green
    exit 0
  }

  if ($Mode -eq 'run') {
    Write-Step 'Abrindo NOVA em modo de desenvolvimento'
    Write-Host '[NOVA] Este modo é apenas para desenvolvimento. O fluxo normal usa o instalador release.' -ForegroundColor Yellow
    & npm.cmd run tauri dev
    exit $LASTEXITCODE
  }

  if ($Mode -eq 'build') {
    Write-Step 'Gerando instalador RELEASE do NOVA'
    Stop-NovaBuildProcesses
    & npm.cmd run tauri build -- --bundles nsis
    if ($LASTEXITCODE -ne 0) { throw "tauri build falhou (código $LASTEXITCODE)." }

    $bundleDir = Join-Path $RepoRoot 'src-tauri\target\release\bundle\nsis'
    $setup = Get-ChildItem $bundleDir -Filter '*.exe' -File -ErrorAction SilentlyContinue |
      Sort-Object LastWriteTime -Descending |
      Select-Object -First 1
    if (-not $setup) { throw "Build terminou, mas o instalador não foi encontrado em $bundleDir" }

    $output = Join-Path $RepoRoot 'NOVA-BUILD'
    New-Item -ItemType Directory -Force -Path $output | Out-Null
    $final = Join-Path $output 'NOVA-Setup.exe'
    Copy-Item $setup.FullName $final -Force

    Write-Host "`n[NOVA] EXE GERADO COM SUCESSO:" -ForegroundColor Green
    Write-Host $final -ForegroundColor White
    Write-Host "[NOVA] Runtime Android: $RuntimeRoot" -ForegroundColor Green
    Start-Process explorer.exe -ArgumentList "/select,`"$final`""
  }
} finally {
  Pop-Location
}
