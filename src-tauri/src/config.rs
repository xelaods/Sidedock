//! 設定ファイル（%APPDATA%\com.xelaods.sidedock\config.json）の読み書き

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AddonEntry {
    pub id: String,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// 画面上部のどこに出すか："left" | "center" | "right"
    pub position: String,
    /// 近づいたときに反応する範囲："narrow" | "normal" | "wide"
    pub trigger: String,
    /// "system" | "light" | "dark"
    pub theme: String,
    /// アドオンの並び順と有効/無効
    pub addons: Vec<AddonEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            position: "center".into(),
            trigger: "normal".into(),
            theme: "dark".into(),
            addons: Vec::new(),
        }
    }
}

impl Config {
    /// 値の範囲を正す（手で config.json を書き換えた場合や、古い版の設定の保険）
    pub fn sanitize(mut self) -> Self {
        let d = Self::default();
        if !matches!(self.position.as_str(), "left" | "center" | "right") {
            self.position = d.position;
        }
        if !matches!(self.trigger.as_str(), "narrow" | "normal" | "wide") {
            self.trigger = d.trigger;
        }
        if !matches!(self.theme.as_str(), "system" | "light" | "dark") {
            self.theme = d.theme;
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
