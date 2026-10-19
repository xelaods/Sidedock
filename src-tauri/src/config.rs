//! 設定ファイル（%APPDATA%\com.xelaods.sidedock\config.json）の読み書き

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Left,
    Right,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AddonEntry {
    pub id: String,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// 画面のどちら側に置くか
    pub edge: Edge,
    /// バーの幅（論理px）
    pub bar_width: u32,
    /// パネルの既定の幅（論理px）
    pub panel_width: u32,
    /// AppBar として画面の領域を確保する（最大化したウィンドウと重ならない）
    pub reserve_space: bool,
    /// "system" | "light" | "dark"
    pub theme: String,
    /// アドオンの並び順と有効/無効
    pub addons: Vec<AddonEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            edge: Edge::Right,
            bar_width: 52,
            panel_width: 340,
            reserve_space: true,
            theme: "system".into(),
            addons: Vec::new(),
        }
    }
}

impl Config {
    /// 値の範囲を正す（手で config.json を書き換えた場合の保険）
    pub fn sanitize(mut self) -> Self {
        self.bar_width = self.bar_width.clamp(40, 96);
        self.panel_width = self.panel_width.clamp(240, 720);
        if !matches!(self.theme.as_str(), "system" | "light" | "dark") {
            self.theme = "system".into();
        }
        let mut seen = std::collections::HashSet::new();
        self.addons.retain(|a| seen.insert(a.id.clone()));
        self
    }
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("config.json"))
}

pub fn load(app: &AppHandle) -> Config {
    path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<Config>(&s).ok())
        .unwrap_or_default()
        .sanitize()
}

pub fn save(app: &AppHandle, config: &Config) -> Result<(), String> {
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let p = path(app)?;
    // 書き込み途中で落ちても壊れないよう、一時ファイル経由で置き換える
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())
}
