mod autostart;
mod logfile;
mod mount;
mod paths;
mod process;
mod rclone_conf;
mod settings;
mod tray;
mod winfsp;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

use mount::MountStatus;
use process::ProcessManager;
use settings::{AppSettings, RemoteSpec};

pub struct AppState {
    pub settings: Mutex<AppSettings>,
    pub pm: Arc<ProcessManager>,
    pub statuses: Mutex<HashMap<String, MountStatus>>,
    pub rclone_info: Mutex<Option<(String, String)>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    remotes: Vec<RemoteSpec>,
    statuses: Vec<MountStatus>,
    rclone_version: String,
    rclone_path: String,
    rclone_found: bool,
    winfsp_ok: bool,
    winfsp_required: bool,
    autostart: bool,
    auto_remount: bool,
    data_dir: String,
    platform: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveRemoteInput {
    spec: RemoteSpec,
    #[serde(default)]
    pass: Option<String>,
}

fn platform_name() -> String {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
    .to_string()
}

fn settings_view(app: &AppHandle, state: &AppState) -> Result<SettingsView, String> {
    let settings = state.settings.lock().clone();

    let mut statuses: Vec<MountStatus> = state.statuses.lock().values().cloned().collect();
    statuses.sort_by(|a, b| a.name.cmp(&b.name));

    let mut cached = state.rclone_info.lock();
    if cached.is_none() {
        if let Ok(bin) = paths::resolve_rclone(app, &settings.rclone_path) {
            let version = paths::rclone_version(&bin).unwrap_or_else(|| "?".to_string());
            *cached = Some((bin.to_string_lossy().into_owned(), version));
        }
    }
    let (rclone_path, rclone_version, rclone_found) = match cached.as_ref() {
        Some((path, version)) => (path.clone(), version.clone(), true),
        None => (String::new(), String::new(), false),
    };
    drop(cached);

    Ok(SettingsView {
        remotes: settings.remotes.clone(),
        statuses,
        rclone_version,
        rclone_path,
        rclone_found,
        winfsp_ok: winfsp::installed(),
        winfsp_required: winfsp::required(),
        autostart: autostart::is_enabled(app),
        auto_remount: settings.auto_remount,
        data_dir: paths::data_dir(app)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        platform: platform_name(),
    })
}

#[tauri::command]
fn get_settings(app: AppHandle, state: State<'_, AppState>) -> Result<SettingsView, String> {
    settings_view(&app, &state)
}

#[tauri::command]
fn save_remote(
    app: AppHandle,
    state: State<'_, AppState>,
    input: SaveRemoteInput,
) -> Result<RemoteSpec, String> {
    let mut spec = input.spec;
    spec.name = spec.name.trim().to_string();
    spec.url = spec.url.trim().to_string();
    spec.vendor = spec.vendor.trim().to_string();
    spec.user = spec.user.trim().to_string();
    spec.mount_point = spec.mount_point.trim().to_string();
    spec.sub_path = spec.sub_path.trim().to_string();
    spec.normalize_flags();

    if spec.name.is_empty() {
        return Err("名称不能为空".to_string());
    }
    if spec.name.contains([':', '/', '\\']) {
        return Err("名称不能包含 : / \\ 这三个字符".to_string());
    }
    if spec.url.is_empty() {
        return Err("WebDAV 地址不能为空".to_string());
    }
    if spec.mount_point.is_empty() {
        return Err("挂载点不能为空".to_string());
    }

    let mut settings = state.settings.lock().clone();

    let pass_value: Option<String> = match input.pass {
        Some(password) if !password.is_empty() => {
            let bin = paths::resolve_rclone(&app, &settings.rclone_path)?;
            Some(rclone_conf::obscure(&bin, &password)?)
        }
        _ => None,
    };

    let conf_path = paths::rclone_conf_path(&app)?;
    let mut conf = rclone_conf::ConfFile::load(&conf_path)?;

    let mut pairs: Vec<(String, String)> = vec![
        ("type".to_string(), "webdav".to_string()),
        ("url".to_string(), spec.url.clone()),
        ("user".to_string(), spec.user.clone()),
    ];
    if !spec.vendor.is_empty() {
        pairs.push(("vendor".to_string(), spec.vendor.clone()));
    }
    match pass_value {
        Some(value) => pairs.push(("pass".to_string(), value)),
        None => {
            if let Some(previous) = conf.get(&spec.name, "pass") {
                pairs.push(("pass".to_string(), previous));
            }
        }
    }
    conf.upsert(&spec.name, &pairs);
    conf.save(&conf_path)?;

    settings.upsert(spec.clone());
    settings.save(&paths::settings_path(&app)?)?;
    *state.settings.lock() = settings;

    logfile::info(&format!("保存挂载项 {}", spec.name));
    let _ = tray::refresh(&app);
    Ok(spec)
}

#[tauri::command]
fn delete_remote(app: AppHandle, state: State<'_, AppState>, name: String) -> Result<(), String> {
    let mut settings = state.settings.lock().clone();

    if let Some(spec) = settings.find(&name).cloned() {
        mount::unmount(&state.pm, &spec);
    }

    let conf_path = paths::rclone_conf_path(&app)?;
    let mut conf = rclone_conf::ConfFile::load(&conf_path)?;
    conf.remove(&name);
    conf.save(&conf_path)?;

    let removed = settings.remove(&name);
    settings.save(&paths::settings_path(&app)?)?;
    *state.settings.lock() = settings;
    state.statuses.lock().remove(&name);

    if removed {
        logfile::info(&format!("删除挂载项 {name}"));
    }
    let _ = tray::refresh(&app);
    Ok(())
}

#[tauri::command]
async fn mount_remote(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<MountStatus, String> {
    let (spec, settings_snapshot, pm) = {
        let settings = state.settings.lock().clone();
        let spec = settings
            .find(&name)
            .cloned()
            .ok_or_else(|| format!("没有名为 {name} 的挂载项"))?;
        (spec, settings, Arc::clone(&state.pm))
    };

    let app_for_task = app.clone();
    let status = tauri::async_runtime::spawn_blocking(move || {
        mount::mount(&app_for_task, &pm, &settings_snapshot, &spec)
    })
    .await
    .map_err(|e| format!("挂载任务异常：{e}"))??;

    {
        let mut statuses = state.statuses.lock();
        statuses.insert(status.name.clone(), status.clone());
    }
    let _ = tray::refresh(&app);
    let _ = app.emit("status-changed", ());
    Ok(status)
}

#[tauri::command]
async fn unmount_remote(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<MountStatus, String> {
    let (spec, pm) = {
        let settings = state.settings.lock().clone();
        let spec = settings
            .find(&name)
            .cloned()
            .ok_or_else(|| format!("没有名为 {name} 的挂载项"))?;
        (spec, Arc::clone(&state.pm))
    };

    let status =
        tauri::async_runtime::spawn_blocking(move || mount::unmount(&pm, &spec))
            .await
            .map_err(|e| format!("卸载任务异常：{e}"))?;

    {
        let mut statuses = state.statuses.lock();
        statuses.insert(status.name.clone(), status.clone());
    }
    let _ = tray::refresh(&app);
    let _ = app.emit("status-changed", ());
    Ok(status)
}

#[tauri::command]
fn list_statuses(state: State<'_, AppState>) -> Result<Vec<MountStatus>, String> {
    let mut statuses: Vec<MountStatus> = state.statuses.lock().values().cloned().collect();
    statuses.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(statuses)
}

#[tauri::command]
fn read_log(app: AppHandle, name: String, lines: Option<usize>) -> Result<String, String> {
    let path = paths::mount_log_path(&app, &name)?;
    let tail = mount::read_log_tail(&path, lines.unwrap_or(200));
    if tail.is_empty() {
        Ok(format!("（{} 还没有日志）", path.display()))
    } else {
        Ok(tail)
    }
}

#[tauri::command]
fn set_autostart(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<SettingsView, String> {
    autostart::set_enabled(&app, enabled)?;

    let mut settings = state.settings.lock().clone();
    settings.autostart = enabled;
    settings.save(&paths::settings_path(&app)?)?;
    *state.settings.lock() = settings;

    settings_view(&app, &state)
}

#[tauri::command]
fn set_auto_remount(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<SettingsView, String> {
    let mut settings = state.settings.lock().clone();
    settings.auto_remount = enabled;
    settings.save(&paths::settings_path(&app)?)?;
    *state.settings.lock() = settings;

    settings_view(&app, &state)
}

#[tauri::command]
fn open_data_dir(app: AppHandle) -> Result<(), String> {
    let dir = paths::data_dir(&app)?;
    open_path_internal(&dir)
}

#[tauri::command]
fn hide_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| format!("隐藏窗口失败：{e}"))?;
    }
    Ok(())
}

fn open_path_internal(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开目录失败：{e}"))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开目录失败：{e}"))?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开目录失败：{e}"))?;
        Ok(())
    }
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "open" => show_main_window(app),
        "quit" => quit_app(app),
        "none" => {}
        other if other.starts_with("toggle:") => {
            let name = other["toggle:".len()..].to_string();
            toggle_from_tray(app, name);
        }
        _ => {}
    }
}

