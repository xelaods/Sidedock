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
    time::{Duration, Instant},
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
    /// 画面の下端（タスクバーの位置）に出すか
    pub bottom: AtomicBool,
}

impl Overlay {
    pub fn new() -> Self {
        Self {
            geom: Mutex::new(Geom::default()),
            rects: Mutex::new(Vec::new()),
            suspended: AtomicBool::new(false),
            last_foreground: AtomicIsize::new(0),
            bottom: AtomicBool::new(false),
        }
    }

    pub fn set_rects(&self, rects: Vec<Rect>) {
        *self.rects.lock().unwrap() = rects;
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended.load(Ordering::SeqCst)
    }
}

/// 出す端を変える（"top" | "bottom"）。すぐに配置し直す
pub fn set_edge(app: &AppHandle, edge: &str) {
    app.state::<Overlay>()
        .bottom
        .store(edge == "bottom", Ordering::SeqCst);
    if layout(app) {
        let _ = app.emit_to("main", "geometry", ());
    }
}

/// メインディスプレイの上端（または下端）に、全幅でウィンドウを合わせる。
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
    let ov = app.state::<Overlay>();
    let h = (WINDOW_HEIGHT * scale).round() as u32;
    let y = if ov.bottom.load(Ordering::SeqCst) {
        wa.position.y + wa.size.height as i32 - h as i32
    } else {
        wa.position.y
    };
    let next = Geom {
        x: wa.position.x,
        y,
        w: wa.size.width,
        h,
        scale,
    };
    let mut g = ov.geom.lock().unwrap();
    if *g == next {
        return false;
    }
    *g = next;
    drop(g);
    let _ = win.set_size(PhysicalSize::new(next.w, next.h));
    let _ = win.set_position(PhysicalPosition::new(next.x, next.y));
    crate::log::write(
        app,
        &format!(
            "配置: x={} y={} w={} h={} 拡大率={}",
            next.x, next.y, next.w, next.h, next.scale
        ),
    );
    true
}

/// 表示する。
/// ※ 必ず Tauri の show() を使う。Win32 の ShowWindow で直接出すと、Tauri（tao）が
///   「まだ隠れている」と思い込み、クリック透過を切り替えるたびにウィンドウを隠し直してしまう。
///   tauri.conf.json の focus: false により、tao はフォーカスを奪わない SW_SHOWNOACTIVATE で出す。
pub fn show(app: &AppHandle) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let _ = win.show();
    let _ = win.set_always_on_top(true);
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

/// SideDock を触る前に前面にあったウィンドウ
pub fn last_foreground(app: &AppHandle) -> isize {
    app.state::<Overlay>()
        .last_foreground
        .load(Ordering::SeqCst)
}

/// カーソル監視のスレッドを始める。
/// 軽くするため、カーソルが画面上部の近くにあるときだけ約 60 回/秒で調べ、
/// 離れているときや全画面の間は約 11 回/秒に落とす
pub fn start(app: AppHandle) {
    thread::spawn(move || {
        let mut last_cursor: Option<Cursor> = None;
        let mut passthrough = true;
        let mut fast = true;
        let mut next_check = Instant::now();
        let mut next_layout = Instant::now();
        loop {
            thread::sleep(Duration::from_millis(if fast { 16 } else { 90 }));
            let now = Instant::now();
            let Some(win) = app.get_webview_window("main") else {
                continue;
            };
            let ov = app.state::<Overlay>();

            #[cfg(windows)]
            let own = win.hwnd().map(|h| h.0 as isize).unwrap_or(0);

            // 4 回/秒：前面のウィンドウを記録し、全画面かどうかを調べ、隠したタスクバーを隠し直す
            if now >= next_check {
                next_check = now + Duration::from_millis(250);
                crate::taskbar::enforce();
                #[cfg(windows)]
                {
                    let fg = win::foreground_other_than(own);
                    if fg != 0 {
                        ov.last_foreground.store(fg, Ordering::SeqCst);
                    }
                    let fullscreen = win::fullscreen_on_same_monitor(own);
                    let was = ov.suspended.swap(fullscreen, Ordering::SeqCst);
                    if fullscreen != was {
                        crate::log::write(
                            &app,
                            &format!(
                                "{}: {}",
                                if fullscreen {
                                    "全画面を検出したので隠す"
                                } else {
                                    "全画面が終わったので表示"
                                },
                                win::describe_foreground()
                            ),
                        );
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
            }
            if ov.is_suspended() {
                fast = false;
                continue;
            }

            // 1 回/秒：解像度・DPI・タスクバーの位置が変わっていないか、最前面を保てているか
            if now >= next_layout {
                next_layout = now + Duration::from_secs(1);
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
            // 上端に出すときも下端に出すときも、ウィンドウの近くにいる間だけ
            let near = in_columns && (-NEAR_MARGIN..=WINDOW_HEIGHT + NEAR_MARGIN).contains(&y);

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
            fast = near || over;
        }
    });
}

#[cfg(windows)]
mod win {
    use windows::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
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
                GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowTextW,
                GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed,
                SetForegroundWindow, SetWindowPos, GWL_STYLE, HWND_TOPMOST, SWP_NOACTIVATE,
                SWP_NOMOVE, SWP_NOSIZE, WS_CAPTION,
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
        matches!(
            class_name(h).as_str(),
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

    /// 他の仮想デスクトップにある・一時停止中のストアアプリなど、見えていないウィンドウ
    fn is_cloaked(h: HWND) -> bool {
        let mut cloaked = 0u32;
        unsafe {
            DwmGetWindowAttribute(
                h,
                DWMWA_CLOAKED,
                &mut cloaked as *mut u32 as *mut _,
                std::mem::size_of::<u32>() as u32,
            )
            .is_ok()
                && cloaked != 0
        }
    }

    fn class_name(h: HWND) -> String {
        let mut buf = [0u16; 128];
        let n = unsafe { GetClassNameW(h, &mut buf) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    fn title(h: HWND) -> String {
        let mut buf = [0u16; 128];
        let n = unsafe { GetWindowTextW(h, &mut buf) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    /// ログ用：前面のウィンドウの説明
    pub fn describe_foreground() -> String {
        let fg = unsafe { GetForegroundWindow() };
        if fg.is_invalid() {
            return "(なし)".into();
        }
        let mut r = RECT::default();
        let _ = unsafe { GetWindowRect(fg, &mut r) };
        let zoomed = unsafe { IsZoomed(fg).as_bool() };
        format!(
            "class={} title={:?} rect=({},{})-({},{}) maximized={}",
            class_name(fg),
            title(fg),
            r.left,
            r.top,
            r.right,
            r.bottom,
            zoomed
        )
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
                || is_cloaked(fg)
            {
                return false;
            }
            // 最大化しているだけのウィンドウは全画面ではない。
            // （タスクバーを自動で隠す設定だと、最大化でも画面全体を覆うので見分けが要る）
            if IsZoomed(fg).as_bool() || IsIconic(fg).as_bool() {
                return false;
            }
            // タイトルバーのある普通のウィンドウも除く
            let style = GetWindowLongW(fg, GWL_STYLE) as u32;
            if style & WS_CAPTION.0 == WS_CAPTION.0 {
                return false;
            }
            let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
            if mon.is_invalid() || mon != MonitorFromWindow(hwnd(own), MONITOR_DEFAULTTOPRIMARY) {
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
