use serde::Serialize;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
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

#[derive(Serialize)]
struct InstalledApp {
    package: String,
}

static ACCELERATION_CACHE: OnceLock<(bool, String)> = OnceLock::new();

fn hidden_command<S: AsRef<OsStr>>(program: S) -> Command {
    let mut command = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

fn dev_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn runtime_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir())
        .join("NOVA")
        .join("Runtime")
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

fn acceleration_probe(runtime: &Path) -> (bool, String) {
    let emulator = emulator_path(runtime);
    if !emulator.exists() {
        return (false, "runtime não instalado".into());
    }

    ACCELERATION_CACHE
        .get_or_init(|| {
            let mut command = hidden_command(&emulator);
            match command.arg("-accel-check").output() {
                Ok(output) => {
                    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
                    text.push_str(&String::from_utf8_lossy(&output.stderr));
                    let normalized = text.trim().replace('\r', "").replace('\n', " · ");
                    let ok = output.status.success();
                    let message = if normalized.is_empty() {
                        if ok {
                            "aceleração disponível · accel-check OK".into()
                        } else {
                            format!("accel-check inconclusivo · exit {} · o NOVA testará no início real", output.status.code().unwrap_or(-1))
                        }
                    } else if ok {
                        format!("OK · {normalized}")
                    } else {
                        format!("Diagnóstico inconclusivo · {normalized}")
                    };
                    (ok, message)
                }
                Err(error) => (false, format!("accel-check indisponível: {error} · o NOVA testará no início real")),
            }
        })
        .clone()
}

fn acceleration_status(runtime: &Path) -> String {
    acceleration_probe(runtime).1
}

fn adb_output(runtime: &Path, args: &[&str]) -> Option<String> {
    let adb = adb_path(runtime);
    if !adb.exists() {
        return None;
    }

    let mut command = hidden_command(&adb);
    command
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn adb_result(runtime: &Path, args: &[&str]) -> Result<String, String> {
    let adb = adb_path(runtime);
    if !adb.exists() {
        return Err("ADB não encontrado. Execute o preparador do NOVA novamente.".into());
    }

    let mut command = hidden_command(&adb);
    let output = command
        .args(args)
        .output()
        .map_err(|error| format!("Falha ao executar ADB: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        if stdout.is_empty() { Ok(stderr) } else { Ok(stdout) }
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
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

fn collect_state() -> EngineState {
    let runtime = runtime_root();
    let runtime_found = emulator_path(&runtime).exists();
    let adb_found = adb_path(&runtime).exists();
    let avd_found = avd_exists(&runtime);
    let running = android_running(&runtime);
    let boot_complete = boot_complete(&runtime);
    let (acceleration_ok, acceleration) = acceleration_probe(&runtime);

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
            message: "Runtime Android ainda não está pronto. Execute o preparador do NOVA para concluir o ambiente.".into(),
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
        message: if acceleration_ok {
            "Runtime Android x86_64 pronto para iniciar com aceleração de hardware.".into()
        } else {
            "Runtime pronto. O accel-check foi inconclusivo, então o NOVA permitirá iniciar e usará o resultado real do Android Emulator.".into()
        },
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
fn install_runtime(app: tauri::AppHandle) -> EngineState {
    let runtime = runtime_root();
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

    let mut command = hidden_command("powershell.exe");
    let spawn_result = command
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script)
        .arg("-RuntimeRoot")
        .arg(&runtime)
        .spawn();

    match spawn_result {
        Ok(_) => EngineState {
            state: "installing".into(),
            message: "Preparando o runtime Android. Na primeira instalação, conclua as licenças oficiais exibidas no terminal.".into(),
            runtime_found: emulator_path(&runtime).exists(),
            adb_found: adb_path(&runtime).exists(),
            avd_found: avd_exists(&runtime),
            running: false,
            boot_complete: false,
            acceleration: acceleration_status(&runtime),
        },
        Err(error) => EngineState {
            state: "error".into(),
            message: format!("Falha ao abrir preparador do runtime: {error}"),
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
    let runtime = runtime_root();
    let current = collect_state();
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

    let mut command = hidden_command("powershell.exe");
    match command
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
fn stop_engine() -> EngineState {
    let runtime = runtime_root();
    let adb = adb_path(&runtime);
    if adb.exists() {
        let mut command = hidden_command(&adb);
        let _ = command
            .args(["-s", "emulator-5554", "emu", "kill"])
            .output();
    }
    collect_state()
}

#[tauri::command]
fn list_apps() -> Result<Vec<InstalledApp>, String> {
    let runtime = runtime_root();
    if !boot_complete(&runtime) {
        return Err("Inicie o Android antes de carregar a lista de apps.".into());
    }

    let output = adb_result(
        &runtime,
        &["-s", "emulator-5554", "shell", "pm", "list", "packages", "-3"],
    )?;

    let mut apps: Vec<InstalledApp> = output
        .lines()
        .filter_map(|line| line.trim().strip_prefix("package:"))
        .filter(|package| !package.is_empty())
        .map(|package| InstalledApp { package: package.to_string() })
        .collect();
    apps.sort_by(|a, b| a.package.cmp(&b.package));
    Ok(apps)
}

#[tauri::command]
fn install_apk() -> Result<String, String> {
    let runtime = runtime_root();
    if !boot_complete(&runtime) {
        return Err("Inicie o Android e aguarde o boot terminar antes de instalar um APK.".into());
    }

    let file = rfd::FileDialog::new()
        .add_filter("Android APK", &["apk"])
        .set_title("Escolha um APK para instalar no NOVA")
        .pick_file()
        .ok_or_else(|| "Seleção de APK cancelada.".to_string())?;

    let adb = adb_path(&runtime);
    let mut command = hidden_command(&adb);
    let output = command
        .args(["-s", "emulator-5554", "install", "-r"])
        .arg(&file)
        .output()
        .map_err(|error| format!("Falha ao executar instalação: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() && stdout.to_ascii_lowercase().contains("success") {
        Ok(format!("APK instalado com sucesso: {}", file.file_name().and_then(|n| n.to_str()).unwrap_or("arquivo.apk")))
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

#[tauri::command]
fn launch_app(package: String) -> Result<String, String> {
    let runtime = runtime_root();
    if !boot_complete(&runtime) {
        return Err("Inicie o Android antes de abrir um app.".into());
    }
    if package.trim().is_empty() {
        return Err("Pacote inválido.".into());
    }

    adb_result(
        &runtime,
        &[
            "-s",
            "emulator-5554",
            "shell",
            "monkey",
            "-p",
            package.trim(),
            "-c",
            "android.intent.category.LAUNCHER",
            "1",
        ],
    )?;
    Ok(format!("Abrindo {}", package.trim()))
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            engine_status,
            install_runtime,
            start_engine,
            stop_engine,
            list_apps,
            install_apk,
            launch_app
        ])
        .run(tauri::generate_context!())
        .expect("error while running NOVA Emulator");
}