fn toggle_from_tray(app: &AppHandle, name: String) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let spec = match state.settings.lock().find(&name).cloned() {
            Some(spec) => spec,
            None => return,
        };

        let current = mount::status_of(&state.pm, &spec);
        let next = if current.state == "mounted" || current.state == "error" {
            mount::unmount(&state.pm, &spec)
        } else {
            let settings = state.settings.lock().clone();
            match mount::mount(&app, &state.pm, &settings, &spec) {
                Ok(status) => status,
                Err(err) => {
                    logfile::error(&format!("托盘挂载 {} 失败：{err}", spec.name));
                    MountStatus {
                        name: spec.name.clone(),
                        state: "error".to_string(),
                        pid: None,
                        mount_point: spec.mount_point.clone(),
                        error: Some(err),
                    }
                }
            }
        };

        state.statuses.lock().insert(name.clone(), next);
        let _ = tray::refresh(&app);
        let _ = app.emit("status-changed", ());
    });
}

pub fn quit_app(app: &AppHandle) {
    let state = app.state::<AppState>();
    for record in state.pm.records() {
        state.pm.stop(&record.id);
    }
    logfile::info("退出 ylrclone");
    app.exit(0);
}

fn refresh_all_statuses(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().clone();
    let mut results = Vec::new();
    for spec in &settings.remotes {
        results.push(mount::status_of(&state.pm, spec));
    }
    let mut statuses = state.statuses.lock();
    for status in results {
        statuses.insert(status.name.clone(), status);
    }
}

