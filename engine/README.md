# NOVA Engine

The NOVA engine is provisioned locally and third-party binaries are not committed to Git.

## Runtime layout

```text
engine/runtime/
  sdk/
    emulator/emulator.exe
    platform-tools/adb.exe
    system-images/android-35/default/x86_64/
  avd/
    NOVA.ini
    NOVA.avd/
```

`install-runtime.ps1` downloads the official Android command-line tools, verifies the pinned SHA-256, asks the user to accept the Android SDK licenses, installs Emulator/QEMU + ADB + an Android 15 x86_64 system image, and creates the `NOVA` AVD.

The launcher uses Windows Hypervisor Platform acceleration and GPU acceleration when available.

## Important limitation

The current MVP is x86_64-only and does not include ARM binary translation. Apps/games that ship only ARM native code may not run yet. ARM compatibility is a separate engine milestone.
