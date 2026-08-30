use serde::Serialize;
use std::{path::{Path, PathBuf}, process::Command};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineState {
    state: String,
    message: String,
    qemu_found: bool,
}

fn project_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn qemu_path(root: &Path) -> PathBuf {
    root.join("engine").join("runtime").join("qemu").join("qemu-system-x86_64.exe")
}

#[tauri::command]
fn engine_status() -> EngineState {
    let root = project_root();
    let qemu = qemu_path(&root);
    if qemu.exists() {
        EngineState {
            state: "ready".into(),
            message: "QEMU detectado. O próximo passo é provisionar a imagem Android x86_64.".into(),
            qemu_found: true,
        }
    } else {
        EngineState {
            state: "runtime_missing".into(),
            message: "Interface pronta. O runtime QEMU/Android ainda não foi provisionado.".into(),
            qemu_found: false,
        }
    }
}

#[tauri::command]
fn start_engine(profile: String) -> EngineState {
    let root = project_root();
    let qemu = qemu_path(&root);
    if !qemu.exists() {
        return EngineState {
            state: "runtime_missing".into(),
            message: format!("Perfil {profile} selecionado. Falta instalar o runtime do engine."),
            qemu_found: false,
        };
    }

    let script = root.join("engine").join("scripts").join("start-engine.ps1");
    match Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script)
        .arg("-Profile")
        .arg(profile)
        .spawn()
    {
        Ok(_) => EngineState {
            state: "starting".into(),
            message: "Engine iniciado. A integração de vídeo/input será conectada no próximo milestone.".into(),
            qemu_found: true,
        },
        Err(error) => EngineState {
            state: "error".into(),
            message: format!("Falha ao iniciar engine: {error}"),
            qemu_found: true,
        },
    }
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![engine_status, start_engine])
        .run(tauri::generate_context!())
        .expect("error while running NOVA Emulator");
}
