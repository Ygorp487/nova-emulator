# NOVA Engine

This folder contains only configuration and orchestration code. Large third-party/runtime binaries are not stored in Git.

Expected runtime layout:

```text
engine/runtime/
  qemu/qemu-system-x86_64.exe
  android/system.img
  android/userdata.qcow2
```

The Windows launcher targets WHPX acceleration. The provisioning milestone will validate Windows Hypervisor Platform, download verified runtime components, create userdata storage and expose ADB to the desktop shell.
