use std::collections::HashMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcRecord {
    pub id: String,
    pub pid: u32,
    pub started_at: u64,
    pub bin: String,
    pub args: Vec<String>,
    pub log: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedState {
    processes: Vec<ProcRecord>,
}

/// 管理 rclone 子进程：启动、停止、存活判定，并把 pid 落盘，
/// 这样程序重开时能认领上次留下的挂载进程。
pub struct ProcessManager {
    live: Mutex<HashMap<String, Child>>,
    records: Mutex<HashMap<String, ProcRecord>>,
    state_file: PathBuf,
}

impl ProcessManager {
    pub fn new(state_file: PathBuf) -> Self {
        let manager = Self {
            live: Mutex::new(HashMap::new()),
            records: Mutex::new(HashMap::new()),
            state_file,
        };
        manager.load_state();
        manager
    }

    fn load_state(&self) {
        let Ok(text) = std::fs::read_to_string(&self.state_file) else {
            return;
        };
        let Ok(state) = serde_json::from_str::<PersistedState>(&text) else {
            return;
        };
        let mut records = self.records.lock();
        for record in state.processes {
            records.insert(record.id.clone(), record);
        }
    }

    fn persist(&self) {
        let processes = self.records.lock().values().cloned().collect();
        let state = PersistedState { processes };
        if let Ok(text) = serde_json::to_string_pretty(&state) {
            let _ = std::fs::write(&self.state_file, text);
        }
    }

    pub fn start(
        &self,
        id: &str,
        bin: &Path,
        args: &[String],
        log_path: &Path,
        envs: &[(String, String)],
    ) -> Result<ProcRecord, String> {
        self.stop(id);

        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("无法创建日志目录：{e}"))?;
        }
        rotate_log(log_path);

        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .map_err(|e| format!("无法打开挂载日志：{e}"))?;
        let err_log = log
            .try_clone()
            .map_err(|e| format!("无法复制日志句柄：{e}"))?;

        let mut cmd = Command::new(bin);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(err_log));
        hide_console(&mut cmd);
        for (key, value) in envs {
            cmd.env(key, value);
        }

        let child = cmd
            .spawn()
            .map_err(|e| format!("启动 rclone 失败：{e}"))?;

        let record = ProcRecord {
            id: id.to_string(),
            pid: child.id(),
            started_at: now_secs(),
            bin: bin.to_string_lossy().into_owned(),
            args: args.to_vec(),
            log: log_path.to_string_lossy().into_owned(),
        };

        self.live.lock().insert(id.to_string(), child);
        self.records.lock().insert(id.to_string(), record.clone());
        self.persist();

        crate::logfile::info(&format!(
            "启动 {} pid={} :: {} {}",
            id,
            record.pid,
            record.bin,
            record.args.join(" ")
        ));

        Ok(record)
    }

    pub fn stop(&self, id: &str) {
        let child = self.live.lock().remove(id);
        let record = self.records.lock().remove(id);

        match (child, record) {
            (Some(mut child), _) => {
                let _ = child.kill();
                let _ = child.wait();
            }
            (None, Some(record)) => {
                if pid_alive(record.pid) {
                    kill_pid(record.pid);
                    wait_until_gone(record.pid, Duration::from_millis(4000));
                }
            }
            (None, None) => {}
        }

        self.persist();
        crate::logfile::info(&format!("停止 {id}"));
    }

    pub fn is_running(&self, id: &str) -> bool {
        {
            let mut live = self.live.lock();
            if let Some(child) = live.get_mut(id) {
                if let Ok(None) = child.try_wait() {
                    return true;
                }
            }
        }

        let pid = match self.records.lock().get(id) {
            Some(record) => record.pid,
            None => return false,
        };

        pid_alive(pid) && process_is_rclone(pid)
    }

    pub fn pid(&self, id: &str) -> Option<u32> {
        {
            let live = self.live.lock();
            if let Some(child) = live.get(id) {
                return Some(child.id());
            }
        }
        self.records.lock().get(id).map(|record| record.pid)
    }

    pub fn records(&self) -> Vec<ProcRecord> {
        self.records.lock().values().cloned().collect()
    }

    /// 清掉已经不存在的进程记录。
    pub fn prune(&self) {
        let ids: Vec<String> = self.records.lock().keys().cloned().collect();
        let alive: Vec<String> = ids
            .into_iter()
            .filter(|id| self.is_running(id))
            .collect();
        {
            let mut records = self.records.lock();
            records.retain(|id, _| alive.iter().any(|kept| kept == id));
        }
        self.persist();
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn rotate_log(path: &Path) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    if meta.len() < MAX_LOG_BYTES {
        return;
    }
    let _ = std::fs::rename(path, path.with_extension("log.1"));
}

fn wait_until_gone(pid: u32, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !pid_alive(pid)
}

fn process_is_rclone(pid: u32) -> bool {
    match process_image_name(pid) {
        Some(name) => name
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or("")
            .to_lowercase()
            .starts_with("rclone"),
        // 拿不到进程名时以 pid 存活为准
        None => true,
    }
}

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
fn kill_pid(pid: u32) {
    let mut cmd = Command::new("taskkill");
    cmd.arg("/PID")
        .arg(pid.to_string())
        .arg("/T")
        .arg("/F");
    hide_console(&mut cmd);
    let _ = cmd.output();
}

#[cfg(unix)]
fn kill_pid(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
    if wait_until_gone(pid, Duration::from_millis(2500)) {
        return;
    }
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    const STILL_ACTIVE: u32 = 259;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut code);
        let _ = CloseHandle(handle);
        ok != 0 && code == STILL_ACTIVE
    }
}

#[cfg(windows)]
fn process_image_name(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
        let _ = CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buffer[..size as usize]))
    }
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(unix)]
fn process_image_name(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    let found = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|text| text.trim().to_string());
    #[cfg(not(target_os = "linux"))]
    let found = {
        let _ = pid;
        None::<String>
    };
    found
}