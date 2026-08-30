param(
  [ValidateSet('eco','balanced','performance')]
  [string]$Profile = 'balanced'
)

$ErrorActionPreference = 'Stop'
$EngineRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Qemu = Join-Path $EngineRoot 'engine\runtime\qemu\qemu-system-x86_64.exe'
$AndroidDir = Join-Path $EngineRoot 'engine\runtime\android'

if (-not (Test-Path $Qemu)) {
  throw "QEMU runtime not found: $Qemu"
}

$settings = switch ($Profile) {
  'eco' { @{ Cpu = 2; Ram = 2048 } }
  'performance' { @{ Cpu = 4; Ram = 6144 } }
  default { @{ Cpu = 4; Ram = 4096 } }
}

$system = Join-Path $AndroidDir 'system.img'
$userdata = Join-Path $AndroidDir 'userdata.qcow2'

if (-not (Test-Path $system)) {
  throw "Android system image not found: $system"
}

$args = @(
  '-accel', 'whpx',
  '-machine', 'q35',
  '-smp', $settings.Cpu,
  '-m', $settings.Ram,
  '-drive', "file=$system,format=raw,readonly=on",
  '-drive', "file=$userdata,format=qcow2",
  '-netdev', 'user,id=n1,hostfwd=tcp::5555-:5555',
  '-device', 'virtio-net-pci,netdev=n1',
  '-display', 'sdl,gl=on'
)

Start-Process -FilePath $Qemu -ArgumentList $args -WorkingDirectory $EngineRoot
