//! 画面上部に重ねて表示するオーバーレイの制御。
//!
//! - ウィンドウは画面上部の全幅を覆う透明な窓で、画面の作業領域は一切変えない
//! - ふだんはクリックを下のアプリへ素通しし、カプセルやパネルの上にカーソルがある間だけ受け取る
//! - カーソルの位置を約 60 回/秒で調べ、画面上部に近いときだけ UI に知らせる（近づくと出てくる動き）
//! - 全画面のアプリ（動画・ゲーム・F11 のブラウザなど）が前面にある間は丸ごと隠す

use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicIsize, Ordering},
        Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize};

/// ウィンドウの高さ（論理px）。カプセル＋開いたパネル＋影が収まる大きさ
pub const WINDOW_HEIGHT: f64 = 520.0;
/// ウィンドウの下端からこの距離（論理px）までカーソルの位置を UI に送る
const NEAR_MARGIN: f64 = 140.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Geom {
    /// ウィンドウ左上（物理px、仮想スクリーン座標）
    x: i32,
    y: i32,
    /// ウィンドウの大きさ（物理px）
    w: u32,
    h: u32,
    scale: f64,
}

/// UI が「ここは操作できる場所」と知らせてくる矩形（ウィンドウ内の論理px）
#[derive(Deserialize, Clone, Copy, Debug)]
pub struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

#[derive(Serialize, Clone, Copy, PartialEq)]
struct Cursor {
    x: f64,
    y: f64,
}

pub struct Overlay {
    geom: Mutex<Geom>,
    rects: Mutex<Vec<Rect>>,
    suspended: AtomicBool,
    /// 自分以外で最後に前面にあったウィンドウ（パネルを閉じたらフォーカスを返す）
    last_foreground: AtomicIsize,
}

impl Overlay {
    pub fn new() -> Self {
        Self {
            geom: Mutex::new(Geom::default()),
            rects: Mutex::new(Vec::new()),
            suspended: AtomicBool::new(false),
            last_foreground: AtomicIsize::new(0),
        }
    }

    pub fn set_rects(&self, rects: Vec<Rect>) {
        *self.rects.lock().unwrap() = rects;
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended.load(Ordering::SeqCst)
    }
}

/// メインディスプレイの上端（作業領域の上端）に、全幅でウィンドウを合わせる。
/// 位置が変わったら true
pub fn layout(app: &AppHandle) -> bool {
    let Some(win) = app.get_webview_window("main") else {
        return false;
    };
    let Ok(Some(mon)) = win.primary_monitor() else {
        return false;
    };
    let scale = mon.scale_factor();
    let wa = mon.work_area();
    let next = Geom {
        x: wa.position.x,
        y: wa.position.y,
        w: wa.size.width,
        h: (WINDOW_HEIGHT * scale).round() as u32,
        scale,
    };
    let ov = app.state::<Overlay>();
    let mut g = ov.geom.lock().unwrap();
    if *g == next {
        return false;
    }
    *g = next;
    drop(g);
    let _ = win.set_size(PhysicalSize::new(next.w, next.h));
    let _ = win.set_position(PhysicalPosition::new(next.x, next.y));
    true
}

/// 前面に出さずに表示する（フォーカスを奪わない）
pub fn show(app: &AppHandle) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    #[cfg(windows)]
    if let Ok(h) = win.hwnd() {
        let raw = h.0 as isize;
        let _ = app.run_on_main_thread(move || win::show_no_activate(raw));
        return;
    }
    let _ = win.show();
}

fn hide(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
}

