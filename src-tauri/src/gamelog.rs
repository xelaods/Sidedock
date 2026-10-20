//! 全画面で遊んだゲームの記録。
//! 前面のアプリが画面全体を覆っている間、そのアプリ（exe）ごとに時間を数える。
//! ブラウザや動画プレイヤーの全画面は数えない。
//! 記録は %APPDATA%\com.xelaods.sidedock\gamelog.json に保存する。

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// 1 回のプレイ
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    pub exe: String,
    /// 始めた時刻と終えた時刻（UNIX ミリ秒）
    pub start: u64,
    pub end: u64,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Log {
    /// 日付（YYYY-MM-DD、PC の時刻）→ exe → 秒
    pub days: BTreeMap<String, BTreeMap<String, u64>>,
    /// exe → 表示名（遊び始めたときのウィンドウのタイトル）
    pub names: BTreeMap<String, String>,
    /// 新しいものが後ろ。1 分未満のものは残さない
    pub sessions: Vec<Session>,
}

/// 今遊んでいるもの
#[derive(Serialize, Clone, Debug)]
pub struct Current {
    pub exe: String,
    pub name: String,
    pub secs: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Snapshot {
    #[serde(flatten)]
    pub log: Log,
    pub today: String,
    pub current: Option<Current>,
}

struct Running {
    exe: String,
    start: u64,
    /// 日付の欄へまだ足していない端数（ミリ秒）
    carry_ms: u64,
}

struct State {
    path: Option<PathBuf>,
    log: Log,
    running: Option<Running>,
    last_tick: Option<Instant>,
    dirty: bool,
    last_save: Instant,
    loaded: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// ゲームとして数えないアプリ（exe の名前、小文字・拡張子なし）
const NOT_GAMES: &[&str] = &[
    // ブラウザ
    "chrome", "msedge", "firefox", "brave", "opera", "vivaldi", "arc", "iexplore", "msedgewebview2",
    "whale", "thorium", "librewolf", "waterfox", "floorp", "zen",
    // 動画・音楽
    "vlc", "mpc-hc", "mpc-hc64", "mpc-be", "mpc-be64", "potplayer", "potplayermini", "potplayermini64",
    "wmplayer", "video.ui", "microsoft.media.player", "mpv", "kodi", "plex", "spotify", "applemusic",
    "itunes", "kanade", "netflix", "primevideo", "disneyplus",
    // Windows 自身や、仕事のアプリ
    "applicationframehost", "explorer", "sidedock", "lockapp", "searchhost", "searchapp",
    "startmenuexperiencehost", "shellexperiencehost", "textinputhost", "powerpnt", "zoom", "teams",
    "ms-teams", "acrord32", "acrobat", "photos", "microsoft.photos", "snippingtool", "screenclippinghost",
];

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// PC の時刻での今日の日付
#[cfg(windows)]
pub fn today() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}
#[cfg(not(windows))]
pub fn today() -> String {
    let days = now_ms() / 86_400_000;
    // 1970-01-01 からの日数を年月日に（UTC。Windows 以外は動作確認用）
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

/// exe のパスから、比べるための短い名前（小文字・拡張子なし）
fn stem(exe: &str) -> String {
    std::path::Path::new(exe)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn is_game(exe: &str) -> bool {
    let s = stem(exe);
    !s.is_empty() && !NOT_GAMES.contains(&s.as_str()) && !exe.to_lowercase().ends_with(".scr")
}

pub fn init(app: &tauri::AppHandle) {
    use tauri::Manager;
    let path = app
        .path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("gamelog.json"));
    let mut g = STATE.lock().unwrap();
    *g = Some(State {
        path,
        log: Log::default(),
        running: None,
        last_tick: None,
        dirty: false,
        last_save: Instant::now(),
        loaded: false,
    });
}

fn ensure_loaded(s: &mut State) {
    if s.loaded {
        return;
    }
    s.loaded = true;
    if let Some(p) = &s.path {
        if let Ok(text) = std::fs::read_to_string(p) {
            if let Ok(log) = serde_json::from_str::<Log>(&text) {
                s.log = log;
            }
        }
    }
}

fn save(s: &mut State) {
    s.dirty = false;
    s.last_save = Instant::now();
    // 古い日付は 400 日で消す
    while s.log.days.len() > 400 {
        let first = s.log.days.keys().next().cloned();
        if let Some(k) = first {
            s.log.days.remove(&k);
        }
    }
    let excess = s.log.sessions.len().saturating_sub(300);
    if excess > 0 {
        s.log.sessions.drain(..excess);
    }
    let Some(p) = &s.path else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(&s.log) {
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, p);
        }
    }
}

