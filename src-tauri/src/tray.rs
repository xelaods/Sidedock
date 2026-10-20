//! 通知領域（トレイ）のアイコンを SideDock が自分で受け取る。
//!
//! アプリは Shell_NotifyIcon で、"Shell_TrayWnd" という名前のウィンドウに WM_COPYDATA を送って
//! トレイアイコンを登録する。SideDock も同じ名前のウィンドウを作って最前面に置き、
//! 「TaskbarCreated」を全体に知らせると、各アプリがアイコンを SideDock に登録し直してくれる。
//! 受け取ったメッセージはすべてエクスプローラーにもそのまま中継するので、
//! SideDock を止めてもエクスプローラーのトレイはそのまま使える（RetroBar などと同じ方式）。
//!
//! もう 1 つ、見えない「AppBar」ウィンドウで画面の端の領域を確保し、
//! 最大化したウィンドウが SideDock の下に潜り込まないようにする。

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrayIcon {
    pub key: String,
    /// マウスを乗せたときの説明
    pub tip: String,
    /// アイコン（PNG の data URL）
    pub icon: Option<String>,
    /// アプリの実行ファイル
    pub exe: String,
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use super::TrayIcon;
    use std::sync::{
        atomic::{AtomicBool, AtomicIsize, Ordering},
        Mutex, OnceLock,
    };
    use tauri::{AppHandle, Emitter};
    use windows::{
        core::{w, PCWSTR},
        Win32::{
            Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
            System::{DataExchange::COPYDATASTRUCT, LibraryLoader::GetModuleHandleW},
            UI::{
                Shell::{
                    SHAppBarMessage, ABE_BOTTOM, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE,
                    ABM_SETPOS, APPBARDATA,
                },
                WindowsAndMessaging::{
                    AllowSetForegroundWindow, CreateWindowExW, DefWindowProcW, DestroyWindow,
                    DispatchMessageW, EnumWindows, GetCursorPos, GetMessageW, IsWindow,
                    PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
                    SendMessageTimeoutW, SendNotifyMessageW, SetWindowPos, TranslateMessage,
                    HWND_BROADCAST, HWND_TOPMOST, MSG, SMTO_ABORTIFHUNG, SWP_NOACTIVATE,
                    SWP_NOMOVE, SWP_NOSIZE, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_CONTEXTMENU,
                    WM_COPYDATA, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
                    WM_RBUTTONDOWN, WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
                    WS_POPUP,
                },
            },
        },
    };

    // Shell_NotifyIcon が送ってくるデータ（32 ビットのハンドルで届く）
    const NIM_ADD: u32 = 0;
    const NIM_MODIFY: u32 = 1;
    const NIM_DELETE: u32 = 2;
    const NIM_SETVERSION: u32 = 4;
    const NIF_MESSAGE: u32 = 0x01;
    const NIF_ICON: u32 = 0x02;
    const NIF_TIP: u32 = 0x04;
    const NIF_STATE: u32 = 0x08;
    const NIF_GUID: u32 = 0x20;
    const NIS_HIDDEN: u32 = 0x01;
    const NIN_SELECT: u32 = 0x0400;
    const TRAY_SIGNATURE: i32 = 0x3475_3423;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NotifyIconData32 {
        cb_size: u32,
        hwnd: u32,
        uid: u32,
        flags: u32,
        callback: u32,
        hicon: u32,
        tip: [u16; 128],
        state: u32,
        state_mask: u32,
        info: [u16; 256],
        version: u32,
        info_title: [u16; 64],
        info_flags: u32,
        guid: [u8; 16],
        balloon_icon: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ShellTrayData {
        signature: i32,
        message: u32,
        nid: NotifyIconData32,
    }

    struct Entry {
        key: String,
        hwnd: u32,
        uid: u32,
        callback: u32,
        version: u32,
        hidden: bool,
        tip: String,
        icon: Option<String>,
        exe: String,
    }

    static APP: OnceLock<AppHandle> = OnceLock::new();
    static ICONS: Mutex<Vec<Entry>> = Mutex::new(Vec::new());
    static HOST: AtomicIsize = AtomicIsize::new(0);
    static APPBAR: AtomicIsize = AtomicIsize::new(0);
    static RUNNING: AtomicBool = AtomicBool::new(false);
    /// 今確保している画面の端（None なら確保していない）
    static RESERVED: Mutex<Option<(bool, RECT)>> = Mutex::new(None);
    /// AppBar の確保を、ウィンドウを持つスレッドで行うためのメッセージ
    const WM_RESERVE: u32 = WM_APP + 0x31;

    fn hwnd32(v: u32) -> HWND {
        HWND(v as i32 as isize as *mut _)
    }

    fn wstr(buf: &[u16]) -> String {
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..n])
    }

    fn key_of(nid: &NotifyIconData32) -> String {
        if nid.flags & NIF_GUID != 0 && nid.guid.iter().any(|&b| b != 0) {
            nid.guid.iter().map(|b| format!("{b:02x}")).collect()
        } else {
            format!("{:x}:{}", nid.hwnd, nid.uid)
        }
    }

    fn notify_changed() {
        if let Some(app) = APP.get() {
            let _ = app.emit_to("main", "tray-changed", ());
        }
    }

    /// エクスプローラーのタスクバー（自分以外の Shell_TrayWnd）
    fn explorer_tray() -> Option<HWND> {
        unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> windows::core::BOOL {
            let mut buf = [0u16; 32];
            let n = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(h, &mut buf);
            if String::from_utf16_lossy(&buf[..n.max(0) as usize]) == "Shell_TrayWnd"
                && h.0 as isize != HOST.load(Ordering::SeqCst)
            {
                *(l.0 as *mut Option<HWND>) = Some(h);
                return false.into();
            }
            true.into()
        }
        let mut found: Option<HWND> = None;
        unsafe {
            let _ = EnumWindows(Some(cb), LPARAM(&mut found as *mut Option<HWND> as isize));
        }
        found
    }

    /// アプリから届いたトレイの登録・変更・削除を反映する
    fn handle(data: &ShellTrayData) {
        if data.signature != TRAY_SIGNATURE {
            return;
        }
        let nid = &data.nid;
        let key = key_of(nid);
        let mut icons = ICONS.lock().unwrap();
        match data.message {
            NIM_ADD | NIM_MODIFY => {
                let idx = match icons.iter().position(|e| e.key == key) {
                    Some(i) => i,
                    None => {
                        if data.message == NIM_MODIFY {
                            // 知らないアイコンの変更は、追加として扱う（SideDock を後から起動したとき）
                        }
                        let pid = crate::taskbar::pid_of(hwnd32(nid.hwnd));
                        icons.push(Entry {
                            key: key.clone(),
                            hwnd: nid.hwnd,
                            uid: nid.uid,
                            callback: 0,
                            version: 0,
                            hidden: false,
                            tip: String::new(),
                            icon: None,
                            exe: crate::taskbar::exe_of_pid(pid),
                        });
                        icons.len() - 1
                    }
                };
                let e = &mut icons[idx];
                e.hwnd = nid.hwnd;
                e.uid = nid.uid;
                if nid.flags & NIF_MESSAGE != 0 {
                    e.callback = nid.callback;
                }
                if nid.flags & NIF_TIP != 0 {
                    e.tip = wstr(&nid.tip);
                }
                if nid.flags & NIF_STATE != 0 && nid.state_mask & NIS_HIDDEN != 0 {
                    e.hidden = nid.state & NIS_HIDDEN != 0;
                }
                if nid.flags & NIF_ICON != 0 && nid.hicon != 0 {
                    // アイコンはアプリがすぐ破棄することがあるので、受け取った時点で画像にする
                    let h = windows::Win32::UI::WindowsAndMessaging::HICON(
                        nid.hicon as i32 as isize as *mut _,
                    );
                    if let Ok(png) = crate::system::hicon_png(h) {
                        e.icon = Some(png);
                    }
                }
            }
            NIM_DELETE => icons.retain(|e| e.key != key),
            NIM_SETVERSION => {
                if let Some(e) = icons.iter_mut().find(|e| e.key == key) {
                    e.version = nid.version;
                }
            }
            _ => return,
        }
        drop(icons);
        notify_changed();
    }

    unsafe extern "system" fn host_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_COPYDATA => {
                // まずエクスプローラーにも同じものを渡す（SideDock を止めても困らないように）
                let mut result = 0usize;
                if let Some(t) = explorer_tray() {
                    let _ = SendMessageTimeoutW(
                        t,
                        WM_COPYDATA,
                        wp,
                        lp,
                        SMTO_ABORTIFHUNG,
                        300,
                        Some(&mut result),
                    );
                }
                let cds = &*(lp.0 as *const COPYDATASTRUCT);
                if cds.dwData == 1 && !cds.lpData.is_null() {
                    // 古いアプリは短いデータを送ってくるので、足りない分は 0 で埋めて読む
                    let mut data: ShellTrayData = std::mem::zeroed();
                    let n = (cds.cbData as usize).min(std::mem::size_of::<ShellTrayData>());
                    std::ptr::copy_nonoverlapping(
                        cds.lpData as *const u8,
                        &mut data as *mut _ as *mut u8,
                        n,
                    );
                    handle(&data);
                    return LRESULT(1);
                }
                // AppBar の問い合わせなどはエクスプローラーの答えをそのまま返す
                LRESULT(result as isize)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(h, msg, wp, lp),
        }
    }

    unsafe extern "system" fn appbar_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_RESERVE {
            apply_reserve(h);
            return LRESULT(0);
        }
        DefWindowProcW(h, msg, wp, lp)
    }

    fn appbar_data(h: HWND) -> APPBARDATA {
        APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: h,
            ..Default::default()
        }
    }

    /// 確保する領域を実際に登録する（AppBar のウィンドウのスレッドで呼ぶ）
    fn apply_reserve(h: HWND) {
        let want = *RESERVED.lock().unwrap();
        unsafe {
            let mut d = appbar_data(h);
            SHAppBarMessage(ABM_REMOVE, &mut d);
            let Some((bottom, rc)) = want else { return };
            let mut d = appbar_data(h);
            SHAppBarMessage(ABM_NEW, &mut d);
            let mut d = appbar_data(h);
            d.uEdge = if bottom { ABE_BOTTOM } else { ABE_TOP };
            d.rc = rc;
            SHAppBarMessage(ABM_QUERYPOS, &mut d);
            // QUERYPOS でずらされても、高さは保つ
            let height = rc.bottom - rc.top;
            if bottom {
                d.rc.top = d.rc.bottom - height;
            } else {
                d.rc.bottom = d.rc.top + height;
            }
            SHAppBarMessage(ABM_SETPOS, &mut d);
        }
    }

    /// トレイの受け取りと AppBar を始める・止める
    pub fn enable(app: &AppHandle, on: bool) {
        let _ = APP.set(app.clone());
        if on && !RUNNING.swap(true, Ordering::SeqCst) {
            std::thread::spawn(run);
        } else if !on && RUNNING.swap(false, Ordering::SeqCst) {
            *RESERVED.lock().unwrap() = None;
            unsafe {
                let ab = APPBAR.swap(0, Ordering::SeqCst);
                if ab != 0 {
                    let mut d = appbar_data(HWND(ab as *mut _));
                    SHAppBarMessage(ABM_REMOVE, &mut d);
                    let _ = PostMessageW(Some(HWND(ab as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                let h = HOST.load(Ordering::SeqCst);
                if h != 0 {
                    let _ = PostMessageW(Some(HWND(h as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            ICONS.lock().unwrap().clear();
            notify_changed();
        }
    }

    fn run() {
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let host_class = w!("Shell_TrayWnd");
            let bar_class = w!("SideDockAppBar");
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(host_proc),
                hInstance: hinst.into(),
                lpszClassName: host_class,
                ..Default::default()
            });
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(appbar_proc),
                hInstance: hinst.into(),
                lpszClassName: bar_class,
                ..Default::default()
            });
            let make = |class: PCWSTR, ex: WINDOW_EX_STYLE| {
                CreateWindowExW(
                    ex,
                    class,
                    w!(""),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(hinst.into()),
                    None,
                )
            };
            let Ok(host) = make(host_class, WS_EX_TOPMOST | WS_EX_TOOLWINDOW) else {
                RUNNING.store(false, Ordering::SeqCst);
                return;
            };
            HOST.store(host.0 as isize, Ordering::SeqCst);
            if let Ok(bar) = make(bar_class, WS_EX_TOOLWINDOW) {
                APPBAR.store(bar.0 as isize, Ordering::SeqCst);
                apply_reserve(bar);
            }
            keep_on_top();
            // 各アプリに、アイコンを登録し直してもらう
            rebroadcast();

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let bar = APPBAR.swap(0, Ordering::SeqCst);
            if bar != 0 {
                let _ = DestroyWindow(HWND(bar as *mut _));
            }
            let _ = DestroyWindow(host);
            HOST.store(0, Ordering::SeqCst);
            // アプリにエクスプローラーへ登録し直してもらう
            rebroadcast();
        }
    }

    fn rebroadcast() {
        unsafe {
            let m = RegisterWindowMessageW(w!("TaskbarCreated"));
            let _ = SendNotifyMessageW(HWND_BROADCAST, m, WPARAM(0), LPARAM(0));
        }
    }

    /// 自分の Shell_TrayWnd をエクスプローラーのものより手前に保つ（アプリが先に見つけるように）
    pub fn keep_on_top() {
        let h = HOST.load(Ordering::SeqCst);
        if h == 0 {
            return;
        }
        unsafe {
            let _ = SetWindowPos(
                HWND(h as *mut _),
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    /// 画面の端を確保する（None で解除）。rect は物理ピクセルの画面座標
    pub fn reserve(edge: Option<(bool, RECT)>) {
        {
            let mut r = RESERVED.lock().unwrap();
            if *r == edge {
                return;
            }
            *r = edge;
        }
        let ab = APPBAR.load(Ordering::SeqCst);
        if ab != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(ab as *mut _)), WM_RESERVE, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn list() -> Vec<TrayIcon> {
        let icons = ICONS.lock().unwrap();
        icons
            .iter()
            .filter(|e| !e.hidden && unsafe { IsWindow(Some(hwnd32(e.hwnd))).as_bool() })
            .map(|e| TrayIcon {
                key: e.key.clone(),
                tip: e.tip.clone(),
                icon: e.icon.clone(),
                exe: e.exe.clone(),
            })
            .collect()
    }

    /// アイコンのクリックをアプリに伝える。button は "left" | "right" | "double"
    pub fn click(key: &str, button: &str) -> Result<(), String> {
        let (hwnd, uid, callback, version) = {
            let icons = ICONS.lock().unwrap();
            let e = icons
                .iter()
                .find(|e| e.key == key)
                .ok_or("アイコンが見つかりません")?;
            (e.hwnd, e.uid, e.callback, e.version)
        };
        if callback == 0 {
            return Err("このアイコンはクリックに反応しません".into());
        }
        let target = hwnd32(hwnd);
        unsafe {
            // アプリが自分のメニューを前面に出せるように許可する
            let pid = crate::taskbar::pid_of(target);
            let _ = AllowSetForegroundWindow(pid);
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let send = |m: u32| {
                let (wp, lp) = if version >= 4 {
                    (
                        WPARAM(((pt.y as u32 & 0xffff) << 16 | (pt.x as u32 & 0xffff)) as usize),
                        LPARAM(((uid & 0xffff) << 16 | (m & 0xffff)) as isize),
                    )
                } else {
                    (WPARAM(uid as usize), LPARAM(m as isize))
                };
                let _ = PostMessageW(Some(target), callback, wp, lp);
            };
            match button {
                "right" => {
                    send(WM_RBUTTONDOWN);
                    send(WM_RBUTTONUP);
                    if version >= 4 {
                        send(WM_CONTEXTMENU);
                    }
                }
                "double" => {
                    send(WM_LBUTTONDOWN);
                    send(WM_LBUTTONUP);
                    send(WM_LBUTTONDBLCLK);
                    send(WM_LBUTTONUP);
                }
                _ => {
                    send(WM_LBUTTONDOWN);
                    send(WM_LBUTTONUP);
                    if version >= 4 {
                        send(NIN_SELECT);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn running() -> bool {
        RUNNING.load(Ordering::SeqCst)
    }

    /// 終了するとき：画面の領域を返し、アプリにエクスプローラーへ登録し直してもらう
    pub fn shutdown() {
        if !RUNNING.load(Ordering::SeqCst) {
            return;
        }
        if let Some(app) = APP.get() {
            enable(app, false);
        }
        // ウィンドウを閉じて登録し直しを知らせるまで少し待つ
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

#[cfg(not(windows))]
mod other {
    use super::TrayIcon;
    pub fn enable(_app: &tauri::AppHandle, _on: bool) {}
    pub fn keep_on_top() {}
    pub fn list() -> Vec<TrayIcon> {
        Vec::new()
    }
    pub fn click(_key: &str, _button: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn running() -> bool {
        false
    }
    pub fn shutdown() {}
}
#[cfg(not(windows))]
pub use other::*;
