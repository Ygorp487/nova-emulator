# NOVA Emulator

NOVA is an experimental lightweight Android emulator shell for Windows focused on low overhead, gaming-oriented controls and a clean desktop experience.

## Current milestone: Engine MVP 0.2

- Tauri + React desktop shell
- Rust backend commands
- Local runtime provisioning without Android Studio
- Official Android Emulator/QEMU runtime
- Windows Hypervisor Platform (WHPX) validation
- Android 15 / API 35 x86_64 AVD creation
- GPU acceleration (`host` / `auto` profiles)
- ADB health detection
- Start/stop controls from the NOVA launcher
- Windows CI build validation

The runtime is **not** stored in Git. It is installed locally under `engine/runtime/`.

## Development

Requirements:

- Windows 10/11 x64
- Node.js 22+
- Rust stable
- Tauri v2 Windows prerequisites
- CPU virtualization enabled in BIOS/UEFI

```powershell
.\setup-dev.ps1
npm run tauri dev
```

Inside NOVA, click **Instalar Runtime**. The installer downloads the Android command-line tools from Google, verifies their SHA-256, asks you to accept the Android SDK licenses, installs Emulator + Platform Tools + Android x86_64 image, creates the NOVA AVD and checks hardware acceleration.

If WHPX is disabled, the installer can open the included administrator script to enable Windows Hypervisor Platform. A Windows restart can be required.

## Project structure

```text
src/                 React launcher UI
src-tauri/           Rust/Tauri desktop backend
engine/config/       NOVA engine defaults
engine/scripts/      Runtime installer and launcher
engine/runtime/      Downloaded SDK/AVD (gitignored)
.github/workflows/   Windows build validation
```

## Architecture note

The Android Emulator itself uses QEMU-based virtualization. Using its official runtime for this milestone gives NOVA a correct Android kernel, ramdisk, system image, ADB plumbing and WHPX integration while we build our own launcher, input layer and optimizations around it.

## Known limitation

MVP 0.2 uses an x86_64 Android image and does not yet provide ARM native-code translation. Some Android games ship only ARM libraries and will require a later ARM-compatibility milestone.
