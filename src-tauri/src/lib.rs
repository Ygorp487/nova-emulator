use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use tauri::Manager;

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

fn dev_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn runtime_root(app: &tauri::AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        return dev_root().join("engine").join("runtime");
    }

    app.path()
        .app_local_data_dir()
        .unwrap_or_else(|_| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| PathBuf::from("."))
        })
        .join("engine")
        .join("runtime")
}

fn script_path(app: &tauri::AppHandle, name: &str) -> PathBuf {
    if cfg!(debug_assertions) {
        return dev_root().join("engine").join("scripts").join(name);
    }

    app.path()
        .resource_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("engine")
        .join("scripts")
        .join(name)
}

fn sdk_root(runtime: &Path) -> PathBuf {
    runtime.join("sdk")
}

fn emulator_path(runtime: &Path) -> PathBuf {
    sdk_root(runtime).join("emulator").join("emulator.exe")
}

fn adb_path(runtime: &Path) -> PathBuf {
    sdk_root(runtime).join("platform-tools").join("adb.exe")
}

fn avd_home(runtime: &Path) -> PathBuf {
    runtime.join("avd")
}

fn avd_exists(runtime: &Path) -> bool {
    avd_home(runtime).join("NOVA.avd").join("config.ini").exists()
}

fn acceleration_status(runtime: &Path) -> String {
    let emulator = emulator_path(runtime);
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

fn adb_output(runtime: &Path, args: &[&str]) -> Option<String> {
    let adb = adb_path(runtime);
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

fn android_running(runtime: &Path) -> bool {
    adb_output(runtime, &["-s", "emulator-5554", "get-state"])
        .map(|state| state.eq_ignore_ascii_case("device"))
        .unwrap_or(false)
}

fn boot_complete(runtime: &Path) -> bool {
    if !android_running(runtime) {
        return false;
    }

    adb_output(
        runtime,
        &["-s", "emulator-5554", "shell", "getprop", "sys.boot_completed"],
    )
    .map(|value| value == "1")
    .unwrap_or(false)
}

fn android_version(runtime: &Path) -> Option<String> {
    adb_output(
        runtime,
        &["-s", "emulator-5554", "shell", "getprop", "ro.build.version.release"],
    )
}

fn collect_state(app: &tauri::AppHandle) -> EngineState {
    let runtime = runtime_root(app);
    let runtime_found = emulator_path(&runtime).exists();
    let adb_found = adb_path(&runtime).exists();
    let avd_found = avd_exists(&runtime);
    let running = android_running(&runtime);
    let boot_complete = boot_complete(&runtime);
    let acceleration = acceleration_status(&runtime);
    let acceleration_ok = acceleration.to_ascii_lowercase().contains("usable");

    if running && boot_complete {
        let version = android_version(&runtime).unwrap_or_else(|| "Android".into());
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
fn engine_status(app: tauri::AppHandle) -> EngineState {
    collect_state(&app)
}

#[tauri::command]
fn install_runtime(app: tauri::AppHandle) -> EngineState {
    let runtime = runtime_root(&app);
    let script = script_path(&app, "install-runtime.ps1");

    if !script.exists() {
        return EngineState {
            state: "error".into(),
            message: format!("Script do runtime não encontrado: {}", script.display()),
            runtime_found: false,
            adb_found: false,
            avd_found: false,
            running: false,
            boot_complete: false,
            acceleration: "indisponível".into(),
        };
    }

    let spawn_result = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script)
        .arg("-RuntimeRoot")
        .arg(&runtime)
        .spawn();

    match spawn_result {
        Ok(_) => EngineState {
            state: "installing".into(),
            message: "Instalador aberto. Ele baixará o runtime oficial e pedirá sua aceitação das licenças do Android SDK.".into(),
            runtime_found: emulator_path(&runtime).exists(),
            adb_found: adb_path(&runtime).exists(),
            avd_found: avd_exists(&runtime),
            running: false,
            boot_complete: false,
            acceleration: acceleration_status(&runtime),
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
fn start_engine(app: tauri::AppHandle, profile: String) -> EngineState {
    let runtime = runtime_root(&app);
    let current = collect_state(&app);
    if current.state != "ready" && current.state != "running" && current.state != "starting" {
        return current;
    }

    if current.boot_complete {
        return current;
    }

    let script = script_path(&app, "start-engine.ps1");
    if !script.exists() {
        return EngineState {
            state: "error".into(),
            message: format!("Script do engine não encontrado: {}", script.display()),
            runtime_found: true,
            adb_found: true,
            avd_found: true,
            running: false,
            boot_complete: false,
            acceleration: acceleration_status(&runtime),
        };
    }

    match Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script)
        .arg("-Profile")
        .arg(profile)
        .arg("-RuntimeRoot")
        .arg(&runtime)
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
            acceleration: acceleration_status(&runtime),
        },
        Err(error) => EngineState {
            state: "error".into(),
            message: format!("Falha ao iniciar engine: {error}"),
            runtime_found: true,
            adb_found: true,
            avd_found: true,
            running: false,
            boot_complete: false,
            acceleration: acceleration_status(&runtime),
        },
    }
}

#[tauri::command]
fn stop_engine(app: tauri::AppHandle) -> EngineState {
    let runtime = runtime_root(&app);
    let adb = adb_path(&runtime);
    if adb.exists() {
        let _ = Command::new(adb)
            .args(["-s", "emulator-5554", "emu", "kill"])
            .output();
    }
    collect_state(&app)
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
