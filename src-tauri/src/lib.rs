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

#[derive(Clone)]
struct AttemptPlan {
    gpu: &'static str,
    cold_boot: bool,
    wipe_data: bool,
    disable_vulkan: bool,
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

fn clean_lines(text: &str) -> Vec<String> {
    text.replace('\r', "")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn limit_tail(text: String, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text;
    }
    let start = chars.len().saturating_sub(max_chars);
    format!("…{}", chars[start..].iter().collect::<String>())
}

fn compact_output(text: String) -> String {
    let joined = clean_lines(&text).join(" · ");
    limit_tail(joined, 2200)
}

fn diagnostic_tail(text: &str) -> String {
    let lines = clean_lines(text);
    if lines.is_empty() {
        return String::new();
    }

    let keywords = [
        "error", "fatal", "panic", "failed", "failure", "cannot", "could not",
        "unable", "whpx", "hypervisor", "vulkan", "opengl", "egl", "qemu",
        "crash", "exit", "memory", "ram", "permission", "denied", "unsupported",
    ];

    let significant: Vec<String> = lines
        .iter()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            keywords.iter().any(|word| lower.contains(word))
        })
        .cloned()
        .collect();

    let tail_start = lines.len().saturating_sub(45);
    let mut selected = lines[tail_start..].to_vec();
    if !significant.is_empty() {
        let sig_start = significant.len().saturating_sub(20);
        let mut merged = significant[sig_start..].to_vec();
        for line in selected.drain(..) {
            if !merged.iter().any(|existing| existing == &line) {
                merged.push(line);
            }
        }
        selected = merged;
    }

    limit_tail(selected.join(" · "), 3600)
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

fn engine_log_path(runtime: &Path) -> PathBuf {
    log_root(runtime).join("engine-last-start.log")
}

fn append_engine_log(runtime: &Path, text: &str) {
    let root = log_root(runtime);
    let _ = fs::create_dir_all(&root);
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(engine_log_path(runtime))
    {
        let _ = writeln!(file, "{text}");
    }
}

fn emulator_version(runtime: &Path) -> String {
    let source = sdk_root(runtime).join("emulator").join("source.properties");
    fs::read_to_string(source)
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                line.strip_prefix("Pkg.Revision=")
                    .or_else(|| line.strip_prefix("Pkg.Revision ="))
                    .map(str::trim)
                    .map(str::to_string)
            })
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "desconhecida".into())
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

    set_ini_value(&config, "hw.keyboard", "yes")?;
    set_ini_value(&config, "hw.gpu.enabled", "yes")?;
    set_ini_value(&config, "hw.gpu.mode", "auto")?;
    set_ini_value(&config, "hw.lcd.width", "720")?;
    set_ini_value(&config, "hw.lcd.height", "1280")?;
    set_ini_value(&config, "hw.lcd.density", "320")?;
    set_ini_value(&config, "disk.dataPartition.size", "12G")?;
    set_ini_value(&config, "fastboot.forceColdBoot", "no")?;
    set_ini_value(&config, "fastboot.forceFastBoot", "no")?;

    if !runtime.join("boot-ok.marker").exists() {
        let _ = fs::remove_dir_all(dir.join("snapshots"));
    }
    Ok(())
}

