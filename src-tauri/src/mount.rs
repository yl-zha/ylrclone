use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::paths;
use crate::process::ProcessManager;
use crate::rclone_conf::ConfFile;
use crate::settings::{AppSettings, RemoteSpec};
use crate::winfsp;

/// 同一时刻只允许一个挂载点探测线程，避免网络挂死时线程越堆越多。
static PROBE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MountStatus {
    pub name: String,
    pub state: String,
    pub pid: Option<u32>,
    pub mount_point: String,
    pub error: Option<String>,
}

impl MountStatus {
    pub fn unmounted(name: &str, mount_point: &str) -> Self {
        Self {
            name: name.to_string(),
            state: "unmounted".to_string(),
            pid: None,
            mount_point: mount_point.to_string(),
            error: None,
        }
    }
}

pub fn process_id(name: &str) -> String {
    format!("mount-{name}")
}

pub fn build_args(spec: &RemoteSpec, conf: &Path) -> Vec<String> {
    let mut args = vec![
        "mount".to_string(),
        "--config".to_string(),
        conf.to_string_lossy().into_owned(),
        spec.remote_path(),
        spec.mount_point.clone(),
    ];

    let has_cache_mode = spec
        .extra_flags
        .iter()
        .any(|flag| flag == "--vfs-cache-mode" || flag.starts_with("--vfs-cache-mode="));
    if !has_cache_mode {
        args.push("--vfs-cache-mode=writes".to_string());
    }

    if cfg!(windows) && spec.network_mode {
        args.push("--network-mode".to_string());
    }

    args.extend(spec.extra_flags.iter().cloned());
    args
}

#[cfg(windows)]
fn is_drive_letter(value: &str) -> bool {
    let trimmed = value.trim_end_matches('\\');
    trimmed.len() == 2
        && trimmed.ends_with(':')
        && trimmed
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic())
            .unwrap_or(false)
}

/// Windows 盘符必须空闲；目录挂载点不存在就建出来。
pub fn ensure_mount_point(mount_point: &str) -> Result<(), String> {
    let target = mount_point.trim();
    if target.is_empty() {
        return Err("挂载点为空".to_string());
    }

    #[cfg(windows)]
    {
        if is_drive_letter(target) {
            let letter = target.chars().next().unwrap_or('X');
            let root = format!("{letter}:\\");
            if Path::new(&root).exists() {
                return Err(format!("盘符 {letter}: 已被占用，换一个吧"));
            }
            return Ok(());
        }
    }

    let path = Path::new(target);
    if !path.exists() {
        std::fs::create_dir_all(path).map_err(|e| format!("无法创建挂载点 {target}：{e}"))?;
    }
    Ok(())
}

fn read_dir_once(mount_point: &str) -> Result<(), String> {
    let path = Path::new(mount_point);
    if !path.exists() {
        return Err("挂载点不存在".to_string());
    }
    let mut entries = std::fs::read_dir(path).map_err(|e| format!("读取挂载点失败：{e}"))?;
    match entries.next() {
        Some(Ok(_)) => Ok(()),
        Some(Err(e)) => Err(format!("读取挂载点失败：{e}")),
        None => Ok(()),
    }
}

pub fn probe(mount_point: &str, timeout: Duration) -> Result<(), String> {
    if PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("上一次探测还没返回，挂载点可能已经失联".to_string());
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let target = mount_point.to_string();
    std::thread::spawn(move || {
        let result = read_dir_once(&target);
        let _ = tx.send(result);
        PROBE_IN_FLIGHT.store(false, Ordering::SeqCst);
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(_) => Err("挂载点无响应（超时）".to_string()),
    }
}

pub fn read_log_tail(path: &Path, lines: usize) -> String {
    let Ok(text) = std::fs::read_to_string(path) else {
        return String::new();
    };
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}

pub fn status_of(pm: &ProcessManager, spec: &RemoteSpec) -> MountStatus {
    let id = process_id(&spec.name);
    if !pm.is_running(&id) {
        return MountStatus::unmounted(&spec.name, &spec.mount_point);
    }

    match probe(&spec.mount_point, Duration::from_secs(2)) {
        Ok(()) => MountStatus {
            name: spec.name.clone(),
            state: "mounted".to_string(),
            pid: pm.pid(&id),
            mount_point: spec.mount_point.clone(),
            error: None,
        },
        Err(reason) => MountStatus {
            name: spec.name.clone(),
            state: "error".to_string(),
            pid: pm.pid(&id),
            mount_point: spec.mount_point.clone(),
            error: Some(reason),
        },
    }
}

pub fn unmount(pm: &ProcessManager, spec: &RemoteSpec) -> MountStatus {
    pm.stop(&process_id(&spec.name));
    MountStatus::unmounted(&spec.name, &spec.mount_point)
}

pub fn mount(
    app: &AppHandle,
    pm: &ProcessManager,
    settings: &AppSettings,
    spec: &RemoteSpec,
) -> Result<MountStatus, String> {
    if winfsp::required() && !winfsp::installed() {
        return Err(
            "未检测到 WinFsp：Windows 上挂载必须先装它（https://winfsp.dev/rel/），装完重开本程序"
                .to_string(),
        );
    }

    let bin = paths::resolve_rclone(app, &settings.rclone_path)?;
    let conf = paths::rclone_conf_path(app)?;

    if ConfFile::load(&conf)?.get(&spec.name, "type").is_none() {
        return Err(format!(
            "rclone.conf 里没有 [{}] 段，请先保存这个挂载项",
            spec.name
        ));
    }

    ensure_mount_point(&spec.mount_point)?;

    let args = build_args(spec, &conf);
    let id = process_id(&spec.name);
    let log_path = paths::mount_log_path(app, &spec.name)?;

    pm.start(&id, &bin, &args, &log_path, &[])?;

    // rclone 遇到密码错、路径不存在这类问题会秒退，先看它稳不稳
    let mut ticks = 0;
    while ticks < 20 {
        std::thread::sleep(Duration::from_millis(250));
        ticks += 1;
        if !pm.is_running(&id) {
            break;
        }
    }

    if !pm.is_running(&id) {
        let tail = read_log_tail(&log_path, 15);
        return Err(if tail.trim().is_empty() {
            "rclone 启动后立刻退出，日志是空的".to_string()
        } else {
            format!("rclone 启动后立刻退出，日志末尾：\n{}", tail.trim())
        });
    }

    match probe(&spec.mount_point, Duration::from_secs(8)) {
        Ok(()) => Ok(MountStatus {
            name: spec.name.clone(),
            state: "mounted".to_string(),
            pid: pm.pid(&id),
            mount_point: spec.mount_point.clone(),
            error: None,
        }),
        Err(reason) => Ok(MountStatus {
            name: spec.name.clone(),
            state: "error".to_string(),
            pid: pm.pid(&id),
            mount_point: spec.mount_point.clone(),
            error: Some(format!("进程在跑，但挂载点暂时读不出来：{reason}")),
        }),
    }
}