fn spawn_auto_mount(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        let state = app.state::<AppState>();
        let settings = state.settings.lock().clone();
        for spec in settings.remotes.iter().filter(|remote| remote.auto_mount) {
            let id = mount::process_id(&spec.name);
            if state.pm.is_running(&id) {
                continue;
            }
            let status = match mount::mount(&app, &state.pm, &settings, spec) {
                Ok(status) => status,
                Err(err) => {
                    logfile::error(&format!("开机自动挂载 {} 失败：{err}", spec.name));
                    MountStatus {
                        name: spec.name.clone(),
                        state: "error".to_string(),
                        pid: None,
                        mount_point: spec.mount_point.clone(),
                        error: Some(err),
                    }
                }
            };
            state.statuses.lock().insert(spec.name.clone(), status);
        }
        let _ = tray::refresh(&app);
        let _ = app.emit("status-changed", ());
    });
}

fn spawn_watchdog(app: AppHandle) {
    std::thread::spawn(move || {
        let mut failures: HashMap<String, u32> = HashMap::new();

        loop {
            std::thread::sleep(Duration::from_secs(6));

            let state = app.state::<AppState>();
            let settings = state.settings.lock().clone();
            let auto_remount = settings.auto_remount;
            let mut results: Vec<MountStatus> = Vec::new();

            for spec in &settings.remotes {
                let id = mount::process_id(&spec.name);
                if state.pm.is_running(&id) {
                    failures.remove(&spec.name);
                    results.push(mount::status_of(&state.pm, spec));
                    continue;
                }

                let attempts = failures.get(&spec.name).copied().unwrap_or(0);
                if auto_remount && spec.auto_mount && attempts < 3 {
                    match mount::mount(&app, &state.pm, &settings, spec) {
                        Ok(status) if status.state == "mounted" => {
                            logfile::info(&format!("看门狗自动重挂 {} 成功", spec.name));
                            failures.remove(&spec.name);
                            results.push(status);
                        }
                        Ok(status) => {
                            failures.insert(spec.name.clone(), attempts + 1);
                            results.push(status);
                        }
                        Err(err) => {
                            logfile::warn(&format!("看门狗重挂 {} 失败：{err}", spec.name));
                            failures.insert(spec.name.clone(), attempts + 1);
                            results.push(MountStatus {
                                name: spec.name.clone(),
                                state: "error".to_string(),
                                pid: None,
                                mount_point: spec.mount_point.clone(),
                                error: Some(err),
                            });
                        }
                    }
                } else {
                    results.push(MountStatus::unmounted(&spec.name, &spec.mount_point));
                }
            }

            let mut changed = false;
            {
                let mut statuses = state.statuses.lock();
                for status in &results {
                    let previous = statuses.get(&status.name).map(|old| old.state.clone());
                    if previous.as_deref() != Some(status.state.as_str()) {
                        changed = true;
                    }
                    statuses.insert(status.name.clone(), status.clone());
                }
                let known: Vec<String> = statuses.keys().cloned().collect();
                for name in known {
                    if !settings.remotes.iter().any(|remote| remote.name == name) {
                        statuses.remove(&name);
                        changed = true;
                    }
                }
            }

            if changed {
                let _ = tray::refresh(&app);
                let _ = app.emit("status-changed", ());
            }
        }
    });
}