/// 自分が前面にいたら、直前に使っていたウィンドウへフォーカスを返す
pub fn release_focus(app: &AppHandle) {
    #[cfg(windows)]
    if let Some(w) = app.get_webview_window("main") {
        if let Ok(h) = w.hwnd() {
            let own = h.0 as isize;
            let prev = app
                .state::<Overlay>()
                .last_foreground
                .load(Ordering::SeqCst);
            win::restore_focus(own, prev);
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}

/// カーソル監視のスレッドを始める
pub fn start(app: AppHandle) {
    thread::spawn(move || {
        let mut last_cursor: Option<Cursor> = None;
        let mut passthrough = true;
        let mut tick: u64 = 0;
        loop {
            thread::sleep(Duration::from_millis(16));
            tick = tick.wrapping_add(1);
            let Some(win) = app.get_webview_window("main") else {
                continue;
            };
            let ov = app.state::<Overlay>();

            #[cfg(windows)]
            let own = win.hwnd().map(|h| h.0 as isize).unwrap_or(0);

            // 4 回に 1 回（約 15 回/秒）：前面のウィンドウを記録し、全画面かどうかを調べる
            #[cfg(windows)]
            if tick.is_multiple_of(4) {
                let fg = win::foreground_other_than(own);
                if fg != 0 {
                    ov.last_foreground.store(fg, Ordering::SeqCst);
                }
                let fullscreen = win::fullscreen_on_same_monitor(own);
                let was = ov.suspended.swap(fullscreen, Ordering::SeqCst);
                if fullscreen != was {
                    let _ = app.emit_to("main", "suspend", fullscreen);
                    if fullscreen {
                        let _ = win.set_ignore_cursor_events(true);
                        passthrough = true;
                        last_cursor = None;
                        hide(&app);
                    } else {
                        show(&app);
                    }
                }
            }
            if ov.is_suspended() {
                continue;
            }

            // 約 1 秒ごと：解像度・DPI・タスクバーの位置が変わっていないか、最前面を保てているか
            if tick.is_multiple_of(60) {
                if layout(&app) {
                    let _ = app.emit_to("main", "geometry", ());
                }
                #[cfg(windows)]
                win::keep_topmost(own);
            }

            let geom = *ov.geom.lock().unwrap();
            if geom.scale <= 0.0 {
                continue;
            }
            let Ok(pos) = app.cursor_position() else {
                continue;
            };
            let x = (pos.x - geom.x as f64) / geom.scale;
            let y = (pos.y - geom.y as f64) / geom.scale;
            let width = geom.w as f64 / geom.scale;
            let in_columns = x >= -1.0 && x <= width + 1.0;
            let near = in_columns && y <= WINDOW_HEIGHT + NEAR_MARGIN;

            // クリックを受け取るかどうか（カプセルとパネルの上だけ受け取る）
            let over = in_columns && ov.rects.lock().unwrap().iter().any(|r| r.contains(x, y));
            if over == passthrough {
                passthrough = !over;
                let _ = win.set_ignore_cursor_events(passthrough);
            }

            // 近くにいる間だけ位置を送る。離れたら一度だけ null を送る
            let next = near.then_some(Cursor {
                x: (x * 2.0).round() / 2.0,
                y: (y * 2.0).round() / 2.0,
            });
            if next != last_cursor {
                last_cursor = next;
                let _ = app.emit_to("main", "cursor", next);
            }
        }
    });
}

#[cfg(windows)]
mod win {
    use windows::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL,
            MONITOR_DEFAULTTOPRIMARY,
        },
        System::Threading::GetCurrentProcessId,
        UI::{
            Shell::{
                SHQueryUserNotificationState, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
            },
            WindowsAndMessaging::{
                GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect,
                GetWindowThreadProcessId, IsWindow, IsWindowVisible, SetForegroundWindow,
                SetWindowPos, ShowWindow, GWL_STYLE, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE,
                SWP_NOSIZE, SW_SHOWNOACTIVATE, WS_CAPTION,
            },
        },
    };

    fn hwnd(raw: isize) -> HWND {
        HWND(raw as *mut _)
    }

    fn is_own_process(h: HWND) -> bool {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        pid == unsafe { GetCurrentProcessId() }
    }

    /// デスクトップやタスクバーなど、全画面の判定から外すウィンドウ
    fn is_shell_window(h: HWND) -> bool {
        let mut buf = [0u16; 64];
        let n = unsafe { GetClassNameW(h, &mut buf) };
        let name = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
        matches!(
            name.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        )
    }

    /// 前面のウィンドウ（自分のプロセスのものなら 0）
    pub fn foreground_other_than(own: isize) -> isize {
        let fg = unsafe { GetForegroundWindow() };
        if fg.is_invalid() || fg.0 as isize == own || is_own_process(fg) {
            0
        } else {
            fg.0 as isize
        }
    }

    /// 自分と同じディスプレイで、全画面のアプリが前面にあるか
    pub fn fullscreen_on_same_monitor(own: isize) -> bool {
        unsafe {
            // D3D の排他的全画面やプレゼンテーションモード
            if let Ok(state) = SHQueryUserNotificationState() {
                if state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE {
                    return true;
                }
            }
            let fg = GetForegroundWindow();
            if fg.is_invalid()
                || !IsWindowVisible(fg).as_bool()
                || is_own_process(fg)
                || is_shell_window(fg)
            {
                return false;
            }
            let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
            if mon.is_invalid() || mon != MonitorFromWindow(hwnd(own), MONITOR_DEFAULTTOPRIMARY) {
                return false;
            }
            // タイトルバーのある普通の最大化ウィンドウは除く（タスクバー自動非表示のときの誤判定対策）
            let style = GetWindowLongW(fg, GWL_STYLE) as u32;
            if style & WS_CAPTION.0 == WS_CAPTION.0 {
                return false;
            }
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let mut r = RECT::default();
            if !GetMonitorInfoW(mon, &mut mi).as_bool() || GetWindowRect(fg, &mut r).is_err() {
                return false;
            }
            let m = mi.rcMonitor;
            r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
        }
    }

    pub fn show_no_activate(own: isize) {
        unsafe {
            let _ = ShowWindow(hwnd(own), SW_SHOWNOACTIVATE);
        }
        keep_topmost(own);
    }

    pub fn keep_topmost(own: isize) {
        if own == 0 {
            return;
        }
        unsafe {
            let _ = SetWindowPos(
                hwnd(own),
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    pub fn restore_focus(own: isize, prev: isize) {
        unsafe {
            if prev == 0 || GetForegroundWindow().0 as isize != own {
                return;
            }
            if IsWindow(Some(hwnd(prev))).as_bool() {
                let _ = SetForegroundWindow(hwnd(prev));
            }
        }
    }
}
