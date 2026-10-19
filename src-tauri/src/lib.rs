mod addons;
mod appbar;
mod config;
mod system;

use addons::AddonInfo;
use config::{AddonEntry, Config};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Mutex, RwLock},
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, RunEvent, State, WebviewWindow,
};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt;

pub struct AppState {
    config: Mutex<Config>,
    /// 開いているパネルの幅（閉じているときは None）
    panel: Mutex<Option<u32>>,
    pub addons: RwLock<HashMap<String, AddonInfo>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitData {
    config: Config,
    addons: Vec<AddonInfo>,
    errors: Vec<String>,
    platform: &'static str,
}

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

/// 設定とパネルの開閉状態からウィンドウの位置・大きさを決め直す
fn apply_layout(app: &AppHandle) {
    let Some(window) = main_window(app) else {
        return;
    };
    let state = app.state::<AppState>();
    let layout = {
        let c = state.config.lock().unwrap();
        appbar::Layout {
            edge: c.edge,
            bar: c.bar_width,
            panel: state.panel.lock().unwrap().unwrap_or(0),
            reserve: c.reserve_space,
        }
    };
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        let raw = hwnd.0 as isize;
        // Win32 のウィンドウ操作は UI スレッドで行う
        let _ = app.run_on_main_thread(move || appbar::apply(raw, layout));
    }
    #[cfg(not(windows))]
    appbar::apply_fallback(&window, layout);
}

/// アドオンを読み直し、設定の並び順に新しいアドオンを足す
fn reload(app: &AppHandle) -> InitData {
    let (found, errors) = addons::scan(app);
    let state = app.state::<AppState>();
    let config = {
        let mut c = state.config.lock().unwrap();
        let mut ids: Vec<_> = found.keys().cloned().collect();
        ids.sort();
        // 同梱アドオンの初期の並び順
        let preferred = [
            "sidedock.shortcuts",
            "sidedock.search",
            "sidedock.volume",
            "sidedock.clock",
        ];
        ids.sort_by_key(|id| preferred.iter().position(|p| p == id).unwrap_or(usize::MAX));
        let mut changed = false;
        for id in ids {
            if !c.addons.iter().any(|a| a.id == id) {
                // 同梱は最初から有効。自分で入れたアドオンは権限を確認してから有効にしてもらう
                let enabled = found[&id].builtin;
                c.addons.push(AddonEntry { id, enabled });
                changed = true;
            }
        }
        if changed {
            let _ = config::save(app, &c);
        }
        c.clone()
    };
    let mut list: Vec<AddonInfo> = found.values().cloned().collect();
    list.sort_by(|a, b| a.id.cmp(&b.id));
    *state.addons.write().unwrap() = found;
    InitData {
        config,
        addons: list,
        errors,
        platform: if cfg!(windows) { "windows" } else { "other" },
    }
}

// =============================================================================
// 本体の UI から呼ぶコマンド
// =============================================================================

#[tauri::command]
fn init(app: AppHandle) -> InitData {
    reload(&app)
}

#[tauri::command]
fn save_config(app: AppHandle, state: State<AppState>, config: Config) -> Result<Config, String> {
    let config = config.sanitize();
    config::save(&app, &config)?;
    *state.config.lock().unwrap() = config.clone();
    apply_layout(&app);
    let _ = app.emit("config-changed", &config);
    Ok(config)
}

#[tauri::command]
fn set_panel(app: AppHandle, state: State<AppState>, open: bool, width: Option<u32>) {
    let w = {
        let default = state.config.lock().unwrap().panel_width;
        width.unwrap_or(default).clamp(240, 720)
    };
    *state.panel.lock().unwrap() = open.then_some(w);
    apply_layout(&app);
    if open {
        if let Some(win) = main_window(&app) {
            let _ = win.set_focus();
        }
    }
}

#[tauri::command]
fn open_addons_folder(app: AppHandle) -> Result<(), String> {
    let dir = addons::user_dir(&app)?;
    system::launch(&dir.to_string_lossy(), None)
}

