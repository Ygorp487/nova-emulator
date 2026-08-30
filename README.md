# NOVA Emulator

NOVA is an experimental lightweight Android emulator shell for Windows, focused on low overhead, gaming-oriented controls and a clean desktop experience.

## Current milestone

MVP 0.1 lays the desktop foundation:

- Tauri + React desktop shell
- Rust backend commands
- Performance profile UI
- Engine status/launch hooks
- QEMU/WHPX configuration placeholders
- APK installation flow placeholder
- Windows CI build validation

> The Android runtime image and QEMU binaries are intentionally not committed to the repository. They will be downloaded/provisioned by the installer in a later milestone.

## Development

Requirements:

- Node.js 22+
- Rust stable
- Tauri v2 prerequisites for Windows

On Windows, you can run:

```powershell
.\setup-dev.ps1
npm run tauri dev
```

Or manually:

```bash
npm install
npm run tauri dev
```

## Project structure

```text
src/                 React launcher UI
src-tauri/           Rust/Tauri desktop backend
engine/              Runtime configuration and Windows scripts
.github/workflows/   Windows build validation
```

## Engine runtime

The runtime will live outside Git history under `engine/runtime/`. The next milestone will provision QEMU, validate WHPX, prepare the Android x86_64 image and connect ADB/video/input to the desktop shell.

## Status

Early development. The launcher/backend foundation is in place; this is not yet a complete Android emulator.
