use serde::Serialize;
use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::Write,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
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

#[derive(Clone, Default)]
struct LaunchStatus {
    in_progress: bool,
    cancel_requested: bool,
    last_error: Option<String>,
    stage: String,
    serial: Option<String>,
    port: Option<u16>,
    pid: Option<u32>,
}

#[derive(Clone)]
struct ProfileSettings {
    cores: u8,
    ram_mb: u32,
    gpu_modes: Vec<&'static str>,
}

static ACCELERATION_CACHE: OnceLock<(bool, String)> = OnceLock::new();
static LAUNCH_STATUS: OnceLock<Mutex<LaunchStatus>> = OnceLock::new();

fn launch_status() -> &'static Mutex<LaunchStatus> {
    LAUNCH_STATUS.get_or_init(|| Mutex::new(LaunchStatus::default()))
}

fn update_launch<F>(f: F)
where
    F: FnOnce(&mut LaunchStatus),
{
    if let Ok(mut status) = launch_status().lock() {
        f(&mut status);
    }
}

fn launch_snapshot() -> LaunchStatus {
    launch_status()
        .lock()
        .map(|status| status.clone())
        .unwrap_or_default()
}

fn set_stage(stage: &str) {
    update_launch(|status| {
        status.stage = stage.to_string();
        status.in_progress = true;
        status.last_error = None;
    });
}

fn mark_launch_error(error: String) {
    update_launch(|status| {
        status.in_progress = false;
        status.cancel_requested = false;
        status.stage = "error".into();
        status.last_error = Some(error);
        status.pid = None;
    });
}

fn mark_launch_ready() {
    update_launch(|status| {
        status.in_progress = false;
        status.cancel_requested = false;
        status.stage = "running".into();
        status.last_error = None;
        status.pid = None;
    });
}

fn launch_cancelled() -> bool {
    launch_snapshot().cancel_requested
}

fn compact_output(text: String) -> String {
    let cleaned = text
        .replace('\r', "")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");

    if cleaned.chars().count() > 2200 {
        format!("{}…", cleaned.chars().take(2200).collect::<String>())
    } else {
        cleaned
    }
}

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

fn avd_dir(runtime: &Path) -> PathBuf {
    avd_home(runtime).join("NOVA.avd")
}

fn avd_exists(runtime: &Path) -> bool {
    avd_dir(runtime).join("config.ini").exists()
}

fn log_root(runtime: &Path) -> PathBuf {
    runtime.join("logs")
}

fn append_engine_log(runtime: &Path, text: &str) {
    let root = log_root(runtime);
    let _ = fs::create_dir_all(&root);
    let path = root.join("engine-last-start.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{text}");
    }
}

