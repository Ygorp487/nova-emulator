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

> The Android runtime image and QEMU binaries are intentionally not committed to the repository. They will be downloaded/provisioned by the installer in a later milestone.

## Development

Requirements:

- Node.js 20+
- Rust stable
- Tauri v2 prerequisites for Windows

```bash
npm install
npm run tauri dev
```

## Project structure

```text
src/                 React launcher UI
src-tauri/           Rust/Tauri desktop backend
engine/              Runtime configuration and Windows scripts
```

## Status

Early development. Not yet a complete Android emulator.
