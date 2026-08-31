import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type EngineState = {
  state: "ready" | "runtime_missing" | "acceleration_missing" | "installing" | "starting" | "running" | "error";
  message: string;
  runtimeFound: boolean;
  adbFound: boolean;
  avdFound: boolean;
  running: boolean;
  bootComplete: boolean;
  acceleration: string;
};

type InstalledApp = { package: string };
type Page = "home" | "apps" | "apk" | "controls" | "settings";

type Bindings = {
  up: string;
  down: string;
  left: string;
  right: string;
  jump: string;
  action: string;
};

const profiles = [
  { name: "Eco", cpu: "2 cores", ram: "2 GB", fps: "60 FPS" },
  { name: "Balanced", cpu: "3 cores", ram: "4 GB", fps: "60 FPS" },
  { name: "Performance", cpu: "4 cores", ram: "6 GB", fps: "120 FPS*" }
];

const defaultBindings: Bindings = {
  up: "W",
  down: "S",
  left: "A",
  right: "D",
  jump: "SPACE",
  action: "MOUSE 1"
};

function StatusDot({ active }: { active: boolean }) {
  return <span className={active ? "status-dot online" : "status-dot"} />;
}

function PageTitle({ eyebrow, title, subtitle }: { eyebrow: string; title: string; subtitle: string }) {
  return (
    <div className="page-title">
      <p className="eyebrow">{eyebrow}</p>
      <h1>{title}</h1>
      <p>{subtitle}</p>
    </div>
  );
}