fn set_ini_value(path: &Path, key: &str, value: &str) -> Result<(), String> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let prefix = format!("{key}=");
    let mut found = false;
    let mut lines = Vec::new();

    for line in existing.lines() {
        if line.starts_with(&prefix) {
            lines.push(format!("{key}={value}"));
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !found {
        lines.push(format!("{key}={value}"));
    }

    fs::write(path, format!("{}\n", lines.join("\n")))
        .map_err(|error| format!("Falha ao atualizar {}: {error}", path.display()))
}

fn repair_avd(runtime: &Path) -> Result<(), String> {
    let home = avd_home(runtime);
    let dir = avd_dir(runtime);
    let config = dir.join("config.ini");
    let ini = home.join("NOVA.ini");

    if !config.exists() {
        return Err(format!("config.ini do AVD não encontrado: {}", config.display()));
    }

    fs::create_dir_all(&home)
        .map_err(|error| format!("Falha ao preparar pasta do AVD: {error}"))?;

    let target = fs::read_to_string(&ini)
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("target=").map(str::trim))
                .map(str::to_string)
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "android-35".into());

    let descriptor = format!(
        "avd.ini.encoding=UTF-8\npath={}\npath.rel=NOVA.avd\ntarget={}\n",
        dir.display(),
        target
    );
    fs::write(&ini, descriptor)
        .map_err(|error| format!("Falha ao reparar NOVA.ini: {error}"))?;

    // Command-line flags control boot and graphics. Keep the AVD itself neutral.
    set_ini_value(&config, "hw.gpu.enabled", "yes")?;
    set_ini_value(&config, "hw.gpu.mode", "auto")?;
    set_ini_value(&config, "fastboot.forceColdBoot", "no")?;
    set_ini_value(&config, "fastboot.forceFastBoot", "no")?;
    Ok(())
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
                    let normalized = compact_output(text);
                    let ok = output.status.success();
                    if ok {
                        (
                            true,
                            if normalized.is_empty() {
                                "hipervisor disponível".into()
                            } else {
                                format!("OK · {normalized}")
                            },
                        )
                    } else {
                        (
                            false,
                            format!(
                                "não confirmado pelo accel-check (código {}) · o boot real fará o teste",
                                output.status.code().unwrap_or(-1)
                            ),
                        )
                    }
                }
                Err(error) => (
                    false,
                    format!("accel-check indisponível: {error} · o boot real fará o teste"),
                ),
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
        if stdout.is_empty() {
            Ok(stderr)
        } else {
            Ok(stdout)
        }
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

fn serial_online(runtime: &Path, serial: &str) -> bool {
    adb_output(runtime, &["devices"])
        .map(|text| {
            text.lines().any(|line| {
                let mut parts = line.split_whitespace();
                parts.next() == Some(serial) && parts.next() == Some("device")
            })
        })
        .unwrap_or(false)
}

fn nova_serial(runtime: &Path) -> Option<String> {
    let snapshot = launch_snapshot();
    if let Some(serial) = snapshot.serial {
        if serial_online(runtime, &serial) {
            return Some(serial);
        }
    }

    let devices = adb_output(runtime, &["devices"])?;
    let mut candidates = Vec::new();
    for line in devices.lines() {
        let mut parts = line.split_whitespace();
        let Some(serial) = parts.next() else { continue };
        let Some(state) = parts.next() else { continue };
        if serial.starts_with("emulator-") && state == "device" {
            candidates.push(serial.to_string());
        }
    }

    for serial in &candidates {
        if let Ok(name) = adb_result(runtime, &["-s", serial, "emu", "avd", "name"]) {
            if name.lines().any(|line| line.trim() == "NOVA") {
                return Some(serial.clone());
            }
        }
    }

    if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        None
    }
}

fn boot_complete_for(runtime: &Path, serial: &str) -> bool {
    adb_output(
        runtime,
        &["-s", serial, "shell", "getprop", "sys.boot_completed"],
    )
    .map(|value| value.trim() == "1")
    .unwrap_or(false)
}

fn android_running(runtime: &Path) -> bool {
    nova_serial(runtime).is_some()
}

fn boot_complete(runtime: &Path) -> bool {
    nova_serial(runtime)
        .map(|serial| boot_complete_for(runtime, &serial))
        .unwrap_or(false)
}

fn android_version(runtime: &Path, serial: &str) -> Option<String> {
    adb_output(
        runtime,
        &["-s", serial, "shell", "getprop", "ro.build.version.release"],
    )
}

fn profile_settings(profile: &str) -> ProfileSettings {
    match profile {
        "eco" => ProfileSettings {
            cores: 2,
            ram_mb: 2048,
            gpu_modes: vec!["auto", "software"],
        },
        "performance" => ProfileSettings {
            cores: 4,
            ram_mb: 6144,
            gpu_modes: vec!["host", "auto", "software"],
        },
        _ => ProfileSettings {
            cores: 3,
            ram_mb: 4096,
            gpu_modes: vec!["auto", "software"],
        },
    }
}

