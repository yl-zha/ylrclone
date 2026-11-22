use std::path::Path;

pub fn required() -> bool {
    cfg!(windows)
}

#[cfg(windows)]
pub fn installed() -> bool {
    const CANDIDATES: [&str; 3] = [
        r"C:\Program Files (x86)\WinFsp\bin\winfsp-x64.dll",
        r"C:\Program Files\WinFsp\bin\winfsp-x64.dll",
        r"C:\Program Files (x86)\WinFsp\bin\winfsp-a64.dll",
    ];
    CANDIDATES
        .iter()
        .any(|candidate| Path::new(candidate).is_file())
}

#[cfg(not(windows))]
pub fn installed() -> bool {
    false
}