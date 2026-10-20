//! アドオンの読み込み、addon:// プロトコルでの配信、アドオンごとの保存領域

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use tauri::{
    http::{header, Request, Response, StatusCode},
    AppHandle, Manager,
};

/// アドオンが要求できる権限
pub const KNOWN_PERMISSIONS: &[&str] = &[
    "activity",
    "app",
    "dialog",
    "drop",
    "browser",
    "volume",
    "network",
    "media",
    "system",
    "clipboard",
    "power",
];

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    /// アドオンフォルダ内の HTML ファイル
    pub entry: String,
    /// パネルの見出し（panel のみ）
    #[serde(default)]
    pub title: Option<String>,
    /// パネルの幅（論理px、panel のみ）
    #[serde(default)]
    pub width: Option<u32>,
    /// パネルの開き方（panel のみ）："hover"（近づけると開く、既定）か "manual"（アドオンが開く）
    #[serde(default)]
    pub trigger: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    id: String,
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    bar: Option<Slot>,
    #[serde(default)]
    panel: Option<Slot>,
    #[serde(default)]
    permissions: Vec<String>,
    /// 同梱アドオンを最初から有効にするか（省略時は有効）
    #[serde(default)]
    default_enabled: Option<bool>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AddonInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub icon: Option<String>,
    pub bar: Option<Slot>,
    pub panel: Option<Slot>,
    pub permissions: Vec<String>,
    /// 本体に同梱されたアドオンか
    pub builtin: bool,
    #[serde(skip)]
    pub default_enabled: bool,
    #[serde(skip)]
    pub dir: PathBuf,
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

/// 同梱アドオンのフォルダ（インストール先の addons）
pub fn builtin_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().resource_dir().ok().map(|d| d.join("addons"))
}

/// ユーザーが自分でアドオンを入れるフォルダ（%APPDATA%\com.xelaods.sidedock\addons）
pub fn user_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?
        .join("addons");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn read_manifest(dir: &Path, builtin: bool) -> Result<AddonInfo, String> {
    let text = fs::read_to_string(dir.join("manifest.json"))
        .map_err(|e| format!("{}: manifest.json を読めません ({e})", dir.display()))?;
    let m: Manifest = serde_json::from_str(&text)
        .map_err(|e| format!("{}: manifest.json の形式が不正です ({e})", dir.display()))?;
    if !valid_id(&m.id) {
        return Err(format!(
            "{}: id は英小文字・数字・. - _ だけで書いてください",
            dir.display()
        ));
    }
    if m.bar.is_none() && m.panel.is_none() {
        return Err(format!("{}: bar か panel のどちらかが必要です", m.id));
    }
    let permissions = m
        .permissions
        .into_iter()
        .filter(|p| KNOWN_PERMISSIONS.contains(&p.as_str()))
        .collect();
    Ok(AddonInfo {
        id: m.id,
        name: m.name,
        version: m.version,
        description: m.description,
        author: m.author,
        icon: m.icon,
        bar: m.bar,
        panel: m.panel,
        permissions,
        builtin,
        default_enabled: m.default_enabled.unwrap_or(true),
        dir: dir.to_path_buf(),
    })
}

/// 同梱フォルダ→ユーザーフォルダの順に読む。同じ id はユーザー側で上書きできる
pub fn scan(app: &AppHandle) -> (HashMap<String, AddonInfo>, Vec<String>) {
    let mut found = HashMap::new();
    let mut errors = Vec::new();
    let dirs = [(builtin_dir(app), true), (user_dir(app).ok(), false)];
    for (dir, builtin) in dirs {
        let Some(dir) = dir else { continue };
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        paths.sort();
        for p in paths {
            if !p.join("manifest.json").exists() {
                continue;
            }
            match read_manifest(&p, builtin) {
                Ok(info) => {
                    found.insert(info.id.clone(), info);
                }
                Err(e) => errors.push(e),
            }
        }
    }
    (found, errors)
}