fn port_pair_is_free(port: u16) -> bool {
    if port >= 65534 {
        return false;
    }
    let first = TcpListener::bind(("127.0.0.1", port));
    if first.is_err() {
        return false;
    }
    let second = TcpListener::bind(("127.0.0.1", port + 1));
    second.is_ok()
}

fn find_free_emulator_port() -> Result<u16, String> {
    (5554u16..=5682u16)
        .step_by(2)
        .find(|port| port_pair_is_free(*port))
        .ok_or_else(|| "Nenhum par de portas livre entre 5554 e 5683 para o Android Emulator.".into())
}

fn kill_process_tree(pid: u32) {
    let mut command = hidden_command("taskkill.exe");
    let _ = command
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
}

fn read_attempt_logs(runtime: &Path, tag: &str) -> String {
    let root = log_root(runtime);
    let stdout_path = root.join(format!("emulator-{tag}-stdout.log"));
    let stderr_path = root.join(format!("emulator-{tag}-stderr.log"));
    let stderr = fs::read_to_string(stderr_path).unwrap_or_default();
    let stdout = fs::read_to_string(stdout_path).unwrap_or_default();
    compact_output(format!("{stderr}\n{stdout}"))
}

fn launch_emulator_attempt(
    runtime: &Path,
    profile: &ProfileSettings,
    gpu_mode: &str,
    cold_boot: bool,
    attempt: usize,
) -> Result<String, String> {
    if launch_cancelled() {
        return Err("__cancelled__".into());
    }

    let port = find_free_emulator_port()?;
    let serial = format!("emulator-{port}");
    let emulator = emulator_path(runtime);
    let root = log_root(runtime);
    fs::create_dir_all(&root).map_err(|error| format!("Falha ao criar pasta de logs: {error}"))?;

    let tag = format!("{}-{}", attempt + 1, gpu_mode.replace('_', "-"));
    let stdout_path = root.join(format!("emulator-{tag}-stdout.log"));
    let stderr_path = root.join(format!("emulator-{tag}-stderr.log"));
    let stdout = File::create(&stdout_path)
        .map_err(|error| format!("Falha ao criar log stdout: {error}"))?;
    let stderr = File::create(&stderr_path)
        .map_err(|error| format!("Falha ao criar log stderr: {error}"))?;

    update_launch(|status| {
        status.serial = Some(serial.clone());
        status.port = Some(port);
        status.stage = if attempt == 0 {
            format!("launching:{gpu_mode}")
        } else {
            format!("fallback:{gpu_mode}")
        };
    });

    append_engine_log(
        runtime,
        &format!(
            "attempt={} gpu={} port={} cores={} ram={} cold_boot={}",
            attempt + 1,
            gpu_mode,
            port,
            profile.cores,
            profile.ram_mb,
            cold_boot
        ),
    );

    let mut command = Command::new(&emulator);
    command
        .current_dir(runtime)
        .env("ANDROID_SDK_ROOT", sdk_root(runtime))
        .env("ANDROID_HOME", sdk_root(runtime))
        .env("ANDROID_AVD_HOME", avd_home(runtime))
        .arg("-avd")
        .arg("NOVA")
        .arg("-port")
        .arg(port.to_string())
        .arg("-accel")
        .arg("on")
        .arg("-gpu")
        .arg(gpu_mode)
        .arg("-cores")
        .arg(profile.cores.to_string())
        .arg("-memory")
        .arg(profile.ram_mb.to_string())
        .arg("-no-metrics")
        .arg("-no-boot-anim")
        .arg("-no-audio")
        .arg("-camera-back")
        .arg("none")
        .arg("-camera-front")
        .arg("none")
        .arg("-netfast")
        .arg("-verbose")
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));

    if cold_boot {
        command.arg("-no-snapshot-load");
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("Não foi possível abrir emulator.exe: {error}"))?;
    let pid = child.id();
    let _ = fs::write(runtime.join("engine.pid"), pid.to_string());
    update_launch(|status| {
        status.pid = Some(pid);
        status.stage = format!("waiting-adb:{gpu_mode}");
    });

    let started = Instant::now();
    let adb_deadline = started + Duration::from_secs(60);
    let mut child_exited_at: Option<Instant> = None;

    while Instant::now() < adb_deadline {
        if launch_cancelled() {
            kill_process_tree(pid);
            return Err("__cancelled__".into());
        }

        if serial_online(runtime, &serial) {
            update_launch(|status| status.stage = format!("booting:{gpu_mode}"));
            append_engine_log(runtime, &format!("adb_online serial={serial}"));
            break;
        }

        if let Ok(Some(exit)) = child.try_wait() {
            if child_exited_at.is_none() {
                child_exited_at = Some(Instant::now());
                append_engine_log(
                    runtime,
                    &format!("emulator_launcher_exited status={exit}"),
                );
            }
            // emulator.exe can hand off to QEMU. Allow a short window for ADB to appear.
            if child_exited_at
                .map(|time| time.elapsed() >= Duration::from_secs(15))
                .unwrap_or(false)
            {
                break;
            }
        }

        thread::sleep(Duration::from_millis(500));
    }

    if !serial_online(runtime, &serial) {
        kill_process_tree(pid);
        thread::sleep(Duration::from_millis(800));
        let details = read_attempt_logs(runtime, &tag);
        return Err(if details.is_empty() {
            format!("GPU {gpu_mode}: o Emulator não apareceu no ADB em 60 segundos.")
        } else {
            format!("GPU {gpu_mode}: {details}")
        });
    }

    let boot_deadline = Instant::now() + Duration::from_secs(180);
    while Instant::now() < boot_deadline {
        if launch_cancelled() {
            let _ = adb_result(runtime, &["-s", &serial, "emu", "kill"]);
            kill_process_tree(pid);
            return Err("__cancelled__".into());
        }

        if boot_complete_for(runtime, &serial) {
            let _ = fs::write(runtime.join("boot-ok.marker"), "1");
            let _ = fs::remove_file(runtime.join("engine.pid"));
            update_launch(|status| {
                status.serial = Some(serial.clone());
                status.port = Some(port);
                status.pid = None;
            });
            return Ok(serial);
        }

        if !serial_online(runtime, &serial) {
            // Temporary offline states are normal during early boot; only abort after a grace period.
            thread::sleep(Duration::from_secs(2));
        } else {
            thread::sleep(Duration::from_secs(1));
        }
    }

    let _ = adb_result(runtime, &["-s", &serial, "emu", "kill"]);
    kill_process_tree(pid);
    let details = read_attempt_logs(runtime, &tag);
    Err(format!(
        "GPU {gpu_mode}: ADB conectou, mas o Android não concluiu o boot em 3 minutos. {details}"
    ))
}

