use chrono::Local;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::Mutex,
    time::{Duration, SystemTime},
};
use tauri::State;
use walkdir::WalkDir;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

const APP_ID: &str = "4019830";
const REQUIRED: [&str; 4] = ["OwnerId", "ServerName", "DefaultWorldName", "AdminPassword"];

#[derive(Default)]
struct AppState {
    server: Mutex<Option<Child>>,
}
#[derive(Serialize)]
struct ServerStatus {
    installed: bool,
    steamcmd_found: bool,
    executable: Option<String>,
    is_running: bool,
    install_dir: Option<String>,
    message: String,
}
#[derive(Serialize)]
struct NetworkReport {
    port: u16,
    listening: bool,
    firewall_rule: bool,
    public_ip: Option<String>,
    external_verification: String,
}
#[derive(Serialize)]
struct BackupResult {
    changed: bool,
    message: String,
    backup: Option<String>,
    total_bytes: u64,
}
#[derive(Serialize, Deserialize, Clone)]
struct BackupPlan {
    enabled: bool,
    interval_minutes: u64,
    max_gib: u64,
    compress_after_hours: u64,
}
#[derive(Serialize, Deserialize)]
struct LocalSettings {
    install_dir: String,
}
impl Default for BackupPlan {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_minutes: 30,
            max_gib: 20,
            compress_after_hours: 24,
        }
    }
}

fn root(path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    if p.as_os_str().is_empty() {
        Err("Choose a server folder first.".into())
    } else {
        Ok(p)
    }
}
fn game_root(root: &Path) -> PathBuf {
    root.join("RSDragonwilds")
}
fn config_path(root: &Path) -> PathBuf {
    game_root(root)
        .join("Saved")
        .join("Config")
        .join("WindowsServer")
        .join("DedicatedServer.ini")
}
fn saves_path(root: &Path) -> PathBuf {
    game_root(root).join("Saved").join("SaveGames")
}
fn backups_path(root: &Path) -> PathBuf {
    root.join("DragonWarden Backups")
}
fn plan_path(root: &Path) -> PathBuf {
    root.join(".dragonwarden-backup-plan.json")
}
fn local_settings_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("DragonWarden")
        .join("settings.json")
}
fn find_exe(root: &Path) -> Option<PathBuf> {
    [
        game_root(root).join("Binaries/Win64/RSDragonwildsServer-Win64-Shipping.exe"),
        game_root(root).join("Binaries/Win64/RSDragonwilds.exe"),
        root.join("RSDragonwilds.exe"),
    ]
    .into_iter()
    .find(|p| p.exists())
}
fn steamcmd(root: &Path) -> Option<PathBuf> {
    let local = root.join("steamcmd").join("steamcmd.exe");
    if local.exists() {
        Some(local)
    } else {
        None
    }
}
fn parse_config(file: &Path) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    if let Ok(text) = fs::read_to_string(file) {
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                if !k.trim_start().starts_with(';') && !k.trim_start().starts_with('[') {
                    values.insert(k.trim().to_owned(), v.trim().to_owned());
                }
            }
        }
    }
    values
}
fn default_config() -> BTreeMap<String, String> {
    let mut v = BTreeMap::new();
    v.insert("OwnerId".into(), "".into());
    v.insert("ServerName".into(), "My Dragonwilds Server".into());
    v.insert("DefaultWorldName".into(), "My World".into());
    v.insert("AdminPassword".into(), "".into());
    v.insert("WorldPassword".into(), "".into());
    v
}
fn write_config(file: &Path, mut values: BTreeMap<String, String>) -> Result<(), String> {
    for (key, value) in &values {
        if key.contains('\n') || value.contains('\n') || key.contains('=') {
            return Err("Configuration keys and values cannot contain new lines or '='.".into());
        }
    }
    for (key, value) in default_config() {
        values.entry(key).or_insert(value);
    }
    let mut output =
        String::from("; Managed by DragonWarden\r\n[/Script/Dominion.DedicatedServerSettings]\r\n");
    for (key, value) in values {
        output.push_str(&format!("{key}={value}\r\n"));
    }
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(file, output).map_err(|e| e.to_string())
}
fn is_running(state: &AppState) -> bool {
    state
        .server
        .lock()
        .ok()
        .and_then(|mut child| {
            child.as_mut().and_then(|c| c.try_wait().ok()).map(|_| {
                *child = None;
            })
        })
        .is_none()
        && state.server.lock().map(|x| x.is_some()).unwrap_or(false)
}

