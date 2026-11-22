use std::path::PathBuf;
use std::process::Command;

use tauri::AppHandle;

fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE_NAME: &str = "ylrclone";

#[cfg(windows)]
pub fn is_enabled(_app: &AppHandle) -> bool {
    let mut cmd = Command::new("reg");
    cmd.args(["query", RUN_KEY, "/v", VALUE_NAME]);
    hide_console(&mut cmd);
    cmd.output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
pub fn set_enabled(_app: &AppHandle, enable: bool) -> Result<bool, String> {
    if enable {
        let exe = std::env::current_exe().map_err(|e| format!("无法定位程序路径：{e}"))?;
        let mut cmd = Command::new("reg");
        cmd.args(["add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d"])
            .arg(exe.to_string_lossy().into_owned())
            .arg("/f");
        hide_console(&mut cmd);
        let output = cmd
            .output()
            .map_err(|e| format!("写入开机自启失败：{e}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
    } else {
        let mut cmd = Command::new("reg");
        cmd.args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"]);
        hide_console(&mut cmd);
        let _ = cmd.output();
    }
    Ok(enable)
}

#[cfg(target_os = "linux")]
fn desktop_file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/autostart/ylrclone.desktop"))
}

#[cfg(target_os = "linux")]
pub fn is_enabled(_app: &AppHandle) -> bool {
    desktop_file()
        .map(|path| path.is_file())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
pub fn set_enabled(_app: &AppHandle, enable: bool) -> Result<bool, String> {
    let Some(path) = desktop_file() else {
        return Err("找不到 HOME 目录".to_string());
    };
    if enable {
        let exe = std::env::current_exe().map_err(|e| format!("无法定位程序路径：{e}"))?;
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let content = format!(
            "[Desktop Entry]\nType=Application\nName=ylrclone\nExec={}\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        std::fs::write(&path, content).map_err(|e| format!("写入自启文件失败：{e}"))?;
    } else {
        let _ = std::fs::remove_file(&path);
    }
    Ok(enable)
}

#[cfg(target_os = "macos")]
pub fn is_enabled(_app: &AppHandle) -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn set_enabled(_app: &AppHandle, _enable: bool) -> Result<bool, String> {
    Err("macOS 暂不支持开机自启".to_string())
}