fn run_launch_sequence(runtime: PathBuf, profile_name: String) -> Result<(), String> {
    set_stage("preflight");
    repair_avd(&runtime)?;

    let emulator = emulator_path(&runtime);
    if !emulator.exists() {
        return Err(format!("emulator.exe não encontrado: {}", emulator.display()));
    }

    // Keep the full acceleration output in the log, but do not use it as the only truth source.
    let mut accel_command = hidden_command(&emulator);
    let accel = accel_command.arg("-accel-check").output();
    if let Ok(output) = accel {
        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        append_engine_log(
            &runtime,
            &format!(
                "accel-check exit={} {}",
                output.status.code().unwrap_or(-1),
                compact_output(text)
            ),
        );
    }

    let mut settings = profile_settings(&profile_name);
    let first_successful_boot = !runtime.join("boot-ok.marker").exists();

    // First successful boot prioritizes compatibility over speed. Once it works, normal profile settings take over.
    if first_successful_boot {
        settings.cores = settings.cores.min(2);
        settings.ram_mb = settings.ram_mb.min(3072);
        settings.gpu_modes = vec!["software", "auto"];
    }

    let mut failures = Vec::new();
    for (attempt, gpu_mode) in settings.gpu_modes.clone().into_iter().enumerate() {
        if launch_cancelled() {
            return Err("__cancelled__".into());
        }

        match launch_emulator_attempt(
            &runtime,
            &settings,
            gpu_mode,
            first_successful_boot || attempt > 0,
            attempt,
        ) {
            Ok(serial) => {
                append_engine_log(&runtime, &format!("boot_success serial={serial} gpu={gpu_mode}"));
                mark_launch_ready();
                return Ok(());
            }
            Err(error) if error == "__cancelled__" => return Err(error),
            Err(error) => {
                append_engine_log(&runtime, &format!("attempt_failed {error}"));
                failures.push(error);
                thread::sleep(Duration::from_secs(2));
            }
        }
    }

    Err(format!(
        "Todas as tentativas de boot falharam. {}",
        failures.join(" | ")
    ))
}