fn acceleration_probe(runtime: &Path) -> (bool, String) {
    let emulator = emulator_path(runtime);
    if !emulator.exists() {
        return (false, "runtime não instalado".into());
    }

    ACCELERATION_CACHE
        .get_or_init(|| {
            let version = emulator_version(runtime);
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
                                format!("Emulator {version} · hipervisor disponível")
                            } else {
                                format!("Emulator {version} · {normalized}")
                            },
                        )
                    } else {
                        (
                            false,
                            format!(
                                "Emulator {version} · accel-check código {} · o boot real fará o teste",
                                output.status.code().unwrap_or(-1)
                            ),
                        )
                    }
                }
                Err(error) => (
                    false,
                    format!("Emulator {version} · accel-check indisponível: {error}"),
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
            gpu_modes: vec!["auto", "host", "software"],
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
    TcpListener::bind(("127.0.0.1", port + 1)).is_ok()
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

fn kill_stale_runtime_emulators(runtime: &Path) {
    #[cfg(target_os = "windows")]
    {
        let runtime_text = runtime.display().to_string().replace("'", "''");
        let script = format!(
            "$root='{runtime_text}'; Get-CimInstance Win32_Process -ErrorAction SilentlyContinue | Where-Object {{ ($_.Name -eq 'emulator.exe' -or $_.Name -eq 'qemu-system-x86_64.exe') -and ((-not [string]::IsNullOrWhiteSpace($_.ExecutablePath) -and $_.ExecutablePath.StartsWith($root,[System.StringComparison]::OrdinalIgnoreCase)) -or (-not [string]::IsNullOrWhiteSpace($_.CommandLine) -and $_.CommandLine.IndexOf($root,[System.StringComparison]::OrdinalIgnoreCase) -ge 0)) }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}"
        );
        let mut command = hidden_command("powershell.exe");
        let _ = command
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
            .output();
    }
}

fn read_attempt_logs(runtime: &Path, tag: &str) -> String {
    let root = log_root(runtime);
    let stdout = fs::read_to_string(root.join(format!("emulator-{tag}-stdout.log"))).unwrap_or_default();
    let stderr = fs::read_to_string(root.join(format!("emulator-{tag}-stderr.log"))).unwrap_or_default();
    diagnostic_tail(&format!("{stdout}\n{stderr}"))
}

fn classify_failure(gpu: &str, details: &str, timeout_label: &str) -> String {
    let lower = details.to_ascii_lowercase();

    let prefix = if (lower.contains("whpx") || lower.contains("hypervisor"))
        && (lower.contains("not installed")
            || lower.contains("not usable")
            || lower.contains("failed")
            || lower.contains("error"))
    {
        "Falha de virtualização/WHPX"
    } else if lower.contains("vulkan") {
        "Falha na camada Vulkan/GPU"
    } else if lower.contains("opengl") || lower.contains("egl") || lower.contains("gfxstream") {
        "Falha na inicialização gráfica"
    } else if lower.contains("not enough memory")
        || lower.contains("commit charge")
        || lower.contains("out of memory")
    {
        "Memória insuficiente para a VM"
    } else {
        timeout_label
    };

    if details.is_empty() {
        format!("GPU {gpu}: {prefix}.")
    } else {
        format!("GPU {gpu}: {prefix}. Diagnóstico final: {details}")
    }
}

fn launch_emulator_attempt(
    runtime: &Path,
    profile: &ProfileSettings,
    plan: &AttemptPlan,
    attempt: usize,
    first_boot: bool,
) -> Result<String, String> {
    if launch_cancelled() {
        return Err("__cancelled__".into());
    }

    let port = find_free_emulator_port()?;
    let serial = format!("emulator-{port}");
    let emulator = emulator_path(runtime);
    let root = log_root(runtime);
    fs::create_dir_all(&root).map_err(|error| format!("Falha ao criar pasta de logs: {error}"))?;

    let tag = format!("{}-{}", attempt + 1, plan.gpu.replace('_', "-"));
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
            format!("launching:{}", plan.gpu)
        } else {
            format!("fallback:{}", plan.gpu)
        };
    });

    append_engine_log(
        runtime,
        &format!(
            "attempt={} gpu={} port={} cores={} ram={} cold_boot={} wipe={} no_vulkan={} emulator={}",
            attempt + 1,
            plan.gpu,
            port,
            profile.cores,
            profile.ram_mb,
            plan.cold_boot,
            plan.wipe_data,
            plan.disable_vulkan,
            emulator_version(runtime)
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
        .arg(plan.gpu)
        .arg("-cores")
        .arg(profile.cores.to_string())
        .arg("-memory")
        .arg(profile.ram_mb.to_string())
        .arg("-skin")
        .arg("720x1280")
        .arg("-no-metrics")
        .arg("-no-boot-anim")
        .arg("-no-audio")
        .arg("-camera-back")
        .arg("none")
        .arg("-camera-front")
        .arg("none")
        .arg("-netfast")
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));

    if plan.disable_vulkan {
        command.arg("-feature").arg("-Vulkan");
    }
    if plan.cold_boot {
        command.arg("-no-snapshot-load").arg("-no-snapshot-save");
    }
    if plan.wipe_data {
        command.arg("-wipe-data");
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("Não foi possível abrir emulator.exe: {error}"))?;
    let pid = child.id();
    let _ = fs::write(runtime.join("engine.pid"), pid.to_string());
    update_launch(|status| {
        status.pid = Some(pid);
        status.stage = format!("waiting-adb:{}", plan.gpu);
    });

    let adb_wait = if first_boot || plan.cold_boot { 180 } else { 120 };
    let adb_deadline = Instant::now() + Duration::from_secs(adb_wait);
    let mut launcher_handoff = false;

    while Instant::now() < adb_deadline {
        if launch_cancelled() {
            kill_process_tree(pid);
            kill_stale_runtime_emulators(runtime);
            return Err("__cancelled__".into());
        }

        if serial_online(runtime, &serial) {
            update_launch(|status| status.stage = format!("booting:{}", plan.gpu));
            append_engine_log(runtime, &format!("adb_online serial={serial}"));
            break;
        }

        if let Ok(Some(exit)) = child.try_wait() {
            if !exit.success() {
                thread::sleep(Duration::from_millis(700));
                let details = read_attempt_logs(runtime, &tag);
                kill_stale_runtime_emulators(runtime);
                return Err(classify_failure(
                    plan.gpu,
                    &details,
                    &format!("emulator.exe encerrou com {exit}"),
                ));
            }
            if !launcher_handoff {
                launcher_handoff = true;
                append_engine_log(runtime, "emulator.exe retornou sucesso; aguardando possível QEMU filho/ADB");
            }
        }

        thread::sleep(Duration::from_millis(750));
    }

    if !serial_online(runtime, &serial) {
        kill_process_tree(pid);
        kill_stale_runtime_emulators(runtime);
        thread::sleep(Duration::from_millis(900));
        let details = read_attempt_logs(runtime, &tag);
        return Err(classify_failure(
            plan.gpu,
            &details,
            &format!("Emulator não apareceu no ADB em {adb_wait} segundos"),
        ));
    }

    let boot_wait = if first_boot { 300 } else { 240 };
    let boot_deadline = Instant::now() + Duration::from_secs(boot_wait);
    while Instant::now() < boot_deadline {
        if launch_cancelled() {
            let _ = adb_result(runtime, &["-s", &serial, "emu", "kill"]);
            kill_process_tree(pid);
            kill_stale_runtime_emulators(runtime);
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

        thread::sleep(Duration::from_secs(1));
    }

    let _ = adb_result(runtime, &["-s", &serial, "emu", "kill"]);
    kill_process_tree(pid);
    kill_stale_runtime_emulators(runtime);
    let details = read_attempt_logs(runtime, &tag);
    Err(classify_failure(
        plan.gpu,
        &details,
        &format!("ADB conectou, mas o Android não concluiu o boot em {boot_wait} segundos"),
    ))
}

