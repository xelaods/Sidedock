//! Windows のタスクバーの代わり：
//! - 開いているウィンドウの一覧・切り替え・最小化・閉じる
//! - スタートメニュー、クイック設定、通知センター、デスクトップの表示
//! - Windows のタスクバーを隠す・元に戻す（トレイを見るときだけ一時的に出す）

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    /// ウィンドウのハンドル（数値）
    pub id: i64,
    pub title: String,
    /// 実行ファイルのパス（アイコンとまとめ表示に使う）
    pub exe: String,
    pub minimized: bool,
    /// 直前まで使っていたウィンドウか（SideDock を触る前の前面）
    pub active: bool,
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use super::WindowInfo;
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicBool, AtomicI32, Ordering},
            Mutex,
        },
    };
    use windows::{
        core::{BOOL, PWSTR},
        Win32::{
            Foundation::{CloseHandle, HWND, LPARAM, WPARAM},
            Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
            System::Threading::{
                GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
                PROCESS_QUERY_LIMITED_INFORMATION,
            },
            UI::{
                Accessibility::{SetWinEventHook, HWINEVENTHOOK},
                Input::KeyboardAndMouse::{
                    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
                    VIRTUAL_KEY, VK_LWIN, VK_MENU,
                },
                Shell::{SHAppBarMessage, ABM_GETSTATE, ABM_SETSTATE, ABS_AUTOHIDE, APPBARDATA},
                WindowsAndMessaging::{
                    DispatchMessageW, EnumChildWindows, EnumWindows, GetClassLongPtrW,
                    GetClassNameW, GetForegroundWindow, GetWindow, GetWindowLongW,
                    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
                    IsWindow, IsWindowVisible, MsgWaitForMultipleObjects, PeekMessageW,
                    PostMessageW, SendMessageTimeoutW, SetForegroundWindow, ShowWindow,
                    TranslateMessage, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE,
                    EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GCLP_HICON, GWL_EXSTYLE,
                    GW_OWNER, HICON, ICON_BIG, MSG, PM_REMOVE, QS_ALLINPUT, SMTO_ABORTIFHUNG,
                    SW_HIDE, SW_MINIMIZE, SW_RESTORE, SW_SHOWNOACTIVATE, WINEVENT_OUTOFCONTEXT,
                    WINEVENT_SKIPOWNPROCESS, WM_CLOSE, WM_GETICON, WS_EX_APPWINDOW,
                    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
                },
            },
        },
    };

    fn hwnd(id: i64) -> HWND {
        HWND(id as isize as *mut _)
    }

    fn class_name(h: HWND) -> String {
        let mut buf = [0u16; 128];
        let n = unsafe { GetClassNameW(h, &mut buf) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    pub(crate) fn pid_of(h: HWND) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        pid
    }

    pub(crate) fn exe_of_pid(pid: u32) -> String {
        unsafe {
            let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return String::new();
            };
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                p,
                PROCESS_NAME_WIN32,
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(p);
            if ok.is_ok() {
                String::from_utf16_lossy(&buf[..len as usize])
            } else {
                String::new()
            }
        }
    }

    fn cloaked(h: HWND) -> bool {
        let mut v = 0u32;
        unsafe {
            DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut v as *mut u32 as *mut _, 4).is_ok()
                && v != 0
        }
    }

    /// ストアアプリは ApplicationFrameHost の中にいるので、中身のプロセスを探す
    fn uwp_child_pid(frame: HWND) -> Option<u32> {
        struct Ctx {
            host: u32,
            found: Option<u32>,
        }
        unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
            let ctx = &mut *(l.0 as *mut Ctx);
            let pid = pid_of(h);
            if pid != ctx.host && class_name(h) == "Windows.UI.Core.CoreWindow" {
                ctx.found = Some(pid);
                return false.into();
            }
            true.into()
        }
        let mut ctx = Ctx {
            host: pid_of(frame),
            found: None,
        };
        unsafe {
            let _ = EnumChildWindows(Some(frame), Some(cb), LPARAM(&mut ctx as *mut Ctx as isize));
        }
        ctx.found
    }

    /// タスクバーに出るのと同じ条件で、アプリのウィンドウか判定する
    fn is_app_window(h: HWND, own_pid: u32) -> bool {
        unsafe {
            if !IsWindowVisible(h).as_bool() || cloaked(h) || pid_of(h) == own_pid {
                return false;
            }
            let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
            let owner = GetWindow(h, GW_OWNER).unwrap_or_default();
            // WS_EX_APPWINDOW が付いていればタスクバーに出る。付いていなければ、
            // ツールウィンドウ・アクティブにならないウィンドウ・持ち主のいるウィンドウは出さない
            if ex & WS_EX_APPWINDOW.0 == 0
                && (ex & WS_EX_TOOLWINDOW.0 != 0
                    || ex & WS_EX_NOACTIVATE.0 != 0
                    || !owner.is_invalid())
            {
                return false;
            }
            if GetWindowTextLengthW(h) == 0 {
                return false;
            }
            !matches!(
                class_name(h).as_str(),
                "Progman"
                    | "WorkerW"
                    | "Shell_TrayWnd"
                    | "Shell_SecondaryTrayWnd"
                    | "Windows.UI.Core.CoreWindow"
            )
        }
    }

    /// 開いているウィンドウ（タスクバーに並ぶもの）。`last` は SideDock を触る前の前面
    /// プロセスの実行ファイルは毎回調べると重いので覚えておく
    static EXE_CACHE: Mutex<Option<HashMap<u32, String>>> = Mutex::new(None);

    fn exe_cached(pid: u32) -> String {
        let mut cache = EXE_CACHE.lock().unwrap();
        let map = cache.get_or_insert_with(HashMap::new);
        if let Some(e) = map.get(&pid) {
            return e.clone();
        }
        if map.len() > 400 {
            map.clear();
        }
        let e = exe_of_pid(pid);
        map.insert(pid, e.clone());
        e
    }

    // ------------------------------------------------------------ ウィンドウの変化を見張る

    static WATCHING: AtomicBool = AtomicBool::new(false);
    static DIRTY: AtomicBool = AtomicBool::new(false);

    unsafe extern "system" fn on_event(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        id_object: i32,
        id_child: i32,
        _thread: u32,
        _time: u32,
    ) {
        // ウィンドウそのもの（中の部品ではない）の変化だけ
        if id_object == 0 && id_child == 0 && !hwnd.is_invalid() {
            DIRTY.store(true, Ordering::Relaxed);
        }
    }

    /// ウィンドウが開いた・閉じた・切り替わった・題名が変わったら "windows-changed" を送る
    /// （1 秒ごとに一覧を取り直す代わり）。何度呼んでも 1 回だけ
    pub fn watch() {
        if WATCHING.swap(true, Ordering::SeqCst) {
            return;
        }
        std::thread::spawn(|| unsafe {
            let ranges = [
                (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
                (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
                (EVENT_OBJECT_CREATE, EVENT_OBJECT_HIDE),
                (EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_NAMECHANGE),
                (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
            ];
            for (a, b) in ranges {
                SetWinEventHook(
                    a,
                    b,
                    None,
                    Some(on_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
            }
            // フックはこのスレッドのメッセージで呼ばれる。150ms ごとにまとめて知らせる
            let mut msg = MSG::default();
            loop {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if DIRTY.swap(false, Ordering::Relaxed) {
                    if let Some(app) = crate::APP.get() {
                        use tauri::Emitter;
                        let _ = app.emit_to("main", "windows-changed", ());
                    }
                }
                let _ = MsgWaitForMultipleObjects(None, false, 150, QS_ALLINPUT);
            }
        });
    }

    pub fn list(last: isize) -> Vec<WindowInfo> {
        watch();
        unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
            let v = &mut *(l.0 as *mut Vec<HWND>);
            v.push(h);
            true.into()
        }
        let mut handles: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(cb), LPARAM(&mut handles as *mut Vec<HWND> as isize));
        }
        let own = unsafe { GetCurrentProcessId() };
        let fg = unsafe { GetForegroundWindow() };
        let current = if pid_of(fg) == own {
            last
        } else {
            fg.0 as isize
        };
        let mut out = Vec::new();
        for h in handles {
            if !is_app_window(h, own) {
                continue;
            }
            let len = unsafe { GetWindowTextLengthW(h) }.max(0) as usize;
            let mut buf = vec![0u16; len + 1];
            let n = unsafe { GetWindowTextW(h, &mut buf) };
            let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            let mut pid = pid_of(h);
            if class_name(h) == "ApplicationFrameWindow" {
                if let Some(p) = uwp_child_pid(h) {
                    pid = p;
                }
            }
            out.push(WindowInfo {
                id: h.0 as isize as i64,
                title,
                exe: exe_cached(pid),
                minimized: unsafe { IsIconic(h).as_bool() },
                active: h.0 as isize == current,
            });
        }
        out
    }

    /// ウィンドウのアイコン（PNG の data URL）。ウィンドウ自身→クラス→実行ファイルの順に探す
    pub fn icon(id: i64, exe: &str) -> Result<String, String> {
        let h = hwnd(id);
        unsafe {
            let mut res = 0usize;
            let _ = SendMessageTimeoutW(
                h,
                WM_GETICON,
                WPARAM(ICON_BIG as usize),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                120,
                Some(&mut res),
            );
            let mut hicon = HICON(res as *mut _);
            if hicon.is_invalid() {
                hicon = HICON(GetClassLongPtrW(h, GCLP_HICON) as *mut _);
            }
            // ウィンドウやクラスのアイコンは借り物なので、こちらで破棄しない
            if !hicon.is_invalid() {
                if let Ok(png) = crate::system::hicon_png(hicon) {
                    return Ok(png);
                }
            }
        }
        if exe.is_empty() {
            return Err("アイコンがありません".into());
        }
        crate::system::icon_png(exe)
    }

    fn press(keys: &[VIRTUAL_KEY]) {
        let mut inputs: Vec<INPUT> = Vec::new();
        let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    ..Default::default()
                },
            },
        };
        for &k in keys {
            inputs.push(key(k, false));
        }
        for &k in keys.iter().rev() {
            inputs.push(key(k, true));
        }
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    /// 前面に出す（最小化していたら戻す）
    fn bring_front(h: HWND) {
        unsafe {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            if !SetForegroundWindow(h).as_bool() {
                // 前面にできないときは Alt を一度押すと許される（Windows の制限の回避）
                press(&[VK_MENU]);
                let _ = SetForegroundWindow(h);
            }
        }
    }

    /// クリックしたとき：使っていたウィンドウなら最小化、そうでなければ前面へ
    pub fn activate(id: i64, last: isize) -> Result<(), String> {
        let h = hwnd(id);
        unsafe {
            if !IsWindow(Some(h)).as_bool() {
                return Err("ウィンドウが見つかりません".into());
            }
            if last == id as isize && !IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_MINIMIZE);
            } else {
                bring_front(h);
            }
        }
        Ok(())
    }

    pub fn focus(id: i64) {
        bring_front(hwnd(id));
    }

    pub fn close(id: i64) {
        unsafe {
            let _ = PostMessageW(Some(hwnd(id)), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }

    /// "start" | "quick" | "notifications" | "desktop" | "taskview"
    pub fn shell(action: &str) -> Result<(), String> {
        match action {
            "start" => press(&[VK_LWIN]),
            "quick" => press(&[VK_LWIN, VIRTUAL_KEY(b'A' as u16)]),
            "notifications" => press(&[VK_LWIN, VIRTUAL_KEY(b'N' as u16)]),
            "desktop" => press(&[VK_LWIN, VIRTUAL_KEY(b'D' as u16)]),
            "taskview" => press(&[VK_LWIN, VIRTUAL_KEY(0x09)]),
            // トレイは SideDock が自分で受け取って表示する（tray.rs）。ここでは何もしない
            "tray" => {}
            _ => return Err(format!("不明な操作です: {action}")),
        }
        Ok(())
    }

    // ------------------------------------------------------------ タスクバーを隠す

    static HIDE: AtomicBool = AtomicBool::new(false);
    /// 隠す前の「自動的に隠す」の状態（-1 は未保存）
    static PREV_STATE: AtomicI32 = AtomicI32::new(-1);
    /// エクスプローラーのタスクバー（メインとサブディスプレイ）。
    /// SideDock 自身も Shell_TrayWnd という名前のウィンドウを持つので、自分のものは除く
    fn trays() -> Vec<HWND> {
        unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
            let class = class_name(h);
            if (class == "Shell_TrayWnd" || class == "Shell_SecondaryTrayWnd")
                && pid_of(h) != GetCurrentProcessId()
            {
                let v = &mut *(l.0 as *mut Vec<HWND>);
                // メインのタスクバーを先頭に
                if class == "Shell_TrayWnd" {
                    v.insert(0, h);
                } else {
                    v.push(h);
                }
            }
            true.into()
        }
        let mut v = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(cb), LPARAM(&mut v as *mut Vec<HWND> as isize));
        }
        v
    }

    fn appbar_state(tray: HWND) -> u32 {
        let mut d = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: tray,
            ..Default::default()
        };
        unsafe { SHAppBarMessage(ABM_GETSTATE, &mut d) as u32 }
    }

    fn set_appbar_state(tray: HWND, state: u32) {
        let mut d = APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: tray,
            lParam: LPARAM(state as isize),
            ..Default::default()
        };
        unsafe {
            SHAppBarMessage(ABM_SETSTATE, &mut d);
        }
    }

    /// 隠す設定を切り替える。隠すときは「自動的に隠す」にして画面の下まで使えるようにする
    pub fn set_hidden(hide: bool) {
        HIDE.store(hide, Ordering::SeqCst);
        let list = trays();
        let Some(&main) = list.first() else { return };
        if hide {
            if PREV_STATE.load(Ordering::SeqCst) < 0 {
                PREV_STATE.store(appbar_state(main) as i32, Ordering::SeqCst);
            }
            set_appbar_state(main, ABS_AUTOHIDE);
            for t in list {
                unsafe {
                    let _ = ShowWindow(t, SW_HIDE);
                }
            }
        } else {
            for t in list {
                unsafe {
                    let _ = ShowWindow(t, SW_SHOWNOACTIVATE);
                }
            }
            let prev = PREV_STATE.swap(-1, Ordering::SeqCst);
            if prev >= 0 {
                set_appbar_state(main, prev as u32);
            }
        }
    }

    /// 定期的に呼ぶ：Windows が勝手にタスクバーを戻したら隠し直す。
    /// スタートメニューや検索を開いている間も出さない（タスクバーがなくてもスタートは開く）
    pub fn enforce() {
        if !HIDE.load(Ordering::SeqCst) {
            return;
        }
        for t in trays() {
            unsafe {
                if IsWindowVisible(t).as_bool() {
                    let _ = ShowWindow(t, SW_HIDE);
                }
            }
        }
        crate::tray::keep_on_top();
    }

    /// 終了するときは必ず元に戻す
    pub fn restore() {
        if HIDE.load(Ordering::SeqCst) {
            set_hidden(false);
        }
    }
}

#[cfg(not(windows))]
mod other {
    use super::WindowInfo;
    pub fn list(_last: isize) -> Vec<WindowInfo> {
        Vec::new()
    }
    pub fn icon(_id: i64, _exe: &str) -> Result<String, String> {
        Err("この OS では使えません".into())
    }
    pub fn activate(_id: i64, _last: isize) -> Result<(), String> {
        Ok(())
    }
    pub fn focus(_id: i64) {}
    pub fn close(_id: i64) {}
    pub fn shell(_action: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn set_hidden(_hide: bool) {}
    pub fn enforce() {}
    pub fn restore() {}
}
#[cfg(not(windows))]
pub use other::*;