fn stage_message(stage: &str) -> String {
    if stage == "preflight" {
        return "Verificando AVD, portas, aceleração e configuração do runtime...".into();
    }
    if let Some(gpu) = stage.strip_prefix("launching:") {
        return format!("Abrindo Android Emulator com renderização {gpu}...");
    }
    if let Some(gpu) = stage.strip_prefix("waiting-adb:") {
        return format!("Emulator aberto com GPU {gpu}; aguardando o dispositivo aparecer no ADB...");
    }
    if let Some(gpu) = stage.strip_prefix("fallback:") {
        return format!("A primeira renderização não respondeu; tentando fallback de GPU {gpu}...");
    }
    if let Some(gpu) = stage.strip_prefix("booting:") {
        return format!("ADB conectado. Android está concluindo o boot com GPU {gpu}...");
    }
    "Iniciando Android Emulator...".into()
}

fn collect_state() -> EngineState {
    let runtime = runtime_root();
    let runtime_found = emulator_path(&runtime).exists();
    let adb_found = adb_path(&runtime).exists();
    let avd_found = avd_exists(&runtime);
    let serial = nova_serial(&runtime);
    let running = serial.is_some();
    let boot_complete = serial
        .as_deref()
        .map(|value| boot_complete_for(&runtime, value))
        .unwrap_or(false);
    let (acceleration_ok, acceleration) = acceleration_probe(&runtime);

    if running && boot_complete {
        mark_launch_ready();
        let version = serial
            .as_deref()
            .and_then(|value| android_version(&runtime, value))
            .unwrap_or_else(|| "Android".into());
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
            message: "ADB conectado; o Android ainda está concluindo o boot.".into(),
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

    let launch = launch_snapshot();
    if launch.in_progress {
        return EngineState {
            state: "starting".into(),
            message: stage_message(&launch.stage),
            runtime_found,
            adb_found,
            avd_found,
            running,
            boot_complete,
            acceleration,
        };
    }

    if let Some(error) = launch.last_error {
        return EngineState {
            state: "error".into(),
            message: format!("Falha ao iniciar Android: {error}"),
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
            "Runtime Android pronto. O NOVA fará o boot diretamente pelo engine Rust.".into()
        } else {
            "Runtime pronto. O accel-check não confirmou o hipervisor; o boot real será usado como teste definitivo.".into()
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
fn start_engine(profile: String) -> EngineState {
    let runtime = runtime_root();
    let current = collect_state();

    if !current.runtime_found || !current.adb_found || !current.avd_found {
        return current;
    }
    if current.boot_complete || current.running || launch_snapshot().in_progress {
        return current;
    }

    update_launch(|status| {
        *status = LaunchStatus {
            in_progress: true,
            cancel_requested: false,
            last_error: None,
            stage: "preflight".into(),
            serial: None,
            port: None,
            pid: None,
        };
    });

    let thread_runtime = runtime.clone();
    let thread_profile = profile.to_lowercase();
    thread::spawn(move || match run_launch_sequence(thread_runtime, thread_profile) {
        Ok(()) => {}
        Err(error) if error == "__cancelled__" => {
            update_launch(|status| *status = LaunchStatus::default());
        }
        Err(error) => mark_launch_error(error),
    });

    EngineState {
        state: "starting".into(),
        message: "Verificando AVD, portas e modo de renderização antes de abrir o Android...".into(),
        runtime_found: true,
        adb_found: true,
        avd_found: true,
        running: false,
        boot_complete: false,
        acceleration: acceleration_status(&runtime),
    }
}

#[tauri::command]
fn stop_engine() -> EngineState {
    let runtime = runtime_root();
    let snapshot = launch_snapshot();
    update_launch(|status| status.cancel_requested = true);

    if let Some(serial) = nova_serial(&runtime).or(snapshot.serial.clone()) {
        let _ = adb_result(&runtime, &["-s", &serial, "emu", "kill"]);
    }
    if let Some(pid) = snapshot.pid {
        kill_process_tree(pid);
    }
    if let Ok(pid_text) = fs::read_to_string(runtime.join("engine.pid")) {
        if let Ok(pid) = pid_text.trim().parse::<u32>() {
            kill_process_tree(pid);
        }
    }
    let _ = fs::remove_file(runtime.join("engine.pid"));
    thread::sleep(Duration::from_millis(400));
    update_launch(|status| *status = LaunchStatus::default());
    collect_state()
}

#[tauri::command]
fn list_apps() -> Result<Vec<InstalledApp>, String> {
    let runtime = runtime_root();
    let serial = nova_serial(&runtime)
        .ok_or_else(|| "Inicie o Android antes de carregar a lista de apps.".to_string())?;
    if !boot_complete_for(&runtime, &serial) {
        return Err("Aguarde o Android concluir o boot antes de carregar os apps.".into());
    }

    let output = adb_result(
        &runtime,
        &["-s", &serial, "shell", "pm", "list", "packages", "-3"],
    )?;

    let mut apps: Vec<InstalledApp> = output
        .lines()
        .filter_map(|line| line.trim().strip_prefix("package:"))
        .filter(|package| !package.is_empty())
        .map(|package| InstalledApp {
            package: package.to_string(),
        })
        .collect();
    apps.sort_by(|a, b| a.package.cmp(&b.package));
    Ok(apps)
}

#[tauri::command]
fn install_apk() -> Result<String, String> {
    let runtime = runtime_root();
    let serial = nova_serial(&runtime)
        .ok_or_else(|| "Inicie o Android antes de instalar um APK.".to_string())?;
    if !boot_complete_for(&runtime, &serial) {
        return Err("Aguarde o Android concluir o boot antes de instalar um APK.".into());
    }

    let file = rfd::FileDialog::new()
        .add_filter("Android APK", &["apk"])
        .set_title("Escolha um APK para instalar no NOVA")
        .pick_file()
        .ok_or_else(|| "Seleção de APK cancelada.".to_string())?;

    let adb = adb_path(&runtime);
    let mut command = hidden_command(&adb);
    let output = command
        .args(["-s", &serial, "install", "-r"])
        .arg(&file)
        .output()
        .map_err(|error| format!("Falha ao executar instalação: {error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() && stdout.to_ascii_lowercase().contains("success") {
        Ok(format!(
            "APK instalado com sucesso: {}",
            file.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("arquivo.apk")
        ))
    } else {
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

#[tauri::command]
fn launch_app(package: String) -> Result<String, String> {
    let runtime = runtime_root();
    let serial = nova_serial(&runtime)
        .ok_or_else(|| "Inicie o Android antes de abrir um app.".to_string())?;
    if !boot_complete_for(&runtime, &serial) {
        return Err("Aguarde o Android concluir o boot antes de abrir um app.".into());
    }
    if package.trim().is_empty() {
        return Err("Pacote inválido.".into());
    }

    adb_result(
        &runtime,
        &[
            "-s",
            &serial,
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