// ---------------------------------------------------------------------------
// addon:// プロトコル
//   http://addon.localhost/<アドオンID>/<ファイル>   … アドオンのファイル
//   http://addon.localhost/_sdk/sidedock.js         … アドオン用 SDK
// ---------------------------------------------------------------------------

const SDK_JS: &[u8] = include_bytes!("../sdk/sidedock.js");
const SDK_CSS: &[u8] = include_bytes!("../sdk/sidedock.css");

fn mime_of(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn respond(status: StatusCode, mime: &str, csp: Option<&str>, body: Vec<u8>) -> Response<Vec<u8>> {
    let mut b = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        // サンドボックス化した iframe（オリジン null）から fetch できるように
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::CACHE_CONTROL, "no-cache");
    if let Some(csp) = csp {
        b = b.header("Content-Security-Policy", csp);
    }
    b.body(body).unwrap_or_else(|_| Response::new(Vec::new()))
}

fn not_found() -> Response<Vec<u8>> {
    respond(
        StatusCode::NOT_FOUND,
        "text/plain; charset=utf-8",
        None,
        b"not found".to_vec(),
    )
}

pub fn handle_protocol(app: &AppHandle, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let raw = request.uri().path();
    let decoded = percent_encoding::percent_decode_str(raw)
        .decode_utf8_lossy()
        .to_string();
    let mut parts = decoded.trim_start_matches('/').splitn(2, '/');
    let id = parts.next().unwrap_or_default();
    let rest = parts.next().unwrap_or_default();

    if id == "_sdk" {
        return match rest {
            "sidedock.js" => respond(
                StatusCode::OK,
                "text/javascript; charset=utf-8",
                None,
                SDK_JS.to_vec(),
            ),
            "sidedock.css" => respond(
                StatusCode::OK,
                "text/css; charset=utf-8",
                None,
                SDK_CSS.to_vec(),
            ),
            _ => not_found(),
        };
    }

    let Some(state) = app.try_state::<crate::AppState>() else {
        return not_found();
    };
    let addon = {
        let map = state.addons.read().unwrap();
        map.get(id).cloned()
    };
    let Some(addon) = addon else {
        return not_found();
    };

    // パストラバーサル対策：正規化したパスがアドオンフォルダの中にあるか確かめる
    let Ok(base) = addon.dir.canonicalize() else {
        return not_found();
    };
    let Ok(target) = base.join(rest).canonicalize() else {
        return not_found();
    };
    if !target.starts_with(&base) || !target.is_file() {
        return not_found();
    }
    let Ok(body) = fs::read(&target) else {
        return not_found();
    };

    let mime = mime_of(&target);
    let csp = if mime.starts_with("text/html") {
        // network 権限がないアドオンは外部と通信できない
        Some(if addon.permissions.iter().any(|p| p == "network") {
            "default-src * data: blob: 'unsafe-inline'"
        } else {
            "default-src http://addon.localhost addon: data: blob: 'unsafe-inline'"
        })
    } else {
        None
    };
    respond(StatusCode::OK, mime, csp, body)
}

// ---------------------------------------------------------------------------
// アドオンごとの保存領域（JSON 1 つ）
// ---------------------------------------------------------------------------

fn storage_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("不正なアドオンIDです".into());
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("addon-data");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join(format!("{id}.json")))
}

pub fn storage_get(app: &AppHandle, id: &str) -> Result<serde_json::Value, String> {
    let p = storage_path(app, id)?;
    match fs::read_to_string(&p) {
        Ok(s) => Ok(serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)),
        Err(_) => Ok(serde_json::Value::Null),
    }
}

pub fn storage_set(app: &AppHandle, id: &str, data: &serde_json::Value) -> Result<(), String> {
    let p = storage_path(app, id)?;
    let text = serde_json::to_string(data).map_err(|e| e.to_string())?;
    if text.len() > 5 * 1024 * 1024 {
        return Err("保存できるのは 5MB までです".into());
    }
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())
}
