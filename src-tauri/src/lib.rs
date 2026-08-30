use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineState {
    state: String,
    message: String,
    runtime_found: bool,
    adb_found: bool,
    avd_found: bool,
    running: bool,
    boot_complete: bool,
    acceleration: String,
}

fn project_root() -> PathBuf {
    if cfg!(debug_assertions) {
        return PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
    }

    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn sdk_root(root: &Path) -> PathBuf {
    root.join("engine").join("runtime").join("sdk")
}

fn emulator_path(root: &Path) -> PathBuf {
    sdk_root(root).join("emulator").join("emulator.exe")
}

fn adb_path(root: &Path) -> PathBuf {
    sdk_root(root).join("platform-tools").join("adb.exe")
}

fn avd_home(root: &Path) -> PathBuf {
    root.join("engine").join("runtime").join("avd")
}

fn avd_exists(root: &Path) -> bool {
    avd_home(root).join("NOVA.avd").join("config.ini").exists()
}

fn acceleration_status(root: &Path) -> String {
    let emulator = emulator_path(root);
    if !emulator.exists() {
        return "runtime não instalado".into();
    }

    match Command::new(emulator).arg("-accel-check").output() {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            let normalized = text.trim().replace('\r', "").replace('\n', " · ");
            if normalized.is_empty() {
                "aceleração não detectada".into()
            } else {
                normalized
            }
        }
        Err(error) => format!("falha no accel-check: {error}"),
    }
}

fn adb_output(root: &Path, args: &[&str]) -> Option<String> {
    let adb = adb_path(root);
    if !adb.exists() {
        return None;
    }

    Command::new(adb)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn android_running(root: &Path) -> bool {
    adb_output(root, &["-s", "emulator-5554", "get-state"])
        .map(|state| state.eq_ignore_ascii_case("device"))
        .unwrap_or(false)
}

fn boot_complete(root: &Path) -> bool {
    if !android_running(root) {
        return false;
    }

    adb_output(
        root,
        &["-s", "emulator-5554", "shell", "getprop", "sys.boot_completed"],
    )
    .map(|value| value == "1")
    .unwrap_or(false)
}

fn android_version(root: &Path) -> Option<String> {
    adb_output(
        root,
        &["-s", "emulator-5554", "shell", "getprop", "ro.build.version.release"],
    )
}

fn collect_state() -> EngineState {
    let root = project_root();
    let runtime_found = emulator_path(&root).exists();
    let adb_found = adb_path(&root).exists();
    let avd_found = avd_exists(&root);
    let running = android_running(&root);
    let boot_complete = boot_complete(&root);
    let acceleration = acceleration_status(&root);
    let acceleration_ok = acceleration.to_ascii_lowercase().contains("usable");

    if running && boot_complete {
        let version = android_version(&root).unwrap_or_else(|| "Android".into());
        return EngineState {
            state: "running".into(),
            message: format!("Android {version} está pronto e conectado via ADB."),
            runtime_found,
            adb_found,
            avd_found,
            running,
            boot_complete,
            acceleration,
        };
    }

    if running {
        return EngineState {
            state: "starting".into(),
            message: "Emulador conectado via ADB; o Android ainda está concluindo o boot.".into(),
            runtime_found,
            adb_found,
            avd_found,
            running,
            boot_complete,
            acceleration,
        };
    }

    if !runtime_found || !adb_found || !avd_found {
        return EngineState {
            state: "runtime_missing".into(),
            message: "Runtime incompleto. Use Instalar Runtime para baixar o Android Emulator/QEMU oficial e criar o AVD NOVA.".into(),
            runtime_found,
            adb_found,
            avd_found,
            running,
            boot_complete,
            acceleration,
        };
    }

    if !acceleration_ok {
        return EngineState {
            state: "acceleration_missing".into(),
            message: "Runtime instalado, mas a aceleração de hardware não está pronta. Ative Windows Hypervisor Platform e reinicie o PC.".into(),
            runtime_found,
            adb_found,
            avd_found,
            running,
            boot_complete,
            acceleration,
        };
    }

    EngineState {
        state: "ready".into(),
        message: "Runtime Android x86_64 pronto para iniciar com aceleração de hardware.".into(),
        runtime_found,
        adb_found,
        avd_found,
        running,
        boot_complete,
        acceleration,
    }
}

#[tauri::command]
fn engine_status() -> EngineState {
    collect_state()
}

#[tauri::command]
fn install_runtime() -> EngineState {
    let root = project_root();
    let script = root.join("engine").join("scripts").join("install-runtime.ps1");

    let spawn_result = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script)
        .spawn();

    match spawn_result {
        Ok(_) => EngineState {
            state: "installing".into(),
            message: "Instalador aberto. Ele baixará o runtime oficial e pedirá sua aceitação das licenças do Android SDK.".into(),
            runtime_found: emulator_path(&root).exists(),
            adb_found: adb_path(&root).exists(),
            avd_found: avd_exists(&root),
            running: false,
            boot_complete: false,
            acceleration: acceleration_status(&root),
        },
        Err(error) => EngineState {
            state: "error".into(),
            message: format!("Falha ao abrir instalador: {error}"),
            runtime_found: false,
            adb_found: false,
            avd_found: false,
            running: false,
            boot_complete: false,
            acceleration: "indisponível".into(),
        },
    }
}

#[tauri::command]
fn start_engine(profile: String) -> EngineState {
    let root = project_root();
    let current = collect_state();
    if current.state != "ready" && current.state != "running" && current.state != "starting" {
        return current;
    }

    if current.boot_complete {
        return current;
    }

    let script = root.join("engine").join("scripts").join("start-engine.ps1");
    match Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script)
        .arg("-Profile")
        .arg(profile)
        .spawn()
    {
        Ok(_) => EngineState {
            state: "starting".into(),
            message: "Android está iniciando. O NOVA vai acompanhar o ADB até o boot terminar.".into(),
            runtime_found: true,
            adb_found: true,
            avd_found: true,
            running: false,
            boot_complete: false,
            acceleration: acceleration_status(&root),
        },
        Err(error) => EngineState {
            state: "error".into(),
            message: format!("Falha ao iniciar engine: {error}"),
            runtime_found: true,
            adb_found: true,
            avd_found: true,
            running: false,
            boot_complete: false,
            acceleration: acceleration_status(&root),
        },
    }
}

#[tauri::command]
fn stop_engine() -> EngineState {
    let root = project_root();
    let adb = adb_path(&root);
    if adb.exists() {
        let _ = Command::new(adb)
            .args(["-s", "emulator-5554", "emu", "kill"])
            .output();
    }
    collect_state()
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            engine_status,
            install_runtime,
            start_engine,
            stop_engine
        ])
        .run(tauri::generate_context!())
        .expect("error while running NOVA Emulator");
}