fn finish(s: &mut State) {
    if let Some(r) = s.running.take() {
        let end = now_ms();
        if end.saturating_sub(r.start) >= 60_000 {
            s.log.sessions.push(Session {
                exe: r.exe,
                start: r.start,
                end,
            });
        }
        s.dirty = true;
    }
}

/// 約 4 回/秒、カーソル監視のスレッドから呼ばれる。
/// playing：全画面で前面にあるアプリの exe と、そのウィンドウのタイトル
pub fn tick(playing: Option<(String, String)>) {
    let mut g = STATE.lock().unwrap();
    let Some(s) = g.as_mut() else { return };
    let now = Instant::now();
    let dt = s.last_tick.map(|t| now - t).unwrap_or_default();
    s.last_tick = Some(now);
    // スリープから戻ったときなど、間が空きすぎたら数えない
    let dt = if dt > Duration::from_secs(5) { Duration::ZERO } else { dt };

    let playing = playing.filter(|(exe, _)| is_game(exe));
    match playing {
        Some((exe, title)) => {
            ensure_loaded(s);
            if s.running.as_ref().map(|r| r.exe != exe).unwrap_or(true) {
                finish(s);
                let name = title.trim();
                if !name.is_empty() && name.chars().count() <= 60 {
                    s.log.names.insert(exe.clone(), name.to_string());
                } else if !s.log.names.contains_key(&exe) {
                    s.log.names.insert(exe.clone(), stem(&exe));
                }
                s.running = Some(Running {
                    exe: exe.clone(),
                    start: now_ms(),
                    carry_ms: 0,
                });
            }
            let r = s.running.as_mut().unwrap();
            r.carry_ms += dt.as_millis() as u64;
            if r.carry_ms >= 1000 {
                let secs = r.carry_ms / 1000;
                r.carry_ms %= 1000;
                *s.log
                    .days
                    .entry(today())
                    .or_default()
                    .entry(exe)
                    .or_insert(0) += secs;
                s.dirty = true;
            }
        }
        None => {
            if s.running.is_some() {
                finish(s);
                save(s);
            }
        }
    }
    if s.dirty && s.last_save.elapsed() > Duration::from_secs(60) {
        save(s);
    }
}

/// 終了するときに、遊んでいる途中のものも書き残す
pub fn flush() {
    let mut g = STATE.lock().unwrap();
    if let Some(s) = g.as_mut() {
        if s.loaded {
            finish(s);
            save(s);
        }
    }
}

pub fn snapshot() -> Snapshot {
    let mut g = STATE.lock().unwrap();
    let Some(s) = g.as_mut() else {
        return Snapshot {
            log: Log::default(),
            today: today(),
            current: None,
        };
    };
    ensure_loaded(s);
    let current = s.running.as_ref().map(|r| Current {
        exe: r.exe.clone(),
        name: s.log.names.get(&r.exe).cloned().unwrap_or_else(|| stem(&r.exe)),
        secs: now_ms().saturating_sub(r.start) / 1000,
    });
    Snapshot {
        log: s.log.clone(),
        today: today(),
        current,
    }
}

/// 記録を消す。exe を指定するとそのアプリの分だけ
pub fn clear(exe: Option<&str>) {
    let mut g = STATE.lock().unwrap();
    let Some(s) = g.as_mut() else { return };
    ensure_loaded(s);
    match exe {
        Some(exe) => {
            for d in s.log.days.values_mut() {
                d.remove(exe);
            }
            s.log.days.retain(|_, d| !d.is_empty());
            s.log.sessions.retain(|x| x.exe != exe);
            s.log.names.remove(exe);
        }
        None => s.log = Log::default(),
    }
    save(s);
}
