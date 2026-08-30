$ErrorActionPreference = 'Stop'

function Require-Command($Name, $Help) {
  if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
    Write-Host "[NOVA] Missing: $Name" -ForegroundColor Red
    Write-Host $Help -ForegroundColor Yellow
    exit 1
  }
}

Write-Host "NOVA Emulator - Development Setup" -ForegroundColor Cyan
Require-Command 'node' 'Install Node.js 22 LTS or newer.'
Require-Command 'npm' 'npm is installed with Node.js.'
Require-Command 'cargo' 'Install Rust using rustup from https://rustup.rs.'

Write-Host "[1/2] Installing npm dependencies..." -ForegroundColor Cyan
npm install

Write-Host "[2/2] Checking Rust backend..." -ForegroundColor Cyan
Push-Location src-tauri
cargo check
Pop-Location

Write-Host "Setup complete. Run: npm run tauri dev" -ForegroundColor Green
