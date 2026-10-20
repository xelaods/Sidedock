mod addons;
mod clipboard;
mod config;
mod log;
mod media;
mod overlay;
mod sysmon;
mod system;
mod taskbar;

use addons::AddonInfo;
use config::{AddonEntry, Config};
use overlay::Overlay;
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
    pub addons: RwLock<HashMap<String, AddonInfo>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitData {
    config: Config,
    addons: Vec<AddonInfo>,
    errors: Vec<String>,
    platform: &'static str,
    suspended: bool,
}

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
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
            "sidedock.taskbar",
            "sidedock.clock",
            "sidedock.media",
            "sidedock.shortcuts",
            "sidedock.search",
            "sidedock.clipboard",
            "sidedock.notes",
            "sidedock.timer",
            "sidedock.volume",
            "sidedock.tray",
        ];
        ids.sort_by_key(|id| preferred.iter().position(|p| p == id).unwrap_or(usize::MAX));
        let mut changed = false;
        for id in ids {
            if !c.addons.iter().any(|a| a.id == id) {
                // 同梱は最初から有効。自分で入れたアドオンは権限を確認してから有効にしてもらう
                let enabled = found[&id].builtin && found[&id].default_enabled;
                // 決まった並び順があるものは、その位置に差し込む（後から増えた同梱アドオン向け）
                let rank = |x: &str| preferred.iter().position(|p| *p == x);
                let at = match rank(&id) {
                    Some(r) => c
                        .addons
                        .iter()
                        .position(|a| rank(&a.id).is_none_or(|ar| ar > r))
                        .unwrap_or(c.addons.len()),
                    None => c.addons.len(),
                };
                c.addons.insert(at, AddonEntry { id, enabled });
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
        suspended: app.state::<Overlay>().is_suspended(),
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
    taskbar::set_hidden(config.hide_taskbar);
    *state.config.lock().unwrap() = config.clone();
    let _ = app.emit("config-changed", &config);
    Ok(config)
}

/// UI 側からのログ（起動・出し入れ・エラー）
#[tauri::command]
fn log_ui(app: AppHandle, message: String) {
    log::write(
        &app,
        &format!("UI: {}", message.chars().take(500).collect::<String>()),
    );
}

/// カーソルを受け取る範囲（カプセルと開いているパネル）
#[tauri::command]
fn set_hit_rects(overlay: State<Overlay>, rects: Vec<overlay::Rect>) {
    overlay.set_rects(rects);
}

/// 検索パネルなど、キーボード入力が必要なときにフォーカスをもらう
#[tauri::command]
fn take_focus(app: AppHandle) {
    if let Some(w) = main_window(&app) {
        let _ = w.set_focus();
    }
}

/// 隠れるときに、直前に使っていたアプリへフォーカスを返す
#[tauri::command]
fn release_focus(app: AppHandle) {
    overlay::release_focus(&app);
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
    app.dialog()
        .file()
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

#[tauri::command(async)]
fn media_get() -> Result<Option<media::MediaInfo>, String> {
    media::get()
}

#[tauri::command(async)]
fn media_art() -> Result<Option<String>, String> {
    media::art()
}

#[tauri::command(async)]
fn media_control(action: String) -> Result<(), String> {
    media::control(&action)
}

#[tauri::command(async)]
fn media_seek(seconds: f64) -> Result<(), String> {
    media::seek(seconds)
}

#[tauri::command(async)]
fn system_stats() -> sysmon::Stats {
    sysmon::stats()
}

#[tauri::command(async)]
fn power_action(action: String) -> Result<(), String> {
    sysmon::power(&action)
}

#[tauri::command]
fn clipboard_watch(app: AppHandle) {
    clipboard::start(app);
}

#[tauri::command]
fn clipboard_list(app: AppHandle) -> Vec<clipboard::Clip> {
    clipboard::start(app);
    clipboard::list()
}

#[tauri::command(async)]
fn clipboard_copy(app: AppHandle, id: String) -> Result<(), String> {
    clipboard::copy(&app, &id)
}

#[tauri::command]
fn clipboard_remove(app: AppHandle, id: String) {
    clipboard::remove(&app, &id);
}

#[tauri::command]
fn clipboard_pin(app: AppHandle, id: String, pinned: bool) {
    clipboard::pin(&app, &id, pinned);
}

#[tauri::command]
fn clipboard_clear(app: AppHandle) {
    clipboard::clear(&app);
}

#[tauri::command(async)]
fn windows_list(app: AppHandle) -> Vec<taskbar::WindowInfo> {
    taskbar::list(overlay::last_foreground(&app))
}

#[tauri::command(async)]
fn windows_icon(id: i64, exe: String) -> Result<String, String> {
    taskbar::icon(id, &exe)
}

#[tauri::command(async)]
fn windows_activate(app: AppHandle, id: i64) -> Result<(), String> {
    taskbar::activate(id, overlay::last_foreground(&app))
}

#[tauri::command(async)]
fn windows_focus(id: i64) {
    taskbar::focus(id);
}

#[tauri::command(async)]
fn windows_close(id: i64) {
    taskbar::close(id);
}

#[tauri::command(async)]
fn shell_action(action: String) -> Result<(), String> {
    taskbar::shell(&action)
}

/// 隠れている間は WebView2 にメモリを手放してもらう（軽量化）
#[tauri::command]
fn set_low_memory(app: AppHandle, low: bool) {
    #[cfg(windows)]
    if let Some(w) = main_window(&app) {
        let _ = w.with_webview(move |wv| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
                COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
            };
            use windows::core::Interface;
            if let Ok(core) = wv.controller().CoreWebView2() {
                if let Ok(v19) = core.cast::<ICoreWebView2_19>() {
                    let _ = v19.SetMemoryUsageTargetLevel(if low {
                        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
                    } else {
                        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
                    });
                }
            }
        });
    }
    #[cfg(not(windows))]
    let _ = (app, low);
}

