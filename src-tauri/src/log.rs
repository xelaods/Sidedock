//! 不具合を調べるための簡単なログ（%APPDATA%\com.xelaods.sidedock\sidedock.log）
//! 起動のたびに作り直し、大きくなりすぎたら頭から書き直す

use std::{fs::OpenOptions, io::Write, path::PathBuf, sync::Mutex, time::SystemTime};
use tauri::{AppHandle, Manager};

static LOCK: Mutex<()> = Mutex::new(());

pub fn path(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_config_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("sidedock.log"))
}

pub fn reset(app: &AppHandle) {
    if let Some(p) = path(app) {
        let _ = std::fs::write(p, "");
    }
}

pub fn write(app: &AppHandle, msg: &str) {
    let Some(p) = path(app) else { return };
    let _guard = LOCK.lock();
    if std::fs::metadata(&p).map(|m| m.len() > 512 * 1024).unwrap_or(false) {
        let _ = std::fs::write(&p, "");
    }
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "[{secs:.3}] {msg}");
    }
}
