import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ChevronRight, Copy, Download, FileText, FolderOpen, LoaderCircle, Network, Play, RefreshCw, RotateCcw, Save, Settings, ShieldCheck, Square, Trash2, Upload } from "lucide-react";
import appLogo from "../src-tauri/icons/icon.png";
import "./styles.css";

type Config = Record<string, string>;
type ServerStatus = { installed: boolean; steamcmd_found: boolean; executable?: string; is_running: boolean; install_dir?: string; message: string };
type NetworkReport = { port: number; listening: boolean; firewall_rule: boolean; public_ip?: string; external_verification: string };
type BackupResult = { message: string };
type InstallProgress = { stage: string; message: string };
type LibraryItem = { key: string; name: string; size_bytes: number; modified: string; compressed: boolean };
type WorldLibrary = { worlds: LibraryItem[]; backups: LibraryItem[] };
type BackupPlan = { enabled: boolean; interval_minutes: number; max_gib: number; compress_after_hours: number };
const blankConfig: Config = { OwnerId: "", ServerName: "My Dragonwilds Server", DefaultWorldName: "My World", AdminPassword: "", WorldPassword: "" };
const configKeys = ["OwnerId", "ServerName", "DefaultWorldName", "AdminPassword", "WorldPassword"];

function App() {
  const [installDir, setInstallDir] = useState("");
  const [status, setStatus] = useState<ServerStatus | null>(null);
  const [config, setConfig] = useState<Config>(blankConfig);
  const [network, setNetwork] = useState<NetworkReport | null>(null);
  const [backup, setBackup] = useState<BackupPlan>({ enabled: true, interval_minutes: 30, max_gib: 20, compress_after_hours: 24 });
  const [library, setLibrary] = useState<WorldLibrary>({ worlds: [], backups: [] });
  const [notice, setNotice] = useState("Choose a folder to install your dedicated server.");
  const [busy, setBusy] = useState<string | null>(null);
  const [installProgress, setInstallProgress] = useState<string | null>(null);
  const [configChanged, setConfigChanged] = useState(false);
  const [setupOpen, setSetupOpen] = useState(true);
  const lastAutomaticBackup = useRef(0);
  const configDirty = useRef(false);
  const savedConfig = useRef<Config>(blankConfig);
  const configRevision = useRef(0);
  const ready = useMemo(() => Boolean(config.OwnerId && config.ServerName && config.DefaultWorldName && config.AdminPassword), [config]);
  const call = async <T,>(name: string, args?: Record<string, unknown>) => invoke<T>(name, args);
  const refresh = async (root = installDir) => {
    if (!root) return;
    const next = await call<ServerStatus>("inspect_server", { installDir: root });
    setStatus(next);
    if (next.installed) {
      const revision = configRevision.current;
      const loadedConfig = await call<Config>("read_config", { installDir: root });
      if (!configDirty.current && revision === configRevision.current) {
        // The dedicated server can return an empty AdminPassword after it has
        // accepted the password. Keep the write-only value in this session so
        // status refreshes do not make a valid configuration look incomplete.
        const effectiveConfig = !loadedConfig.AdminPassword && savedConfig.current.AdminPassword
          ? { ...loadedConfig, AdminPassword: savedConfig.current.AdminPassword }
          : loadedConfig;
        setConfig(effectiveConfig);
        savedConfig.current = effectiveConfig;
      }
      setBackup(await call<BackupPlan>("get_backup_plan", { installDir: root }));
      setLibrary(await call<WorldLibrary>("list_world_library", { installDir: root }));
    }
  };
  useEffect(() => { call<string | null>("get_last_install_dir").then((path) => { if (path) { setInstallDir(path); refresh(path); } }).catch(() => undefined); }, []);
  useEffect(() => { let unlisten: (() => void) | undefined; listen<InstallProgress>("install-progress", (event) => setInstallProgress(event.payload.message)).then((remove) => { unlisten = remove; }); return () => unlisten?.(); }, []);
  useEffect(() => { const id = window.setInterval(() => refresh().catch(() => undefined), 5000); return () => clearInterval(id); }, [installDir]);
  useEffect(() => {
    const id = window.setInterval(() => {
      if (installDir && status?.installed && backup.enabled && !busy && Date.now() - lastAutomaticBackup.current >= backup.interval_minutes * 60000) {
        lastAutomaticBackup.current = Date.now();
        call<BackupResult>("run_backup", { installDir }).then(({ message }) => setNotice(message)).catch((error) => setNotice(String(error)));
      }
    }, 30000);
    return () => clearInterval(id);
  }, [installDir, status?.installed, backup.enabled, backup.interval_minutes, busy]);
  const run = async (label: string, action: () => Promise<void>) => { setBusy(label); try { await action(); } catch (error) { setNotice(String(error)); } finally { setBusy(null); } };
  const chooseFolder = () => run("folder", async () => { const selected = await open({ directory: true, multiple: false, title: "Choose a folder for Dragonwilds" }); if (typeof selected === "string") { configRevision.current += 1; configDirty.current = false; setConfigChanged(false); setInstallDir(selected); await call("remember_install_dir", { installDir: selected }); await refresh(selected); setNotice("Server folder selected."); } });
  const install = () => run("install", async () => { if (!installDir) throw new Error("Choose a folder first."); setInstallProgress("Preparing the installer…"); try { setNotice(await call<string>("install_server", { installDir })); await refresh(); } finally { setInstallProgress(null); } });
  const start = () => run("start", async () => { if (!ready) throw new Error("Complete Owner ID and server settings before starting."); await call("save_config", { installDir, config }); setNotice(await call<string>("start_server", { installDir })); await refresh(); });
  const stop = () => run("stop", async () => { setNotice(await call<string>("stop_server", {})); await refresh(); });
  const saveConfig = (event: FormEvent) => { event.preventDefault(); run("config", async () => { setNotice(await call<string>("save_config", { installDir, config })); configRevision.current += 1; savedConfig.current = config; configDirty.current = false; setConfigChanged(false); }); };
  const testNetwork = () => run("network", async () => { const report = await call<NetworkReport>("check_network", { installDir, port: 7777 }); setNetwork(report); setNotice(report.external_verification); });
  const importWorld = () => run("import", async () => { const selected = await open({ multiple: false, filters: [{ name: "Dragonwilds world", extensions: ["sav"] }] }); if (typeof selected === "string") { setNotice(await call<string>("import_world", { installDir, sourcePath: selected })); await refresh(); } });
  const copyWorld = (worldKey: string) => run("copy", async () => { setNotice(await call<string>("duplicate_world", { installDir, worldKey })); await refresh(); });
  const restoreBackup = (backupKey: string, name: string) => run("restore", async () => { if (!window.confirm(`Restore ${name}? Your current saves will be protected in a new backup.`)) return; setNotice(await call<string>("restore_backup", { installDir, backupKey })); await refresh(); });
  const deleteItem = (key: string, name: string, isBackup: boolean) => {
    if (!window.confirm(`Delete ${name}? This cannot be undone.`)) return;
    run("delete", async () => {
      setNotice(await call<string>("delete_world_item", { installDir, key, backup: isBackup }));
      await refresh();
    });
  };
  const update = (key: string, value: string) => setConfig((current) => { const next = { ...current, [key]: value }; const changed = configKeys.some((configKey) => next[configKey] !== savedConfig.current[configKey]); configRevision.current += 1; configDirty.current = changed; setConfigChanged(changed); return next; });
  const isRunning = Boolean(status?.is_running);
  const setupComplete = Boolean(status?.installed && ready);
  const wasSetupComplete = useRef(false);
  useEffect(() => {
    if (setupComplete && !wasSetupComplete.current) setSetupOpen(false);
    if (!setupComplete && wasSetupComplete.current) setSetupOpen(true);
    wasSetupComplete.current = setupComplete;
  }, [setupComplete]);
  const startDisabled = !status?.installed || !ready || Boolean(busy);
  return <main className="app-shell"><div className="dashboard">
      <header className="topbar"><div><div className="app-brand"><img src={appLogo} alt="DragonWarden"/><strong>DragonWarden</strong></div><h1>My Dragonwilds Server</h1><p>Manage your local dedicated server for RuneScape: Dragonwilds</p></div><div className="header-actions"><button className="ghost-button" onClick={() => openUrl("https://runescapedragonwilds.help.jagex.com/hc/en-gb/articles/45365343055249-Dedicated-Servers-How-to-Guide")}><FileText size={17}/> Setup guide</button><button className="icon-button" title="Refresh server data" onClick={() => refresh().catch((e) => setNotice(String(e)))}><RefreshCw size={18} className={busy ? "spin" : ""}/></button></div></header>
      <p className="notice" aria-live="polite">{notice}</p>
      <div className="content-grid" id="overview"><div className="main-column">
        <section className="setup-card"><button className="setup-summary" type="button" aria-expanded={setupOpen} aria-controls="setup-steps" onClick={() => setSetupOpen((open) => !open)}><div className="section-heading"><h2>{setupComplete ? "Quick setup complete" : "Quick setup"}</h2><p>{setupComplete ? "Your server is configured and ready to start." : "Get your server running in a few steps"}</p></div><ChevronRight size={18}/></button>{setupOpen && <div className="setup-steps" id="setup-steps"><SetupStep number="1" title="Select server location" text={installDir ? installDir : "Choose a folder to install the dedicated server files."} action="Choose folder" onClick={chooseFolder} busy={busy === "folder"} complete={Boolean(installDir)}/><SetupStep number="2" title="Install or update server" text={busy === "install" ? installProgress ?? "Contacting SteamCMD…" : "Download the latest dedicated server files from Steam."} action={status?.installed ? "Check for updates" : "Install / Update"} onClick={install} busy={busy === "install"} disabled={!installDir} complete={Boolean(status?.installed)}/><SetupStep number="3" title="Configure server" text="Set your server name, world, and required access options." action="Configure" onClick={() => document.getElementById("configuration")?.scrollIntoView({ behavior: "smooth", block: "start" })} complete={ready}/><SetupStep number="4" title="Start server" text="Launch your server and invite your friends." action="Start server" onClick={start} busy={busy === "start"} disabled={startDisabled} complete={isRunning} last/></div>}</section>
        <section className="hero-card"><div className="hero-copy"><span className={`server-status ${isRunning ? "running" : ""}`}>{isRunning ? "Server running" : "Server stopped"}</span><h2>{config.DefaultWorldName || "My World"}</h2><p>{isRunning ? "Active world · Your server is online and accepting players." : "Start your server to play with others."}</p></div>{isRunning ? <button className="danger-button" onClick={stop} disabled={Boolean(busy)}><Square size={15}/> Stop server</button> : <button className="primary-button" onClick={start} disabled={startDisabled}>{busy === "start" ? <LoaderCircle className="spin"/> : <Play size={16} fill="currentColor"/>} Start server</button>}</section>
        <section className="console-card" id="connection"><div className="section-heading inline-heading"><div><h2>Connection status</h2><p>Check your local network and port forwarding.</p></div><button className="ghost-button small" onClick={testNetwork} disabled={!status?.installed || Boolean(busy)}>{busy === "network" ? <LoaderCircle className="spin"/> : <Network size={15}/>} Run port check</button></div><div className="connection-list"><ConnectionRow label="Game port" value="7777" okay={network?.listening}/>{network?.public_ip && <ConnectionRow label="Public IP" value={network.public_ip} copy/>}</div></section>
        <section className="library-card" id="library"><div className="section-heading inline-heading"><div><h2>World library</h2><p>Import, duplicate, or restore your local worlds and managed backups.</p></div><button className="ghost-button small" onClick={importWorld} disabled={!status?.installed || isRunning || Boolean(busy)}><Upload size={15}/> Import world</button></div><div className="library-grid"><LibraryList title="Live worlds" items={library.worlds} empty="No saved worlds found." onCopy={copyWorld} onDelete={(item) => deleteItem(item.key, item.name, false)}/><LibraryList title="Backups" items={library.backups} empty="No managed backups yet." onRestore={restoreBackup} onDelete={(item) => deleteItem(item.key, item.name, true)}/></div></section>
      </div><div className="side-column">
        <form id="configuration" className="config-card" onSubmit={saveConfig}><div className="section-heading"><h2>Server configuration</h2><p>Basic settings to get you started.</p></div><Field label="Server name" value={config.ServerName} onChange={(value) => update("ServerName", value)} disabled={isRunning}/><Field label="World name" value={config.DefaultWorldName} onChange={(value) => update("DefaultWorldName", value)} disabled={isRunning}/><Field label="Owner ID" value={config.OwnerId} placeholder="Your in-game Player ID" onChange={(value) => update("OwnerId", value)} disabled={isRunning}/><Field label="Admin password" type="password" value={config.AdminPassword} placeholder="Required to start" onChange={(value) => update("AdminPassword", value)} disabled={isRunning}/><Field label="World password" type="password" value={config.WorldPassword} placeholder="Optional" onChange={(value) => update("WorldPassword", value)} disabled={isRunning}/><button className="primary-button full" disabled={isRunning || !status?.installed || !configChanged || Boolean(busy)} type="submit">{busy === "config" ? <LoaderCircle className="spin"/> : <Save size={16}/>} Save configuration</button></form>
        <details id="advanced" className="advanced-card"><summary><span><Settings size={17}/> Advanced settings</span><ChevronRight size={17}/></summary><div className="advanced-content"><label className="toggle-row"><span>Automatic backups <small>Every {backup.interval_minutes} minutes</small></span><input type="checkbox" checked={backup.enabled} onChange={(event) => setBackup({ ...backup, enabled: event.target.checked })}/></label><div className="two-fields"><NumberField label="Minutes" value={backup.interval_minutes} onChange={(value) => setBackup({ ...backup, interval_minutes: value })}/><NumberField label="Storage (GiB)" value={backup.max_gib} onChange={(value) => setBackup({ ...backup, max_gib: value })}/></div><button type="button" className="ghost-button small" onClick={() => run("backup settings", async () => { await call("set_backup_plan", { installDir, plan: backup }); setNotice("Backup settings saved."); })}><Save size={15}/> Save backup plan</button></div></details>
      </div></div>
    </div>
  </main>;
}
function SetupStep({ number, title, text, action, onClick, busy, disabled, complete, last }: { number: string; title: string; text: string; action: string; onClick: () => void; busy?: boolean; disabled?: boolean; complete?: boolean; last?: boolean }) { return <div className="setup-step"><div className={`step-number ${complete ? "complete" : ""}`}><b>{complete ? "✓" : number}</b>{!last && <i/>}</div><div className="step-copy"><strong>{title}</strong><p title={text}>{text}</p></div><button className="ghost-button small" onClick={onClick} disabled={disabled || busy}>{busy ? <LoaderCircle className="spin"/> : number === "1" ? <FolderOpen size={15}/> : number === "2" ? <Download size={15}/> : number === "3" ? <Settings size={15}/> : <Play size={14}/>} {action}</button></div>; }
function Field({ label, value, placeholder, type = "text", onChange, disabled }: { label: string; value: string; placeholder?: string; type?: string; onChange: (value: string) => void; disabled: boolean }) { return <label className="field-label"><span>{label}</span><input className="field" type={type} value={value} placeholder={placeholder} onChange={(event) => onChange(event.target.value)} disabled={disabled}/></label>; }
function NumberField({ label, value, onChange }: { label: string; value: number; onChange: (value: number) => void }) { return <label className="field-label"><span>{label}</span><input className="field" type="number" min="1" value={value} onChange={(event) => onChange(Number(event.target.value))}/></label>; }
function ConnectionRow({ label, value, okay, copy }: { label: string; value: string; okay?: boolean; copy?: boolean }) { return <div className="connection-row"><span><ShieldCheck size={15}/>{label}</span><span>{value}</span>{copy ? <button className="copy-button" onClick={() => navigator.clipboard.writeText(value)}><Copy size={13}/> Copy</button> : <em className={okay ? "okay" : "pending"}>{okay ? "Open" : "Check"}</em>}</div>; }
function LibraryList({ title, items, empty, onCopy, onRestore, onDelete }: { title: string; items: LibraryItem[]; empty: string; onCopy?: (key: string) => void; onRestore?: (key: string, name: string) => void; onDelete: (item: LibraryItem) => void }) { return <div className="library-list"><h3>{title}<small>{items.length}</small></h3>{items.length === 0 ? <p className="empty-state">{empty}</p> : items.map((item) => <div className="library-item" key={item.key}><div><strong>{item.name}</strong><span>{formatBytes(item.size_bytes)} · {item.modified}{item.compressed ? " · zipped" : ""}</span></div><div>{onCopy && <button type="button" title="Duplicate world" onClick={() => onCopy(item.key)}><Copy size={15}/></button>}{onRestore && <button type="button" title="Restore backup" onClick={() => onRestore(item.key, item.name)}><RotateCcw size={15}/></button>}<button type="button" title="Delete" onClick={() => onDelete(item)}><Trash2 size={15}/></button></div></div>)}</div>; }
function formatBytes(bytes: number) { if (bytes < 1024) return `${bytes} B`; const units = ["KB", "MB", "GB", "TB"]; let value = bytes / 1024; let index = 0; while (value >= 1024 && index < units.length - 1) { value /= 1024; index++; } return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[index]}`; }
createRoot(document.getElementById("root")!).render(<App/>);