fn run_launch_sequence(runtime: PathBuf, profile_name: String) -> Result<(), String> {
    let _ = fs::create_dir_all(log_root(&runtime));
    let _ = fs::write(engine_log_path(&runtime), "");
    set_stage("preflight");
    repair_avd(&runtime)?;
    kill_stale_runtime_emulators(&runtime);
    thread::sleep(Duration::from_millis(700));

    let emulator = emulator_path(&runtime);
    if !emulator.exists() {
        return Err(format!("emulator.exe não encontrado: {}", emulator.display()));
    }

    let version = emulator_version(&runtime);
    append_engine_log(&runtime, &format!("NOVA boot engine · emulator={version}"));

    let _ = adb_result(&runtime, &["start-server"]);

    let mut accel_command = hidden_command(&emulator);
    if let Ok(output) = accel_command.arg("-accel-check").output() {
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
    let first_boot = !runtime.join("boot-ok.marker").exists();

    let plans: Vec<AttemptPlan> = if first_boot {
        settings.cores = settings.cores.min(2);
        settings.ram_mb = settings.ram_mb.min(3072);
        vec![
            AttemptPlan {
                gpu: "swiftshader_indirect",
                cold_boot: true,
                wipe_data: true,
                disable_vulkan: true,
            },
            AttemptPlan {
                gpu: "software",
                cold_boot: true,
                wipe_data: false,
                disable_vulkan: true,
            },
            AttemptPlan {
                gpu: "auto",
                cold_boot: true,
                wipe_data: false,
                disable_vulkan: false,
            },
        ]
    } else {
        settings
            .gpu_modes
            .iter()
            .enumerate()
            .map(|(index, gpu)| AttemptPlan {
                gpu: *gpu,
                cold_boot: index > 0,
                wipe_data: false,
                disable_vulkan: *gpu == "software" || *gpu == "swiftshader_indirect",
            })
            .collect()
    };

    let mut failures = Vec::new();
    for (attempt, plan) in plans.iter().enumerate() {
        if launch_cancelled() {
            return Err("__cancelled__".into());
        }

        match launch_emulator_attempt(&runtime, &settings, plan, attempt, first_boot) {
            Ok(serial) => {
                append_engine_log(
                    &runtime,
                    &format!("boot_success serial={serial} gpu={}", plan.gpu),
                );
                mark_launch_ready();
                return Ok(());
            }
            Err(error) if error == "__cancelled__" => return Err(error),
            Err(error) => {
                append_engine_log(&runtime, &format!("attempt_failed {error}"));
                let lower = error.to_ascii_lowercase();
                failures.push(error);
                if lower.contains("falha de virtualização/whpx") {
                    break;
                }
                thread::sleep(Duration::from_secs(2));
            }
        }
    }

    Err(format!(
        "Todas as tentativas de boot falharam com Emulator {version}. {}",
        limit_tail(failures.join(" | "), 5200)
    ))
}

fn stage_message(stage: &str) -> String {
    if stage == "preflight" {
        return "Limpando processos antigos, reparando o AVD e validando o runtime...".into();
    }
    if let Some(gpu) = stage.strip_prefix("launching:") {
        return format!("Abrindo Android com GPU {gpu}. No primeiro cold boot o ADB pode levar até 3 minutos...");
    }
    if let Some(gpu) = stage.strip_prefix("waiting-adb:") {
        return format!("Android Emulator está aberto com GPU {gpu}; aguardando o ADB aparecer (até 3 minutos no primeiro boot)...");
    }
    if let Some(gpu) = stage.strip_prefix("fallback:") {
        return format!("A tentativa anterior falhou; testando automaticamente o modo gráfico {gpu}...");
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
            "Runtime Android pronto. O NOVA fará um boot controlado pelo engine Rust.".into()
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
            message: "Verificando e reparando o runtime Android do NOVA...".into(),
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
        message: "Preparando cold boot seguro. A primeira inicialização pode levar alguns minutos.".into(),
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
    kill_stale_runtime_emulators(&runtime);
    let _ = fs::remove_file(runtime.join("engine.pid"));
    thread::sleep(Duration::from_millis(500));
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