#[tauri::command]
fn get_last_install_dir() -> Option<String> {
    fs::read_to_string(local_settings_path())
        .ok()
        .and_then(|text| serde_json::from_str::<LocalSettings>(&text).ok())
        .map(|settings| settings.install_dir)
}

#[tauri::command]
fn remember_install_dir(install_dir: String) -> Result<(), String> {
    let path = local_settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        path,
        serde_json::to_string(&LocalSettings { install_dir }).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn inspect_server(install_dir: String, state: State<AppState>) -> Result<ServerStatus, String> {
    let root = root(&install_dir)?;
    let executable = find_exe(&root);
    let installed = executable.is_some();
    let steam = steamcmd(&root).is_some();
    let running = is_running(&state);
    Ok(ServerStatus {
        installed,
        steamcmd_found: steam,
        executable: executable.map(|p| p.display().to_string()),
        is_running: running,
        install_dir: Some(root.display().to_string()),
        message: if running {
            "The server process is running.".into()
        } else if installed {
            "Ready to start.".into()
        } else {
            "Install the dedicated server in this folder.".into()
        },
    })
}

#[tauri::command]
fn install_server(install_dir: String) -> Result<String, String> {
    let root = root(&install_dir)?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let steam = root.join("steamcmd").join("steamcmd.exe");
    if !steam.exists() {
        let download = root.join("steamcmd.zip");
        let quote = |p: &Path| p.display().to_string().replace('\'', "''");
        let script = format!("$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -Uri 'https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip' -OutFile '{}'; Expand-Archive -Force -LiteralPath '{}' -DestinationPath '{}'; Remove-Item -LiteralPath '{}'", quote(&download), quote(&download), quote(&root.join("steamcmd")), quote(&download));
        let status = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .status()
            .map_err(|e| format!("Could not download SteamCMD: {e}"))?;
        if !status.success() {
            return Err("SteamCMD download failed. Check your internet connection and Windows Defender settings.".into());
        }
    }
    let status = Command::new(&steam)
        .args([
            "+force_install_dir",
            &root.display().to_string(),
            "+login",
            "anonymous",
            "+app_update",
            APP_ID,
            "validate",
            "+quit",
        ])
        .status()
        .map_err(|e| format!("Could not run SteamCMD: {e}"))?;
    if !status.success() {
        return Err(
            "SteamCMD could not install the dedicated server. See its console for details.".into(),
        );
    }
    Ok("Server installation/update completed. Configure it before starting.".into())
}

#[tauri::command]
fn read_config(install_dir: String) -> Result<BTreeMap<String, String>, String> {
    let root = root(&install_dir)?;
    let mut values = default_config();
    values.extend(parse_config(&config_path(&root)));
    Ok(values)
}

#[tauri::command]
fn save_config(
    install_dir: String,
    config: BTreeMap<String, String>,
    state: State<AppState>,
) -> Result<String, String> {
    if is_running(&state) {
        return Err("Stop the server before editing DedicatedServer.ini. The game overwrites changes while it is running.".into());
    }
    let root = root(&install_dir)?;
    for key in REQUIRED {
        if config.get(key).map(|v| v.trim().is_empty()).unwrap_or(true) {
            return Err(format!("{key} is required."));
        }
    }
    let mut merged = parse_config(&config_path(&root));
    merged.extend(config);
    write_config(&config_path(&root), merged)?;
    Ok("DedicatedServer.ini saved. Unknown server-generated values were preserved.".into())
}

#[tauri::command]
fn start_server(install_dir: String, state: State<AppState>) -> Result<String, String> {
    if is_running(&state) {
        return Err("The server is already running.".into());
    }
    let root = root(&install_dir)?;
    let cfg = parse_config(&config_path(&root));
    for key in REQUIRED {
        if cfg.get(key).map(|v| v.trim().is_empty()).unwrap_or(true) {
            return Err("Save a complete configuration first.".into());
        }
    }
    let exe = find_exe(&root).ok_or("Server executable not found. Install or repair it first.")?;
    let mut command = Command::new(&exe);
    command
        .args(["-log", "-NewConsole"])
        .current_dir(exe.parent().unwrap_or(&root));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0000_0010);
    }
    let child = command
        .spawn()
        .map_err(|e| format!("Could not start the server: {e}"))?;
    *state
        .server
        .lock()
        .map_err(|_| "Server state lock failed")? = Some(child);
    Ok("Server started with -log -NewConsole.".into())
}