export default function App() {
  const [page, setPage] = useState<Page>("home");
  const [engine, setEngine] = useState<EngineState>({
    state: "runtime_missing",
    message: "Verificando ambiente Android...",
    runtimeFound: false,
    adbFound: false,
    avdFound: false,
    running: false,
    bootComplete: false,
    acceleration: "verificando"
  });
  const [profile, setProfile] = useState(() => localStorage.getItem("nova.profile") || "Balanced");
  const [busy, setBusy] = useState(false);
  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [appsMessage, setAppsMessage] = useState("");
  const [apkMessage, setApkMessage] = useState("");
  const [bindings, setBindings] = useState<Bindings>(() => {
    try {
      const saved = localStorage.getItem("nova.bindings");
      return saved ? { ...defaultBindings, ...JSON.parse(saved) } : defaultBindings;
    } catch {
      return defaultBindings;
    }
  });
  const [controlsSaved, setControlsSaved] = useState(false);
  const autoRuntimeRequested = useRef(false);

  async function refreshStatus() {
    try {
      const state = await invoke<EngineState>("engine_status");
      setEngine(state);

      if (state.state === "runtime_missing" && !autoRuntimeRequested.current) {
        autoRuntimeRequested.current = true;
        void installRuntime(true);
      }
    } catch (error) {
      setEngine((current) => ({ ...current, state: "error", message: String(error), running: false, bootComplete: false }));
    }
  }

  async function installRuntime(automatic = false) {
    setBusy(true);
    try {
      const state = await invoke<EngineState>("install_runtime");
      setEngine(state);
      if (!automatic) setPage("home");
    } catch (error) {
      setEngine((current) => ({ ...current, state: "error", message: String(error) }));
      autoRuntimeRequested.current = false;
    } finally {
      setBusy(false);
    }
  }

  async function startEngine() {
    setBusy(true);
    try {
      localStorage.setItem("nova.profile", profile);
      setEngine(await invoke<EngineState>("start_engine", { profile: profile.toLowerCase() }));
      window.setTimeout(() => void refreshStatus(), 800);
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
      setApps([]);
    } catch (error) {
      setEngine((current) => ({ ...current, state: "error", message: String(error) }));
    } finally {
      setBusy(false);
    }
  }

  async function loadApps() {
    setAppsMessage("");
    if (!engine.bootComplete) {
      setAppsMessage("Inicie o Android primeiro. A navegação do NOVA funciona sem ele, mas a biblioteca vem do Android via ADB.");
      return;
    }
    try {
      const result = await invoke<InstalledApp[]>("list_apps");
      setApps(result);
      setAppsMessage(result.length ? `${result.length} app(s) de usuário encontrado(s).` : "Nenhum app de usuário instalado ainda.");
    } catch (error) {
      setAppsMessage(String(error));
    }
  }

  async function installApk() {
    setApkMessage("Abrindo seletor de APK...");
    try {
      const result = await invoke<string>("install_apk");
      setApkMessage(result);
      await loadApps();
    } catch (error) {
      setApkMessage(String(error));
    }
  }

  async function launchApp(packageName: string) {
    try {
      setAppsMessage(await invoke<string>("launch_app", { package: packageName }));
    } catch (error) {
      setAppsMessage(String(error));
    }
  }

  function saveControls() {
    localStorage.setItem("nova.bindings", JSON.stringify(bindings));
    setControlsSaved(true);
    window.setTimeout(() => setControlsSaved(false), 1800);
  }

  function chooseProfile(next: string) {
    setProfile(next);
    localStorage.setItem("nova.profile", next);
  }

  useEffect(() => {
    void refreshStatus();
    const timer = window.setInterval(() => void refreshStatus(), 1500);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (page === "apps") void loadApps();
  }, [page, engine.bootComplete]);

  const runtimePreparing = !engine.runtimeFound && (engine.state === "runtime_missing" || engine.state === "installing");
  const booting = engine.state === "starting";
  const primaryLabel = runtimePreparing
    ? "PREPARANDO ANDROID..."
    : booting
      ? "■ CANCELAR BOOT"
      : engine.running
        ? "■ PARAR ANDROID"
        : busy
          ? "PROCESSANDO..."
          : "▶ INICIAR ANDROID";

  const heroTitle = engine.bootComplete
    ? "Android pronto"
    : booting
      ? "Android iniciando"
      : engine.state === "error"
        ? "Falha ao iniciar"
        : runtimePreparing
          ? "Preparando ambiente Android"
          : engine.runtimeFound
            ? "Engine preparado"
            : "Ambiente precisa de reparo";

  function renderHome() {
    return (
      <>
        <header>
          <div><p className="eyebrow">ANDROID GAMING / WINDOWS</p><h1>Seu Android.<br/><span>Sem peso extra.</span></h1></div>
          <button className="ghost" onClick={() => void refreshStatus()}>↻ Atualizar</button>
        </header>

        <section className="hero-card">
          <div className="hero-copy">
            <span className="badge"><StatusDot active={engine.bootComplete || engine.state === "ready"} /> {engine.state.replaceAll("_", " ")}</span>
            <h2>{heroTitle}</h2>
            <p>{engine.message}</p>
            <div className="hero-actions">
              {runtimePreparing ? (
                <button className="primary" disabled>PREPARANDO ANDROID...</button>
              ) : (
                <button className="primary" onClick={() => void ((engine.running || booting) ? stopEngine() : startEngine())} disabled={busy}>{primaryLabel}</button>
              )}
              {engine.state === "error" && <button className="secondary" onClick={() => void installRuntime(false)}>Reparar ambiente</button>}
            </div>
            <p className="eyebrow">WHPX: {engine.acceleration}</p>
          </div>
          <div className="device-stage">
            <div className="phone"><div className="camera"/><div className="android-orb">N</div><strong>NOVA Android</strong><small>{engine.bootComplete ? "Sistema pronto · ADB conectado" : "Android 15 · x86_64"}</small></div>
            <div className="glow"/>
          </div>
        </section>

        <section className="section-title"><div><p className="eyebrow">PERFIL ATUAL</p><h3>Desempenho</h3></div><span>Altere antes de iniciar</span></section>
        <div className="profiles">
          {profiles.map((item) => (
            <button key={item.name} className={`profile-card ${profile === item.name ? "selected" : ""}`} onClick={() => chooseProfile(item.name)} disabled={engine.running || booting}>
              <div className="profile-top"><strong>{item.name}</strong>{profile === item.name && <span>✓</span>}</div>
              <div className="specs"><span>CPU <b>{item.cpu}</b></span><span>RAM <b>{item.ram}</b></span><span>MAX <b>{item.fps}</b></span></div>
            </button>
          ))}
        </div>

        <section className="library">
          <div className="section-title"><div><p className="eyebrow">BIBLIOTECA</p><h3>Meus apps</h3></div><button className="text-button" onClick={() => setPage("apps")}>Ver todos →</button></div>
          <div className="empty-library"><div className="empty-icon">＋</div><div><strong>Instale seus APKs</strong><p>Escolha um APK pelo Windows e o NOVA instala diretamente no Android por ADB.</p></div><button className="secondary" onClick={() => setPage("apk")}>Adicionar APK</button></div>
        </section>
      </>
    );
  }

  function renderApps() {
    return (
      <>
        <PageTitle eyebrow="BIBLIOTECA ANDROID" title="Apps" subtitle="Aplicativos instalados pelo usuário dentro do NOVA." />
        <div className="page-actions">
          <button className="secondary" onClick={() => void loadApps()}>↻ Atualizar lista</button>
          <button className="primary" onClick={() => setPage("apk")}>＋ Instalar APK</button>
        </div>
        {appsMessage && <div className="notice">{appsMessage}</div>}
        {!engine.bootComplete ? (
          <div className="panel empty-panel"><span className="big-icon">◉</span><h3>Android está desligado</h3><p>Você pode usar o restante da interface normalmente. Para listar ou abrir apps, inicie o Android na tela Início.</p><button className="secondary" onClick={() => setPage("home")}>Ir para Início</button></div>
        ) : apps.length ? (
          <div className="apps-grid">
            {apps.map((app) => <button className="app-card" key={app.package} onClick={() => void launchApp(app.package)}><span className="app-icon">A</span><strong>{app.package.split(".").pop()}</strong><small>{app.package}</small><em>Abrir →</em></button>)}
          </div>
        ) : (
          <div className="panel empty-panel"><span className="big-icon">＋</span><h3>Nenhum app instalado</h3><p>Instale seu primeiro APK e ele aparecerá aqui.</p><button className="primary" onClick={() => setPage("apk")}>Escolher APK</button></div>
        )}
      </>
    );
  }

  function renderApk() {
    return (
      <>
        <PageTitle eyebrow="INSTALAÇÃO VIA ADB" title="Instalar APK" subtitle="Selecione um arquivo .apk do seu computador e envie para o Android." />
        <div className="panel apk-panel">
          <div className="drop-symbol">＋</div>
          <h2>Escolher arquivo APK</h2>
          <p>O Android precisa estar iniciado e com o boot concluído. A instalação usa ADB diretamente, sem precisar arrastar arquivos para a janela.</p>
          <button className="primary large" onClick={() => void installApk()} disabled={!engine.bootComplete}>Selecionar APK</button>
          {!engine.bootComplete && <button className="secondary" onClick={() => setPage("home")}>Iniciar Android primeiro</button>}
          {apkMessage && <div className="notice">{apkMessage}</div>}
        </div>
      </>
    );
  }

  function renderControls() {
    const fields: Array<[keyof Bindings, string]> = [["up","Mover para cima"],["down","Mover para baixo"],["left","Mover para esquerda"],["right","Mover para direita"],["jump","Pular"],["action","Ação / tiro"]];
    return (
      <>
        <PageTitle eyebrow="INPUT" title="Controles" subtitle="Edite e salve seu perfil de teclado e mouse." />
        <div className="panel">
          <div className="settings-heading"><div><h3>Perfil padrão</h3><p>As teclas ficam salvas no NOVA. A camada avançada de injeção por jogo será ligada ao motor nas próximas versões.</p></div><button className="primary" onClick={saveControls}>{controlsSaved ? "✓ Salvo" : "Salvar controles"}</button></div>
          <div className="binding-grid">
            {fields.map(([key, label]) => <label className="binding-row" key={key}><span>{label}</span><input value={bindings[key]} onChange={(event) => setBindings((current) => ({ ...current, [key]: event.target.value.toUpperCase() }))} /></label>)}
          </div>
        </div>
      </>
    );
  }

  function renderSettings() {
    return (
      <>
        <PageTitle eyebrow="NOVA" title="Configurações" subtitle="Ajustes de desempenho e diagnóstico do ambiente Android." />
        <div className="panel settings-panel">
          <div className="settings-block"><span>Perfil de desempenho</span><div className="segmented">{profiles.map((item) => <button key={item.name} className={profile === item.name ? "active" : ""} onClick={() => chooseProfile(item.name)} disabled={engine.running || booting}>{item.name}</button>)}</div></div>
          <div className="settings-block"><span>Runtime Android</span><strong>{engine.runtimeFound && engine.adbFound && engine.avdFound ? "Instalado" : "Incompleto"}</strong></div>
          <div className="settings-block"><span>ADB</span><strong>{engine.adbFound ? "Disponível" : "Ausente"}</strong></div>
          <div className="settings-block"><span>Dispositivo virtual</span><strong>{engine.avdFound ? "NOVA AVD pronto" : "Ausente"}</strong></div>
          <div className="settings-block"><span>Aceleração</span><strong className="diagnostic-text">{engine.acceleration}</strong></div>
          <div className="settings-footer"><button className="secondary" onClick={() => void refreshStatus()}>Atualizar diagnóstico</button><button className="secondary" onClick={() => void installRuntime(false)}>Reparar runtime</button></div>
        </div>
      </>
    );
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand"><span className="brand-mark">N</span><div><strong>NOVA</strong><small>EMULATOR</small></div></div>
        <nav>
          <button className={`nav-item ${page === "home" ? "active" : ""}`} onClick={() => setPage("home")}><span>⌂</span>Início</button>
          <button className={`nav-item ${page === "apps" ? "active" : ""}`} onClick={() => setPage("apps")}><span>▦</span>Apps</button>
          <button className={`nav-item ${page === "apk" ? "active" : ""}`} onClick={() => setPage("apk")}><span>＋</span>Instalar APK</button>
          <button className={`nav-item ${page === "controls" ? "active" : ""}`} onClick={() => setPage("controls")}><span>⌨</span>Controles</button>
          <button className={`nav-item ${page === "settings" ? "active" : ""}`} onClick={() => setPage("settings")}><span>⚙</span>Configurações</button>
        </nav>
        <div className="sidebar-bottom">
          <div className="engine-pill"><StatusDot active={engine.bootComplete} /><div><b>Engine</b><small>{engine.bootComplete ? "Android pronto" : booting ? "Boot em andamento" : engine.runtimeFound ? "Runtime instalado" : "Preparando runtime"}</small></div></div>
          <span className="version">NOVA 0.3.1 · COMPAT RUNTIME</span>
        </div>
      </aside>

      <main className="content">
        {page === "home" && renderHome()}
        {page === "apps" && renderApps()}
        {page === "apk" && renderApk()}
        {page === "controls" && renderControls()}
        {page === "settings" && renderSettings()}
      </main>
    </div>
  );
}
