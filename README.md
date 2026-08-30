# NOVA Emulator

NOVA is an experimental lightweight Android emulator for Windows focused on low overhead, gaming-oriented controls and a clean desktop experience.

## Jeito mais fácil — ZIP

Baixe o repositório como ZIP, extraia a pasta e use um destes arquivos:

### `NOVA-INSTALAR-E-ABRIR.bat`

Dê dois cliques. O assistente verifica o PC e instala automaticamente, quando necessário:

- Node.js LTS
- Rust / Cargo
- Visual C++ Build Tools
- Microsoft Edge WebView2 Runtime
- dependências npm do NOVA

Depois ele compila a versão de desenvolvimento e abre o NOVA.

### `NOVA-GERAR-EXE.bat`

Dê dois cliques para verificar/instalar as mesmas dependências e gerar o instalador do Windows.

O resultado fica em:

```text
NOVA-BUILD\NOVA-Setup.exe
```

O Windows pode pedir permissão de administrador para instalar componentes do sistema.

## Instalador gerado pelo GitHub

O workflow `NOVA CI` também compila um instalador NSIS no Windows e publica o artefato `NOVA-Windows-Setup` nas execuções do GitHub Actions.

## Engine MVP 0.2

- Tauri + React desktop shell
- Rust backend commands
- runtime Android instalado fora de `Program Files`, em uma pasta gravável do usuário
- Android Emulator/QEMU + ADB
- Windows Hypervisor Platform (WHPX)
- Android 15 / API 35 x86_64 AVD
- GPU acceleration (`host` / `auto` profiles)
- ADB + `sys.boot_completed` health detection
- start/stop pelo launcher NOVA
- scripts do engine embutidos no instalador Windows

## Primeiro uso do Android

Depois de abrir o NOVA, clique em **Instalar Runtime**. Essa etapa baixa as ferramentas Android necessárias, cria o AVD NOVA e verifica a aceleração de hardware.

O Android SDK exige que as licenças sejam apresentadas ao usuário durante a instalação. Se WHPX estiver desativado, o NOVA pode abrir o script incluído para ativar Windows Hypervisor Platform; uma reinicialização do Windows pode ser necessária.

## Desenvolvimento manual

O fluxo antigo continua disponível:

```powershell
.\setup-dev.ps1
npm run tauri dev
```

Mas normalmente basta usar `NOVA-INSTALAR-E-ABRIR.bat`.

## Estrutura

```text
src/                    React launcher UI
src-tauri/              Rust/Tauri desktop backend
engine/config/          configurações do engine
engine/scripts/         instalador e launcher do Android
engine/runtime/         SDK/AVD local (gitignored)
tools/                  bootstrap automático do Windows
scripts/                preparação dos recursos do instalador
NOVA-GERAR-EXE.bat      gera NOVA-Setup.exe
NOVA-INSTALAR-E-ABRIR.bat instala dependências e abre
.github/workflows/      compilação Windows automática
```

## Limitação atual

O MVP usa Android x86_64 e ainda não possui tradução nativa de bibliotecas ARM. Alguns jogos Android que só incluem bibliotecas ARM precisarão de uma etapa posterior de compatibilidade ARM.
