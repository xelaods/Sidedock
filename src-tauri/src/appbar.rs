//! 画面端への固定。
//! Windows では「AppBar」として登録し、タスクバーと同じように画面の領域を確保する。
//! （最大化したウィンドウがサイドバーの下に潜り込まない）

use crate::config::Edge;

/// 論理px で表したレイアウト。実際の px への変換は DPI を見てこのモジュールが行う
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub edge: Edge,
    pub bar: u32,
    /// パネルを開いているときの幅（閉じているときは 0）
    pub panel: u32,
    pub reserve: bool,
}

#[cfg(windows)]
pub use win::{apply, unregister};

#[cfg(windows)]
mod win {
    use super::Layout;
    use crate::config::Edge;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    };
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY},
        UI::{
            HiDpi::GetDpiForWindow,
            Shell::{
                DefSubclassProc, SHAppBarMessage, SetWindowSubclass, ABE_LEFT, ABE_RIGHT,
                ABM_ACTIVATE, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABM_WINDOWPOSCHANGED,
                ABN_FULLSCREENAPP, ABN_POSCHANGED, APPBARDATA,
            },
            WindowsAndMessaging::{
                SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
                WM_ACTIVATE, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_SETTINGCHANGE,
                WM_WINDOWPOSCHANGED,
            },
        },
    };

    /// シェルから AppBar 通知を受け取るためのメッセージ番号
    const CALLBACK_MSG: u32 = WM_APP + 0x51;
    const SUBCLASS_ID: usize = 0x51DE;

    struct Inner {
        hwnd: isize,
        layout: Option<Layout>,
        registered: bool,
        subclassed: bool,
    }

    static STATE: Mutex<Inner> = Mutex::new(Inner {
        hwnd: 0,
        layout: None,
        registered: false,
        subclassed: false,
    });
    /// SetWindowPos などで自分宛ての通知が再入したときに無限ループしないための目印
    static APPLYING: AtomicBool = AtomicBool::new(false);

    fn abd(hwnd: HWND) -> APPBARDATA {
        APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            uCallbackMessage: CALLBACK_MSG,
            ..Default::default()
        }
    }

    /// レイアウトを反映する（UI スレッドから呼ぶ）
    pub fn apply(hwnd_raw: isize, layout: Layout) {
        let hwnd = HWND(hwnd_raw as *mut _);
        let (need_register, need_unregister, need_subclass) = {
            let mut s = STATE.lock().unwrap();
            s.hwnd = hwnd_raw;
            s.layout = Some(layout);
            let r = (
                layout.reserve && !s.registered,
                !layout.reserve && s.registered,
                !s.subclassed,
            );
            s.registered = layout.reserve;
            s.subclassed = true;
            r
        };

        unsafe {
            if need_subclass {
                let _ = SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0);
            }
            if need_register {
                let mut d = abd(hwnd);
                SHAppBarMessage(ABM_NEW, &mut d);
            }
            if need_unregister {
                let mut d = abd(hwnd);
                SHAppBarMessage(ABM_REMOVE, &mut d);
            }
        }
        position(hwnd, layout);
    }

    /// 終了時に AppBar の登録を外す（外さないと確保した領域が残ってしまう）
    pub fn unregister() {
        let (hwnd, registered) = {
            let mut s = STATE.lock().unwrap();
            let r = (s.hwnd, s.registered);
            s.registered = false;
            r
        };
        if registered && hwnd != 0 {
            unsafe {
                let mut d = abd(HWND(hwnd as *mut _));
                SHAppBarMessage(ABM_REMOVE, &mut d);
            }
        }
    }

    fn position(hwnd: HWND, layout: Layout) {
        if APPLYING.swap(true, Ordering::SeqCst) {
            return;
        }
        unsafe {
            let scale = match GetDpiForWindow(hwnd) {
                0 => 1.0,
                dpi => dpi as f64 / 96.0,
            };
            let bar = (layout.bar as f64 * scale).round() as i32;
            let panel = (layout.panel as f64 * scale).round() as i32;

            // 常にメインディスプレイに置く
            let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
                APPLYING.store(false, Ordering::SeqCst);
                return;
            }

            // バー本体が占める横方向の範囲を決める
            let (bar_left, bar_right) = if layout.reserve {
                let mut d = abd(hwnd);
                d.uEdge = if layout.edge == Edge::Right {
                    ABE_RIGHT
                } else {
                    ABE_LEFT
                };
                d.rc = mi.rcMonitor;
                set_width(&mut d.rc, layout.edge, bar);
                SHAppBarMessage(ABM_QUERYPOS, &mut d);
                // QUERYPOS でずらされた位置を基準に、幅をもう一度合わせる
                set_width(&mut d.rc, layout.edge, bar);
                SHAppBarMessage(ABM_SETPOS, &mut d);
                (d.rc.left, d.rc.right)
            } else {
                let mut rc = mi.rcWork;
                set_width(&mut rc, layout.edge, bar);
                (rc.left, rc.right)
            };

            // 縦はタスクバーを除いた作業領域に合わせる
            let top = mi.rcWork.top;
            let height = mi.rcWork.bottom - mi.rcWork.top;
            // パネルを開いたときは画面の内側に向かって広げる（他のウィンドウの上に重なる）
            let (x, width) = match layout.edge {
                Edge::Right => (bar_left - panel, bar + panel),
                Edge::Left => (bar_left, (bar_right - bar_left) + panel),
            };
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                top,
                width,
                height,
                SWP_NOACTIVATE,
            );
        }
        APPLYING.store(false, Ordering::SeqCst);
    }

    fn set_width(rc: &mut RECT, edge: Edge, width: i32) {
        match edge {
            Edge::Right => rc.left = rc.right - width,
            Edge::Left => rc.right = rc.left + width,
        }
    }

    fn reapply() {
        let snapshot = {
            let s = STATE.lock().unwrap();
            s.layout.map(|l| (s.hwnd, l))
        };
        if let Some((hwnd, layout)) = snapshot {
            position(HWND(hwnd as *mut _), layout);
        }
    }

    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {
            CALLBACK_MSG => {
                match wparam.0 as u32 {
                    // 他の AppBar（タスクバーなど）が動いた
                    ABN_POSCHANGED => reapply(),
                    // 全画面アプリ（動画・ゲーム）が出ている間は最前面をやめる
                    ABN_FULLSCREENAPP => {
                        let after = if lparam.0 != 0 {
                            HWND_NOTOPMOST
                        } else {
                            HWND_TOPMOST
                        };
                        let _ = SetWindowPos(
                            hwnd,
                            Some(after),
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                    }
                    _ => {}
                }
                return LRESULT(0);
            }
            WM_ACTIVATE => {
                let mut d = abd(hwnd);
                SHAppBarMessage(ABM_ACTIVATE, &mut d);
            }
            WM_WINDOWPOSCHANGED => {
                let mut d = abd(hwnd);
                SHAppBarMessage(ABM_WINDOWPOSCHANGED, &mut d);
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE => {
                // 既定の処理（DPI 変更時のリサイズなど）の後に位置を合わせ直す
                let r = DefSubclassProc(hwnd, msg, wparam, lparam);
                reapply();
                return r;
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

// ---------------------------------------------------------------------------
// Windows 以外（開発時の確認用）：画面端に置くだけ
// ---------------------------------------------------------------------------
#[cfg(not(windows))]
pub fn apply_fallback(window: &tauri::WebviewWindow, layout: Layout) {
    use tauri::{PhysicalPosition, PhysicalSize};
    let Ok(Some(m)) = window.primary_monitor() else {
        return;
    };
    let scale = m.scale_factor();
    let size = m.size();
    let pos = m.position();
    let bar = (layout.bar as f64 * scale).round() as i32;
    let panel = (layout.panel as f64 * scale).round() as i32;
    let width = bar + panel;
    let x = match layout.edge {
        Edge::Right => pos.x + size.width as i32 - width,
        Edge::Left => pos.x,
    };
    let _ = window.set_size(PhysicalSize::new(width as u32, size.height));
    let _ = window.set_position(PhysicalPosition::new(x, pos.y));
}