#[tauri::command]
fn stop_server(state: State<AppState>) -> Result<String, String> {
    let mut locked = state
        .server
        .lock()
        .map_err(|_| "Server state lock failed")?;
    if let Some(mut child) = locked.take() {
        child
            .kill()
            .map_err(|e| format!("Could not stop the server: {e}"))?;
        let _ = child.wait();
        Ok("Server stopped.".into())
    } else {
        Err("DragonWarden has no running server process to stop.".into())
    }
}

#[tauri::command]
fn check_network(install_dir: String, port: u16) -> Result<NetworkReport, String> {
    let root = root(&install_dir)?;
    let listening = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            &format!("[bool](Get-NetUDPEndpoint -LocalPort {port} -ErrorAction SilentlyContinue)"),
        ])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .eq_ignore_ascii_case("True")
        })
        .unwrap_or(false);
    let rule = Command::new("powershell.exe").args(["-NoProfile", "-Command", &format!("[bool](Get-NetFirewallRule -Enabled True -ErrorAction SilentlyContinue | Get-NetFirewallPortFilter | Where-Object {{ $_.Protocol -eq 'UDP' -and $_.LocalPort -eq '{port}' }})")]).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().eq_ignore_ascii_case("True")).unwrap_or(false);
    let public_ip = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "(Invoke-RestMethod -Uri 'https://api.ipify.org' -TimeoutSec 8)",
        ])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let _ = root;
    Ok(NetworkReport { port, listening, firewall_rule: rule, public_ip, external_verification: "UDP forwarding cannot be proven from the same home network. Forward UDP 7777 on your router to this PC, then have someone outside your network join or use a remote UDP testing service.".into() })
}

