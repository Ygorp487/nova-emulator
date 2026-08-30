import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type EngineState = {
  state: "ready" | "runtime_missing" | "starting" | "error";
  message: string;
  qemuFound: boolean;
};

const profiles = [
  { name: "Eco", cpu: "2 cores", ram: "2 GB", fps: "60 FPS" },
  { name: "Balanced", cpu: "4 cores", ram: "4 GB", fps: "60 FPS" },
  { name: "Performance", cpu: "4+ cores", ram: "6 GB", fps: "120 FPS" }
];

function StatusDot({ active }: { active: boolean }) {
  return <span className={active ? "status-dot online" : "status-dot"} />;
}

export default function App() {
  const [engine, setEngine] = useState<EngineState>({
    state: "runtime_missing",
    message: "Verificando runtime...",
    qemuFound: false
  });
  const [profile, setProfile] = useState("Balanced");
  const [busy, setBusy] = useState(false);

  async function refreshStatus() {
    try {
      setEngine(await invoke<EngineState>("engine_status"));
    } catch (error) {
      setEngine({ state: "error", message: String(error), qemuFound: false });
    }
  }

  async function startEngine() {
    setBusy(true);
    try {
      const result = await invoke<EngineState>("start_engine", { profile: profile.toLowerCase() });
      setEngine(result);
    } catch (error) {
      setEngine({ state: "error", message: String(error), qemuFound: false });
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    void refreshStatus();
  }, []);

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
          <div className="engine-pill"><StatusDot active={engine.qemuFound} /><div><b>Engine</b><small>{engine.qemuFound ? "Runtime detectado" : "Aguardando runtime"}</small></div></div>
          <span className="version">NOVA 0.1.0 · MVP</span>
        </div>
      </aside>

      <main className="content">
        <header>
          <div><p className="eyebrow">ANDROID GAMING / WINDOWS</p><h1>Seu Android.<br/><span>Sem peso extra.</span></h1></div>
          <button className="ghost" onClick={() => void refreshStatus()}>↻ Atualizar</button>
        </header>

        <section className="hero-card">
          <div className="hero-copy">
            <span className="badge"><StatusDot active={engine.qemuFound} /> {engine.state.replace("_", " ")}</span>
            <h2>Pronto para iniciar</h2>
            <p>{engine.message}</p>
            <div className="hero-actions">
              <button className="primary" onClick={() => void startEngine()} disabled={busy}>{busy ? "INICIANDO..." : "▶ INICIAR ANDROID"}</button>
              <button className="secondary">＋ APK</button>
            </div>
          </div>
          <div className="device-stage">
            <div className="phone"><div className="camera"/><div className="android-orb">N</div><strong>NOVA Android</strong><small>Runtime x86_64</small></div>
            <div className="glow"/>
          </div>
        </section>

        <section className="section-title"><div><p className="eyebrow">PERFIL ATUAL</p><h3>Desempenho</h3></div><span>Altere quando quiser</span></section>
        <div className="profiles">
          {profiles.map((item) => (
            <button key={item.name} className={`profile-card ${profile === item.name ? "selected" : ""}`} onClick={() => setProfile(item.name)}>
              <div className="profile-top"><strong>{item.name}</strong>{profile === item.name && <span>✓</span>}</div>
              <div className="specs"><span>CPU <b>{item.cpu}</b></span><span>RAM <b>{item.ram}</b></span><span>MAX <b>{item.fps}</b></span></div>
            </button>
          ))}
        </div>

        <section className="library">
          <div className="section-title"><div><p className="eyebrow">BIBLIOTECA</p><h3>Meus apps</h3></div><button className="text-button">Ver todos →</button></div>
          <div className="empty-library"><div className="empty-icon">＋</div><div><strong>Instale seu primeiro APK</strong><p>Na próxima etapa, este botão abrirá o seletor de APK e instalará via ADB.</p></div><button className="secondary">Adicionar APK</button></div>
        </section>
      </main>
    </div>
  );
}