#[tauri::command]
fn autostart_get(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn autostart_set(app: AppHandle, enabled: bool) -> Result<(), String> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(|e| e.to_string())
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

// =============================================================================
// アドオン API の中身（権限の確認は本体の UI 側の橋渡しで行う）
// =============================================================================

#[tauri::command]
fn storage_get(app: AppHandle, addon_id: String) -> Result<serde_json::Value, String> {
    addons::storage_get(&app, &addon_id)
}

#[tauri::command]
fn storage_set(app: AppHandle, addon_id: String, data: serde_json::Value) -> Result<(), String> {
    addons::storage_set(&app, &addon_id, &data)
}

#[tauri::command(async)]
fn app_launch(path: String, args: Option<String>) -> Result<(), String> {
    system::launch(&path, args.as_deref())
}

#[tauri::command(async)]
fn app_icon(path: String) -> Result<String, String> {
    system::icon_png(&path)
}

#[derive(serde::Deserialize)]
struct FileFilter {
    name: String,
    extensions: Vec<String>,
}

#[tauri::command(async)]
fn pick_files(app: AppHandle, filters: Option<Vec<FileFilter>>) -> Vec<String> {
    let mut dialog = app.dialog().file();
    if let Some(win) = main_window(&app) {
        dialog = dialog.set_parent(&win);
    }
    for f in filters.unwrap_or_default() {
        let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(&f.name, &exts);
    }
    dialog
        .blocking_pick_files()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

#[tauri::command(async)]
fn pick_folder(app: AppHandle) -> Option<String> {
    let mut dialog = app.dialog().file();
    if let Some(win) = main_window(&app) {
        dialog = dialog.set_parent(&win);
    }
    dialog
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command(async)]
fn browser_list() -> Vec<system::BrowserInfo> {
    system::browsers()
}

#[tauri::command(async)]
fn browser_open(browser_id: Option<String>, url: String) -> Result<(), String> {
    system::open_in_browser(browser_id.as_deref(), &url)
}

#[tauri::command(async)]
fn volume_get() -> Result<system::VolumeInfo, String> {
    system::volume_get()
}

#[tauri::command(async)]
fn volume_set(level: f32) -> Result<(), String> {
    system::volume_set(level)
}

#[tauri::command(async)]
fn volume_set_mute(muted: bool) -> Result<(), String> {
    system::volume_set_mute(muted)
}

// =============================================================================

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItem::with_id(app, "settings", "設定", true, None::<&str>)?;
    let folder = MenuItem::with_id(app, "folder", "アドオンフォルダを開く", true, None::<&str>)?;
    let reload = MenuItem::with_id(app, "reload", "再読み込み", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings, &folder, &reload, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("SideDock")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "settings" => {
                let _ = app.emit_to("main", "open-settings", ());
            }
            "folder" => {
                let _ = open_addons_folder(app.clone());
            }
            "reload" => {
                if let Some(w) = main_window(app) {
                    let _ = w.eval("location.reload()");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 左クリックで設定を開く
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = tray.app_handle().emit_to("main", "open-settings", ());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // 二重起動を防ぐ（2 つ目は設定を開くだけにする）
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = app.emit_to("main", "open-settings", ());
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .register_uri_scheme_protocol("addon", |ctx, request| {
            addons::handle_protocol(ctx.app_handle(), &request)
        })
        .setup(|app| {
            let handle = app.handle().clone();
            app.manage(AppState {
                config: Mutex::new(config::load(&handle)),
                panel: Mutex::new(None),
                addons: RwLock::new(HashMap::new()),
            });
            // 起動直後にアドオン一覧を用意しておく（addon:// の配信に必要）
            reload(&handle);
            build_tray(&handle)?;
            apply_layout(&handle);
            // 位置を決めてから表示する（apply_layout と同じキューに積むので順番が保たれる）
            let h = handle.clone();
            handle.run_on_main_thread(move || {
                if let Some(w) = main_window(&h) {
                    let _ = w.show();
                }
            })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            init,
            save_config,
            set_panel,
            open_addons_folder,
            autostart_get,
            autostart_set,
            quit,
            storage_get,
            storage_set,
            app_launch,
            app_icon,
            pick_files,
            pick_folder,
            browser_list,
            browser_open,
            volume_get,
            volume_set,
            volume_set_mute,
        ])
        .build(tauri::generate_context!())
        .expect("SideDock の起動に失敗しました");

    app.run(|_app, event| {
        if let RunEvent::Exit = event {
            #[cfg(windows)]
            appbar::unregister();
        }
    });
}
