use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录：{e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建应用数据目录：{e}"))?;
    Ok(dir)
}

pub fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("settings.json"))
}

pub fn rclone_conf_path(app: &AppHandle) -> Result<PathBuf, String> {
    let path = data_dir(app)?.join("rclone.conf");
    if !path.exists() {
        std::fs::write(&path, "").map_err(|e| format!("无法创建 rclone.conf：{e}"))?;
    }
    Ok(path)
}

pub fn logs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建日志目录：{e}"))?;
    Ok(dir)
}

pub fn process_state_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("process_state.json"))
}

pub fn mount_log_path(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    Ok(logs_dir(app)?.join(format!("mount-{name}.log")))
}

fn exe_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.parent().map(|p| p.to_path_buf())
}

fn looks_like_rclone(path: &Path) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let name = name.to_string_lossy().to_lowercase();
    #[cfg(windows)]
    {
        name.starts_with("rclone") && name.ends_with(".exe")
    }
    #[cfg(not(windows))]
    {
        // sidecar 可能是 rclone-x86_64-unknown-linux-gnu，没有扩展名
        name.starts_with("rclone") && !name.contains('.')
    }
}

fn find_binary_in(dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && looks_like_rclone(&path) {
            return Some(path);
        }
    }
    None
}

fn find_in_path() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "rclone.exe" } else { "rclone" };
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(exe_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 依次找：自定义路径 → 程序目录 → bin/、resources/ 子目录 → 数据目录 bin/ → PATH。
pub fn resolve_rclone(app: &AppHandle, custom: &str) -> Result<PathBuf, String> {
    let custom = custom.trim();
    if !custom.is_empty() {
        let path = PathBuf::from(custom);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!("自定义 rclone 路径不存在：{}", path.display()));
    }

    if let Some(dir) = exe_dir() {
        if let Some(path) = find_binary_in(&dir) {
            return Ok(path);
        }
        for sub in ["bin", "resources"] {
            if let Some(path) = find_binary_in(&dir.join(sub)) {
                return Ok(path);
            }
        }
    }

    if let Ok(dir) = data_dir(app) {
        if let Some(path) = find_binary_in(&dir.join("bin")) {
            return Ok(path);
        }
    }

    if let Some(path) = find_in_path() {
        return Ok(path);
    }

    Err("找不到 rclone：请把 rclone.exe 放到程序目录，或在「数据目录」里改 settings.json 的 rclonePath".to_string())
}

pub fn rclone_version(bin: &Path) -> Option<String> {
    let mut cmd = std::process::Command::new(bin);
    cmd.arg("version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next()?.trim().to_string();
    Some(first.split_whitespace().nth(1).unwrap_or(&first).to_string())
}