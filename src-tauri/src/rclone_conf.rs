use std::path::Path;
use std::process::Command;

/// rclone.conf 的极简编辑器：只替换/删除目标段，其它段原样保留。
#[derive(Debug, Default)]
pub struct ConfFile {
    text: String,
}

fn header_name(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('[') && trimmed.ends_with(']') {
        Some(&trimmed[1..trimmed.len() - 1])
    } else {
        None
    }
}

impl ConfFile {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("读取 rclone.conf 失败：{e}"))?;
        Ok(Self { text })
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("无法创建配置目录：{e}"))?;
        }
        std::fs::write(path, &self.text).map_err(|e| format!("写入 rclone.conf 失败：{e}"))
    }

    fn lines(&self) -> Vec<String> {
        self.text.lines().map(|line| line.to_string()).collect()
    }

    fn find_range(lines: &[String], section: &str) -> Option<(usize, usize)> {
        let start = lines.iter().position(|line| {
            header_name(line)
                .map(|name| name.trim() == section)
                .unwrap_or(false)
        })?;
        let mut end = lines.len();
        for (index, line) in lines.iter().enumerate().skip(start + 1) {
            if header_name(line).is_some() {
                end = index;
                break;
            }
        }
        Some((start, end))
    }

    pub fn section(&self, section: &str) -> Vec<(String, String)> {
        let lines = self.lines();
        let Some((start, end)) = Self::find_range(&lines, section) else {
            return Vec::new();
        };
        let mut pairs = Vec::new();
        for line in &lines[start + 1..end] {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                pairs.push((key.trim().to_string(), value.trim().to_string()));
            }
        }
        pairs
    }

    pub fn get(&self, section: &str, key: &str) -> Option<String> {
        self.section(section)
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, value)| value)
    }

    pub fn upsert(&mut self, section: &str, pairs: &[(String, String)]) {
        let mut lines = self.lines();
        let mut block = vec![format!("[{section}]")];
        for (key, value) in pairs {
            block.push(format!("{key} = {value}"));
        }

        match Self::find_range(&lines, section) {
            Some((start, end)) => {
                lines.splice(start..end, block);
            }
            None => {
                if lines
                    .last()
                    .map(|line| !line.trim().is_empty())
                    .unwrap_or(false)
                {
                    lines.push(String::new());
                }
                lines.extend(block);
            }
        }

        self.text = serialize(&lines);
    }

    pub fn remove(&mut self, section: &str) -> bool {
        let mut lines = self.lines();
        let Some((start, end)) = Self::find_range(&lines, section) else {
            return false;
        };
        lines.drain(start..end);

        let mut cleaned: Vec<String> = Vec::new();
        for line in lines {
            if line.trim().is_empty()
                && cleaned
                    .last()
                    .map(|previous| previous.trim().is_empty())
                    .unwrap_or(true)
            {
                continue;
            }
            cleaned.push(line);
        }
        while cleaned
            .last()
            .map(|line| line.trim().is_empty())
            .unwrap_or(false)
        {
            cleaned.pop();
        }

        self.text = serialize(&cleaned);
        true
    }
}

fn serialize(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
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

pub fn obscure(bin: &Path, password: &str) -> Result<String, String> {
    if password.is_empty() {
        return Ok(String::new());
    }
    if !bin.is_file() {
        return Err(format!("找不到 rclone：{}", bin.display()));
    }
    let mut cmd = Command::new(bin);
    cmd.arg("obscure").arg(password);
    hide_console(&mut cmd);
    let output = cmd
        .output()
        .map_err(|e| format!("调用 rclone obscure 失败：{e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("rclone obscure 失败：{stderr}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}