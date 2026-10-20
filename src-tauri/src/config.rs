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

/// 見た目の改訂の番号（0.9.0 のオブシディアンが 1、0.10.0 の EasyUI が 2）
const UI_REV: u32 = 2;

/// 画面の端でのジェスチャーと、それぞれに割り当てた操作
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Gestures {
    pub enabled: bool,
    /// 端に沿って右へ素早く払う
    pub swipe_right: String,
    /// 端に沿って左へ素早く払う
    pub swipe_left: String,
    /// 端で左右に小刻みに振る
    pub shake: String,
    /// 左の角に押し当てて少し止める
    pub corner_left: String,
    /// 右の角に押し当てて少し止める
    pub corner_right: String,
}

impl Default for Gestures {
    fn default() -> Self {
        Self {
            enabled: true,
            swipe_right: "media.next".into(),
            swipe_left: "media.prev".into(),
            shake: "media.toggle".into(),
            corner_left: "none".into(),
            corner_right: "none".into(),
        }
    }
}

impl Gestures {
    fn sanitize(mut self) -> Self {
        // 操作の名前は本体（main.js）が知っているものだけを実行するので、ここでは長さだけ見る
        for v in [
            &mut self.swipe_right,
            &mut self.swipe_left,
            &mut self.shake,
            &mut self.corner_left,
            &mut self.corner_right,
        ] {
            if v.is_empty() || v.len() > 40 {
                *v = "none".into();
            }
        }
        self
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// 画面の上端と下端のどちらに出すか："top" | "bottom"（タスクバーの位置）
    pub edge: String,
    /// 端のどこに出すか："left" | "center" | "right"
    pub position: String,
    /// 近づいたときに反応する範囲："narrow" | "normal" | "wide"
    pub trigger: String,
    /// "system" | "light" | "dark"
    pub theme: String,
    /// "hover"（近づけたときだけ出る）| "always"（常に表示）
    pub show: String,
    /// Windows のタスクバーを隠して、SideDock を代わりに使う
    pub hide_taskbar: bool,
    /// 見た目："easy" | "obsidian" | "glass" | "minimal" | "neon" | "soft" | "classic"
    pub skin: String,
    /// アクセントカラー："blue" | "violet" | "pink" | "orange" | "green" | "mono"
    pub accent: String,
    /// 見た目の改訂の番号。古い設定を新しい既定のデザインへ移すのに使う
    #[serde(default)]
    pub ui_rev: u32,
    /// 画面の端でのジェスチャー
    pub gestures: Gestures,
    /// アドオンの並び順と有効/無効
    pub addons: Vec<AddonEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            edge: "top".into(),
            position: "center".into(),
            trigger: "normal".into(),
            theme: "dark".into(),
            show: "hover".into(),
            hide_taskbar: false,
            skin: "easy".into(),
            accent: "mono".into(),
            ui_rev: UI_REV,
            gestures: Gestures::default(),
            addons: Vec::new(),
        }
    }
}

impl Config {
    /// 値の範囲を正す（手で config.json を書き換えた場合や、古い版の設定の保険）
    pub fn sanitize(mut self) -> Self {
        let d = Self::default();
        if !matches!(self.edge.as_str(), "top" | "bottom") {
            self.edge = d.edge.clone();
        }
        if !matches!(self.position.as_str(), "left" | "center" | "right") {
            self.position = d.position;
        }
        if !matches!(self.trigger.as_str(), "narrow" | "normal" | "wide") {
            self.trigger = d.trigger;
        }
        if !matches!(self.theme.as_str(), "system" | "light" | "dark") {
            self.theme = d.theme;
        }
        if !matches!(self.show.as_str(), "hover" | "always") {
            self.show = d.show;
        }
        // 見た目を刷新したら、それより前の設定は一度だけ新しい既定のデザインへ切り替える
        if self.ui_rev < UI_REV {
            self.ui_rev = UI_REV;
            self.skin = "easy".into();
            self.accent = "mono".into();
        }
        if !matches!(self.skin.as_str(), "easy" | "obsidian" | "glass" | "minimal" | "neon" | "soft" | "classic") {
            self.skin = d.skin;
        }
        if !matches!(self.accent.as_str(), "blue" | "violet" | "pink" | "orange" | "green" | "mono") {
            self.accent = d.accent;
        }
        self.gestures = self.gestures.sanitize();
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
