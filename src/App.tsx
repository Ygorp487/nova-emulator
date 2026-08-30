import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type EngineState = {
  state: "ready" | "runtime_missing" | "acceleration_missing" | "installing" | "starting" | "running" | "error";
  message: string;
  runtimeFound: boolean;
  adbFound: boolean;
  avdFound: boolean;
  running: boolean;
  acceleration: string;
};

const profiles = [
  { name: "Eco", cpu: "2 cores", ram: "2 GB", fps: "60 FPS" },
  { name: "Balanced", cpu: "4 cores", ram: "4 GB", fps: "60 FPS" },
  { name: "Performance", cpu: "4 cores", ram: "6 GB", fps: "120 FPS*" }
];

function StatusDot({ active }: { active: boolean }) {
  return <span className={active ? "status-dot online" : "status-dot"} />;
}

export default function App() {
  const [engine, setEngine] = useState<EngineState>({
    state: "runtime_missing",
    message: "Verificando runtime...",
    runtimeFound: false,
    adbFound: false,
    avdFound: false,
    running: false,
    acceleration: "verificando"
  });
  const [profile, setProfile] = useState("Balanced");
  const [busy, setBusy] = useState(false);

  async function refreshStatus() {
    try {
      setEngine(await invoke<EngineState>("engine_status"));
    } catch (error) {
      setEngine((current) => ({ ...current, state: "error", message: String(error), running: false }));
    }
  }

  async function installRuntime() {
    setBusy(true);
    try {
      setEngine(await invoke<EngineState>("install_runtime"));
    } catch (error) {
      setEngine((current) => ({ ...current, state: "error", message: String(error) }));
    } finally {
      setBusy(false);
    }
  }

  async function startEngine() {
    setBusy(true);
    try {
      setEngine(await invoke<EngineState>("start_engine", { profile: profile.toLowerCase() }));
      window.setTimeout(() => void refreshStatus(), 5000);
    } catch (error) {
      setEngine((current) => ({ ...current, state: "error", message: String(error) }));
    } finally {
      setBusy(false);
    }
  }

  async function stopEngine() {
    setBusy(true);
    try {
      setEngine(await invoke<EngineState>("stop_engine"));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void refreshStatus();
    const timer = window.setInterval(() => void refreshStatus(), 8000);
    return () => window.clearInterval(timer);
  }, []);

  const primaryLabel = !engine.runtimeFound
    ? "⬇ INSTALAR RUNTIME"
    : engine.running
      ? "■ PARAR ANDROID"
      : busy
        ? "PROCESSANDO..."
        : "▶ INICIAR ANDROID";

  const primaryAction = !engine.runtimeFound
    ? installRuntime
    : engine.running
      ? stopEngine
      : startEngine;

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand"><span className="brand-mark">N</span><div><strong>NOVA</strong><small>EMULATOR</small></div></div>
        <nav>
          <button className="nav-item active"><span>⌂</span>Início</button>
          <button className="nav-item"><span>▦</span>Apps</button>
          <button className="nav-item"><span>＋</span>Instalar APK</button>
          <button className="nav-item"><span>⌨</span>Controles</button>
          <button className="nav-item"><span>⚙</span>Configurações</button>
        </nav>
        <div className="sidebar-bottom">
          <div className="engine-pill"><StatusDot active={engine.running} /><div><b>Engine</b><small>{engine.running ? "Android online" : engine.runtimeFound ? "Runtime instalado" : "Runtime ausente"}</small></div></div>
          <span className="version">NOVA 0.2.0 · ENGINE MVP</span>
        </div>
      </aside>

      <main className="content">
        <header>
          <div><p className="eyebrow">ANDROID GAMING / WINDOWS</p><h1>Seu Android.<br/><span>Sem peso extra.</span></h1></div>
          <button className="ghost" onClick={() => void refreshStatus()}>↻ Atualizar</button>
        </header>

        <section className="hero-card">
          <div className="hero-copy">
            <span className="badge"><StatusDot active={engine.running || engine.state === "ready"} /> {engine.state.replaceAll("_", " ")}</span>
            <h2>{engine.running ? "Android em execução" : engine.runtimeFound ? "Engine preparado" : "Instale o runtime"}</h2>
            <p>{engine.message}</p>
            <div className="hero-actions">
              <button className="primary" onClick={() => void primaryAction()} disabled={busy || engine.state === "acceleration_missing"}>{primaryLabel}</button>
              {engine.runtimeFound && !engine.running && <button className="secondary" onClick={() => void installRuntime()}>Reparar runtime</button>}
            </div>
            <p className="eyebrow">WHPX: {engine.acceleration}</p>
          </div>
          <div className="device-stage">
            <div className="phone"><div className="camera"/><div className="android-orb">N</div><strong>NOVA Android</strong><small>Android 15 · x86_64</small></div>
            <div className="glow"/>
          </div>
        </section>

        <section className="section-title"><div><p className="eyebrow">PERFIL ATUAL</p><h3>Desempenho</h3></div><span>Altere antes de iniciar</span></section>
        <div className="profiles">
          {profiles.map((item) => (
            <button key={item.name} className={`profile-card ${profile === item.name ? "selected" : ""}`} onClick={() => setProfile(item.name)} disabled={engine.running}>
              <div className="profile-top"><strong>{item.name}</strong>{profile === item.name && <span>✓</span>}</div>
              <div className="specs"><span>CPU <b>{item.cpu}</b></span><span>RAM <b>{item.ram}</b></span><span>MAX <b>{item.fps}</b></span></div>
            </button>
          ))}
        </div>

        <section className="library">
          <div className="section-title"><div><p className="eyebrow">BIBLIOTECA</p><h3>Meus apps</h3></div><button className="text-button">Ver todos →</button></div>
          <div className="empty-library"><div className="empty-icon">＋</div><div><strong>Próximo: APK via ADB</strong><p>Com o Android iniciando, a próxima integração será seletor de APK + instalação e biblioteca real.</p></div><button className="secondary">Adicionar APK</button></div>
        </section>
      </main>
    </div>
  );
}