fn folder_fingerprint(source: &Path) -> Result<String, String> {
    let mut hasher = Sha256::new();
    for item in WalkDir::new(source).sort_by_file_name() {
        let item = item.map_err(|e| e.to_string())?;
        if item.file_type().is_file() {
            let meta = item.metadata().map_err(|e| e.to_string())?;
            hasher.update(
                item.path()
                    .strip_prefix(source)
                    .unwrap_or(item.path())
                    .to_string_lossy()
                    .as_bytes(),
            );
            hasher.update(meta.len().to_le_bytes());
            hasher.update(
                meta.modified()
                    .unwrap_or(SystemTime::UNIX_EPOCH)
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .to_le_bytes(),
            );
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}
fn copy_dir(source: &Path, target: &Path) -> Result<(), String> {
    for item in WalkDir::new(source) {
        let item = item.map_err(|e| e.to_string())?;
        let relative = item
            .path()
            .strip_prefix(source)
            .map_err(|e| e.to_string())?;
        let out = target.join(relative);
        if item.file_type().is_dir() {
            fs::create_dir_all(out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(item.path(), out).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
fn zip_directory(source: &Path, target: &Path) -> Result<(), String> {
    let file = fs::File::create(target).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for item in WalkDir::new(source) {
        let item = item.map_err(|e| e.to_string())?;
        if item.file_type().is_file() {
            let name = item
                .path()
                .strip_prefix(source)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            let mut input = fs::File::open(item.path()).map_err(|e| e.to_string())?;
            let mut bytes = Vec::new();
            input.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            zip.write_all(&bytes).map_err(|e| e.to_string())?;
        }
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(())
}
fn size(path: &Path) -> u64 {
    WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .filter_map(|x| x.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}
fn enforce_retention(backups: &Path, plan: &BackupPlan) -> Result<u64, String> {
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(
            plan.compress_after_hours.saturating_mul(3600),
        ))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for entry in fs::read_dir(backups).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir()
            && entry
                .metadata()
                .map_err(|e| e.to_string())?
                .modified()
                .unwrap_or(SystemTime::now())
                < cutoff
        {
            let zip = path.with_extension("zip");
            zip_directory(&path, &zip)?;
            fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
        }
    }
    let limit = plan.max_gib.saturating_mul(1024 * 1024 * 1024);
    let mut files: Vec<_> = fs::read_dir(backups)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|x| {
            (
                x.metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(SystemTime::UNIX_EPOCH),
                x.path(),
            )
        })
        .collect();
    files.sort_by_key(|x| x.0);
    let mut total: u64 = files.iter().map(|x| size(&x.1)).sum();
    for (_, path) in files {
        if total <= limit {
            break;
        }
        let removed = size(&path);
        if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        }
        .map_err(|e| e.to_string())?;
        total = total.saturating_sub(removed);
    }
    Ok(total)
}

#[tauri::command]
fn get_backup_plan(install_dir: String) -> Result<BackupPlan, String> {
    let p = plan_path(&root(&install_dir)?);
    Ok(fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default())
}
#[tauri::command]
fn set_backup_plan(install_dir: String, plan: BackupPlan) -> Result<(), String> {
    if plan.interval_minutes == 0 || plan.max_gib == 0 {
        return Err("Backup interval and maximum storage must be greater than zero.".into());
    }
    fs::write(
        plan_path(&root(&install_dir)?),
        serde_json::to_string_pretty(&plan).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
#[tauri::command]
fn run_backup(install_dir: String) -> Result<BackupResult, String> {
    let root = root(&install_dir)?;
    let source = saves_path(&root);
    if !source.exists() {
        return Err(
            "No SaveGames folder exists yet. Start the server and create a world first.".into(),
        );
    }
    let backups = backups_path(&root);
    fs::create_dir_all(&backups).map_err(|e| e.to_string())?;
    let fingerprint = folder_fingerprint(&source)?;
    let manifest = backups.join(".last-fingerprint");
    if fs::read_to_string(&manifest).ok().as_deref() == Some(&fingerprint) {
        let plan = get_backup_plan(install_dir)?;
        return Ok(BackupResult {
            changed: false,
            message: "No world changes since the last backup; no duplicate snapshot created."
                .into(),
            backup: None,
            total_bytes: enforce_retention(&backups, &plan)?,
        });
    }
    let name = format!("world-{}", Local::now().format("%Y-%m-%d_%H-%M-%S"));
    let target = backups.join(&name);
    copy_dir(&source, &target)?;
    fs::write(manifest, fingerprint).map_err(|e| e.to_string())?;
    let plan = get_backup_plan(install_dir)?;
    let total = enforce_retention(&backups, &plan)?;
    Ok(BackupResult {
        changed: true,
        message: format!("Created backup {name}."),
        backup: Some(target.display().to_string()),
        total_bytes: total,
    })
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_last_install_dir,
            remember_install_dir,
            inspect_server,
            install_server,
            read_config,
            save_config,
            start_server,
            stop_server,
            check_network,
            get_backup_plan,
            set_backup_plan,
            run_backup
        ])
        .run(tauri::generate_context!())
        .expect("error while running DragonWarden");
}