fn setup_app(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    let data_dir = paths::data_dir(&handle).unwrap_or_else(|_| PathBuf::from("."));
    let logs_dir = paths::logs_dir(&handle).unwrap_or_else(|_| data_dir.join("logs"));
    let _ = logfile::init(&logs_dir);

    let settings_path =
        paths::settings_path(&handle).unwrap_or_else(|_| data_dir.join("settings.json"));
    let mut settings = AppSettings::load(&settings_path);
    settings.autostart = autostart::is_enabled(&handle);
    if let Err(err) = settings.save(&settings_path) {
        logfile::warn(&format!("保存设置失败：{err}"));
    }

    let state_file =
        paths::process_state_path(&handle).unwrap_or_else(|_| data_dir.join("process_state.json"));
    let pm = Arc::new(ProcessManager::new(state_file));

    app.manage(AppState {
        settings: Mutex::new(settings),
        pm,
        statuses: Mutex::new(HashMap::new()),
        rclone_info: Mutex::new(None),
    });

    if let Err(err) = tray::create(&handle) {
        logfile::error(&format!("托盘初始化失败：{err}"));
    }

    refresh_all_statuses(&handle);
    spawn_auto_mount(handle.clone());
    spawn_watchdog(handle.clone());

    logfile::info("ylrclone 启动完成");
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_remote,
            delete_remote,
            mount_remote,
            unmount_remote,
            list_statuses,
            read_log,
            set_autostart,
            set_auto_remount,
            open_data_dir,
            hide_window,
        ])
        .setup(|app| {
            setup_app(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("启动 ylrclone 失败");
}