// =============================================================================

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItem::with_id(app, "settings", "設定", true, None::<&str>)?;
    let folder = MenuItem::with_id(app, "folder", "アドオンフォルダを開く", true, None::<&str>)?;
    let logs = MenuItem::with_id(app, "log", "診断ログを開く", true, None::<&str>)?;
    let reload = MenuItem::with_id(app, "reload", "再読み込み", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings, &folder, &logs, &reload, &sep, &quit])?;

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
            "log" => {
                if let Some(p) = log::path(app) {
                    let _ = system::launch(&p.to_string_lossy(), None);
                }
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
            log::reset(&handle);
            log::write(
                &handle,
                &format!("SideDock {} を起動", env!("CARGO_PKG_VERSION")),
            );
            app.manage(AppState {
                config: Mutex::new(config::load(&handle)),
                addons: RwLock::new(HashMap::new()),
            });
            app.manage(Overlay::new());
            // 起動直後にアドオン一覧を用意しておく（addon:// の配信に必要）
            reload(&handle);
            build_tray(&handle)?;

            if let Some(w) = main_window(&handle) {
                // ふだんはクリックを下のアプリへ素通しする
                let _ = w.set_ignore_cursor_events(true);
            }
            // 設定に合わせてタスクバーを隠す（オフなら、前回落ちて隠れたままのものも戻す）
            let hide_taskbar = handle
                .state::<AppState>()
                .config
                .lock()
                .unwrap()
                .hide_taskbar;
            taskbar::set_hidden(hide_taskbar);
            overlay::layout(&handle);
            overlay::show(&handle);
            overlay::start(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            init,
            save_config,
            set_hit_rects,
            log_ui,
            take_focus,
            release_focus,
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
            media_get,
            media_art,
            media_control,
            media_seek,
            system_stats,
            power_action,
            clipboard_watch,
            clipboard_list,
            clipboard_copy,
            clipboard_remove,
            clipboard_pin,
            clipboard_clear,
            windows_list,
            windows_icon,
            windows_activate,
            windows_focus,
            windows_close,
            shell_action,
            set_low_memory,
        ])
        .build(tauri::generate_context!())
        .expect("SideDock の起動に失敗しました");

    app.run(|_app, event: RunEvent| {
        // 終了するときは、隠していた Windows のタスクバーを必ず元に戻す
        if let RunEvent::Exit = event {
            taskbar::restore();
        }
    });
}
