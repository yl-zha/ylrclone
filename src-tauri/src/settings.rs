use std::path::Path;

use serde::{Deserialize, Serialize};

/// 一个 WebDAV 挂载项的持久化配置（不含密码，密码只存在 rclone.conf 里且已 obscure）。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RemoteSpec {
    pub name: String,
    pub url: String,
    pub vendor: String,
    pub user: String,
    pub mount_point: String,
    pub sub_path: String,
    pub extra_flags: Vec<String>,
    pub auto_mount: bool,
    pub network_mode: bool,
}

impl RemoteSpec {
    /// 去掉空行和由勾选框统一管理的 --network-mode 参数。
    pub fn normalize_flags(&mut self) {
        let flags: Vec<String> = self
            .extra_flags
            .iter()
            .map(|flag| flag.trim().to_string())
            .filter(|flag| !flag.is_empty())
            .filter(|flag| flag != "--network-mode" && !flag.starts_with("--network-mode="))
            .collect();
        self.extra_flags = flags;
    }

    pub fn remote_path(&self) -> String {
        let sub = self.sub_path.trim().trim_start_matches('/').to_string();
        format!("{}:{}", self.name, sub)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub remotes: Vec<RemoteSpec>,
    /// 空字符串表示自动查找。
    pub rclone_path: String,
    pub auto_remount: bool,
    pub autostart: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            remotes: Vec::new(),
            rclone_path: String::new(),
            auto_remount: true,
            autostart: false,
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                crate::logfile::warn(&format!("settings.json 解析失败，改用默认配置：{e}"));
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("无法创建配置目录：{e}"))?;
        }
        let text =
            serde_json::to_string_pretty(self).map_err(|e| format!("配置序列化失败：{e}"))?;
        std::fs::write(path, text).map_err(|e| format!("保存配置失败：{e}"))
    }

    pub fn find(&self, name: &str) -> Option<&RemoteSpec> {
        self.remotes.iter().find(|remote| remote.name == name)
    }

    pub fn find_mut(&mut self, name: &str) -> Option<&mut RemoteSpec> {
        self.remotes.iter_mut().find(|remote| remote.name == name)
    }

    pub fn upsert(&mut self, spec: RemoteSpec) {
        match self.find_mut(&spec.name) {
            Some(existing) => *existing = spec,
            None => self.remotes.push(spec),
        }
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.remotes.len();
        self.remotes.retain(|remote| remote.name != name);
        self.remotes.len() != before
    }
}