//! クリップボードの履歴（文字だけ）。
//! パスワード管理アプリなどが「履歴に残さないで」と印を付けたものは記録しない。
//! 履歴は %APPDATA%\com.xelaods.sidedock\clipboard.json に保存する

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};

const MAX_ITEMS: usize = 60;
const MAX_LEN: usize = 20_000;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: String,
    pub text: String,
    /// 記録した時刻（UNIX ミリ秒）
    pub at: u64,
    #[serde(default)]
    pub pinned: bool,
}

static ITEMS: Mutex<Vec<Clip>> = Mutex::new(Vec::new());
static STARTED: AtomicBool = AtomicBool::new(false);
/// 自分で書き込んだ直後は、それを新しい項目として数えない
static LAST_SELF: Mutex<Option<String>> = Mutex::new(None);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_config_dir().ok()?;
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join("clipboard.json"))
}

fn save(app: &AppHandle, items: &[Clip]) {
    if let (Some(p), Ok(json)) = (file(app), serde_json::to_string(items)) {
        let tmp = p.with_extension("json.tmp");
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(tmp, p);
        }
    }
}

fn add(app: &AppHandle, text: String) {
    let text = text.trim_end_matches(['\r', '\n']).to_string();
    if text.trim().is_empty() || text.len() > MAX_LEN {
        return;
    }
    let mut items = ITEMS.lock().unwrap();
    // 同じ文字列があれば先頭へ移す（ピン留めは保つ）
    let pinned = items
        .iter()
        .find(|c| c.text == text)
        .map(|c| c.pinned)
        .unwrap_or(false);
    items.retain(|c| c.text != text);
    items.insert(
        0,
        Clip {
            id: format!("{:x}", now_ms() ^ (text.len() as u64) << 40),
            text,
            at: now_ms(),
            pinned,
        },
    );
    // ピン留めしていないものから古い順に消す
    while items.len() > MAX_ITEMS {
        if let Some(i) = items.iter().rposition(|c| !c.pinned) {
            items.remove(i);
        } else {
            break;
        }
    }
    save(app, &items);
    drop(items);
    let _ = app.emit_to("main", "clipboard-changed", ());
}

/// 監視を始める（何度呼んでも 1 回だけ）
pub fn start(app: AppHandle) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(p) = file(&app) {
        if let Ok(s) = fs::read_to_string(p) {
            if let Ok(v) = serde_json::from_str::<Vec<Clip>>(&s) {
                *ITEMS.lock().unwrap() = v;
            }
        }
    }
    thread::spawn(move || {
        let mut last_seq = sequence();
        let mut board = arboard::Clipboard::new().ok();
        loop {
            thread::sleep(Duration::from_millis(500));
            let seq = sequence();
            if seq == last_seq {
                continue;
            }
            last_seq = seq;
            if excluded() {
                continue;
            }
            if board.is_none() {
                board = arboard::Clipboard::new().ok();
            }
            let Some(b) = board.as_mut() else { continue };
            let Ok(text) = b.get_text() else { continue };
            if LAST_SELF.lock().unwrap().take().as_deref() == Some(text.as_str()) {
                continue;
            }
            add(&app, text);
        }
    });
}

pub fn list() -> Vec<Clip> {
    let items = ITEMS.lock().unwrap();
    // ピン留めを先に
    let mut v: Vec<Clip> = items.iter().filter(|c| c.pinned).cloned().collect();
    v.extend(items.iter().filter(|c| !c.pinned).cloned());
    v
}

pub fn copy(app: &AppHandle, id: &str) -> Result<(), String> {
    let text = ITEMS
        .lock()
        .unwrap()
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.text.clone())
        .ok_or("見つかりません")?;
    *LAST_SELF.lock().unwrap() = Some(text.clone());
    arboard::Clipboard::new()
        .and_then(|mut b| b.set_text(text.clone()))
        .map_err(|e| e.to_string())?;
    // コピーし直したものは先頭へ
    add(app, text);
    Ok(())
}

pub fn remove(app: &AppHandle, id: &str) {
    let mut items = ITEMS.lock().unwrap();
    items.retain(|c| c.id != id);
    save(app, &items);
}

pub fn pin(app: &AppHandle, id: &str, pinned: bool) {
    let mut items = ITEMS.lock().unwrap();
    if let Some(c) = items.iter_mut().find(|c| c.id == id) {
        c.pinned = pinned;
    }
    save(app, &items);
}

/// ピン留め以外を消す
pub fn clear(app: &AppHandle) {
    let mut items = ITEMS.lock().unwrap();
    items.retain(|c| c.pinned);
    save(app, &items);
}

#[cfg(windows)]
fn sequence() -> u32 {
    unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() }
}
#[cfg(not(windows))]
fn sequence() -> u32 {
    0
}

/// パスワード管理アプリなどが付ける「履歴・監視の対象外」の印があるか
#[cfg(windows)]
fn excluded() -> bool {
    use windows::{
        core::w,
        Win32::System::DataExchange::{IsClipboardFormatAvailable, RegisterClipboardFormatW},
    };
    unsafe {
        for name in [
            w!("ExcludeClipboardContentFromMonitorProcessing"),
            w!("Clipboard Viewer Ignore"),
            w!("CanIncludeInClipboardHistory"),
        ] {
            let f = RegisterClipboardFormatW(name);
            if f != 0 && IsClipboardFormatAvailable(f).is_ok() {
                return true;
            }
        }
    }
    false
}
#[cfg(not(windows))]
fn excluded() -> bool {
    false
}
