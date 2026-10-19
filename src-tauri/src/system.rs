//! Windows の機能：音量、アプリの起動、アイコンの取り出し、インストール済みブラウザの一覧

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    /// 0.0〜1.0
    pub level: f32,
    pub muted: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInfo {
    pub id: String,
    pub name: String,
    pub exe: String,
}

/// ブラウザに渡してよい URL か（"-" 始まりのコマンドライン引数の注入などを防ぐ）
pub fn is_web_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    (lower.starts_with("https://") || lower.starts_with("http://"))
        && !url.contains(['\r', '\n', '"'])
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use super::{is_web_url, BrowserInfo, VolumeInfo};
    use base64::Engine;
    use std::{ffi::c_void, path::Path};
    use windows::{
        core::{w, HSTRING, PCWSTR},
        Win32::{
            Graphics::Gdi::{
                CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP,
                BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
            },
            Media::Audio::{
                eConsole, eRender, Endpoints::IAudioEndpointVolume, IMMDeviceEnumerator,
                MMDeviceEnumerator,
            },
            Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES,
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL,
                COINIT_APARTMENTTHREADED,
            },
            UI::{
                Controls::{IImageList, ILD_TRANSPARENT},
                Shell::{
                    SHGetFileInfoW, SHGetImageList, ShellExecuteW, SHFILEINFOW, SHGFI_ICON,
                    SHGFI_LARGEICON, SHGFI_SYSICONINDEX, SHIL_EXTRALARGE,
                },
                WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO, SW_SHOWNORMAL},
            },
        },
    };

    /// このスレッドで COM を使う間だけ初期化しておく
    struct ComGuard(bool);
    impl ComGuard {
        fn new() -> Self {
            // シェル系 API のため STA で初期化。既に別モードで初期化済みなら何もしない
            let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            ComGuard(hr.is_ok())
        }
    }
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    // ---------------------------------------------------------------- 音量

    fn with_volume<T>(
        f: impl FnOnce(&IAudioEndpointVolume) -> windows::core::Result<T>,
    ) -> Result<T, String> {
        let _com = ComGuard::new();
        let result = unsafe {
            (|| {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
                let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
                f(&volume)
            })()
        };
        result.map_err(|e| format!("音量を操作できませんでした: {}", e.message()))
    }

    pub fn volume_get() -> Result<VolumeInfo, String> {
        with_volume(|v| unsafe {
            Ok(VolumeInfo {
                level: v.GetMasterVolumeLevelScalar()?,
                muted: v.GetMute()?.as_bool(),
            })
        })
    }

    pub fn volume_set(level: f32) -> Result<(), String> {
        let level = if level.is_finite() {
            level.clamp(0.0, 1.0)
        } else {
            0.0
        };
        with_volume(|v| unsafe {
            v.SetMasterVolumeLevelScalar(level, std::ptr::null())?;
            // 音量を上げたらミュートは解除する（タスクバーと同じ挙動）
            if level > 0.0 && v.GetMute()?.as_bool() {
                v.SetMute(false, std::ptr::null())?;
            }
            Ok(())
        })
    }

    pub fn volume_set_mute(muted: bool) -> Result<(), String> {
        with_volume(|v| unsafe { v.SetMute(muted, std::ptr::null()) })
    }

    // ---------------------------------------------------------------- 起動

    /// exe / lnk / フォルダ / URL などを「開く」（エクスプローラーでダブルクリックしたのと同じ）
    pub fn launch(path: &str, args: Option<&str>) -> Result<(), String> {
        let _com = ComGuard::new();
        let file = HSTRING::from(path);
        let args = args.filter(|a| !a.is_empty()).map(HSTRING::from);
        let dir = Path::new(path)
            .parent()
            .filter(|p| p.is_dir())
            .map(|p| HSTRING::from(p.as_os_str()));
        let r = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                &file,
                args.as_ref()
                    .map(|a| PCWSTR(a.as_ptr()))
                    .unwrap_or(PCWSTR::null()),
                dir.as_ref()
                    .map(|d| PCWSTR(d.as_ptr()))
                    .unwrap_or(PCWSTR::null()),
                SW_SHOWNORMAL,
            )
        };
        // 戻り値が 32 以下ならエラー
        if r.0 as isize > 32 {
            Ok(())
        } else {
            Err(format!("起動できませんでした: {path}"))
        }
    }

    // ---------------------------------------------------------------- アイコン

    /// ファイルのアイコンを PNG の data URL で返す（48px、取れなければ 32px）
    pub fn icon_png(path: &str) -> Result<String, String> {
        let _com = ComGuard::new();
        let wide = HSTRING::from(path);
        unsafe {
            let mut info = SHFILEINFOW::default();
            let ok = SHGetFileInfoW(
                &wide,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_SYSICONINDEX,
            );
            let mut hicon: Option<HICON> = None;
            if ok != 0 {
                if let Ok(list) = SHGetImageList::<IImageList>(SHIL_EXTRALARGE as i32) {
                    if let Ok(h) = list.GetIcon(info.iIcon, ILD_TRANSPARENT.0) {
                        hicon = Some(h);
                    }
                }
            }
            if hicon.is_none() {
                let mut info = SHFILEINFOW::default();
                let ok = SHGetFileInfoW(
                    &wide,
                    FILE_FLAGS_AND_ATTRIBUTES(0),
                    Some(&mut info),
                    std::mem::size_of::<SHFILEINFOW>() as u32,
                    SHGFI_ICON | SHGFI_LARGEICON,
                );
                if ok != 0 && !info.hIcon.is_invalid() {
                    hicon = Some(info.hIcon);
                }
            }
            let hicon = hicon.ok_or("アイコンを取得できませんでした")?;
            let result = hicon_to_png(hicon);
            let _ = DestroyIcon(hicon);
            let png = result?;
            Ok(format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(png)
            ))
        }
    }

    unsafe fn hicon_to_png(hicon: HICON) -> Result<Vec<u8>, String> {
        let mut ii = ICONINFO::default();
        GetIconInfo(hicon, &mut ii).map_err(|e| e.to_string())?;
        let cleanup = |ii: &ICONINFO| {
            if !ii.hbmColor.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
            }
            if !ii.hbmMask.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
            }
        };
        if ii.hbmColor.is_invalid() {
            cleanup(&ii);
            return Err("モノクロアイコンには対応していません".into());
        }

        let mut bm = BITMAP::default();
        let got = GetObjectW(
            HGDIOBJ(ii.hbmColor.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut BITMAP as *mut c_void),
        );
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        if got == 0 || w <= 0 || h <= 0 || w > 512 || h > 512 {
            cleanup(&ii);
            return Err("アイコンの大きさを取得できませんでした".into());
        }

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // 上から下の順で受け取る
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let hdc = CreateCompatibleDC(None);
        let lines = GetDIBits(
            hdc,
            ii.hbmColor,
            0,
            h as u32,
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut bmi,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(hdc);
        cleanup(&ii);
        if lines == 0 {
            return Err("アイコンの画像を読み取れませんでした".into());
        }

        // BGRA → RGBA。アルファが全部 0 の古い形式のアイコンは不透明として扱う
        let has_alpha = buf.chunks_exact(4).any(|p| p[3] != 0);
        for p in buf.chunks_exact_mut(4) {
            p.swap(0, 2);
            if !has_alpha {
                p[3] = 255;
            }
        }

        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().map_err(|e| e.to_string())?;
            writer.write_image_data(&buf).map_err(|e| e.to_string())?;
        }
        Ok(out)
    }

    // ---------------------------------------------------------------- ブラウザ

    /// レジストリ（StartMenuInternet）からインストール済みブラウザを列挙する
    pub fn browsers() -> Vec<BrowserInfo> {
        use winreg::{
            enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
            RegKey,
        };
        let roots = [
            (HKEY_CURRENT_USER, r"SOFTWARE\Clients\StartMenuInternet"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\Clients\StartMenuInternet"),
            (
                HKEY_LOCAL_MACHINE,
                r"SOFTWARE\WOW6432Node\Clients\StartMenuInternet",
            ),
        ];
        let mut list: Vec<BrowserInfo> = Vec::new();
        for (hive, path) in roots {
            let Ok(root) = RegKey::predef(hive).open_subkey(path) else {
                continue;
            };
            for key in root.enum_keys().flatten() {
                if list.iter().any(|b| b.id.eq_ignore_ascii_case(&key)) {
                    continue;
                }
                let Ok(sub) = root.open_subkey(&key) else {
                    continue;
                };
                let name: String = sub.get_value("").unwrap_or_else(|_| key.clone());
                let Ok(cmd) = sub.open_subkey(r"shell\open\command") else {
                    continue;
                };
                let Ok(command): Result<String, _> = cmd.get_value("") else {
                    continue;
                };
                let Some(exe) = exe_from_command(&command) else {
                    continue;
                };
                if !Path::new(&exe).is_file() {
                    continue;
                }
                list.push(BrowserInfo { id: key, name, exe });
            }
        }
        list.sort_by_key(|b| b.name.to_lowercase());
        list
    }

    /// `"C:\...\chrome.exe" --flag` のようなコマンド文字列から exe のパスを取り出す
    fn exe_from_command(cmd: &str) -> Option<String> {
        let cmd = cmd.trim();
        if let Some(rest) = cmd.strip_prefix('"') {
            rest.split('"').next().map(str::to_string)
        } else {
            let lower = cmd.to_ascii_lowercase();
            lower.find(".exe").map(|i| cmd[..i + 4].to_string())
        }
    }

    /// 指定したブラウザで URL を開く。browser_id が空なら既定のブラウザ
    pub fn open_in_browser(browser_id: Option<&str>, url: &str) -> Result<(), String> {
        if !is_web_url(url) {
            return Err("http:// か https:// で始まる URL だけ開けます".into());
        }
        match browser_id.filter(|s| !s.is_empty()) {
            None => launch(url, None),
            Some(id) => {
                let b = browsers()
                    .into_iter()
                    .find(|b| b.id == id)
                    .ok_or("指定したブラウザが見つかりません")?;
                std::process::Command::new(&b.exe)
                    .arg(url.trim())
                    .spawn()
                    .map(|_| ())
                    .map_err(|e| format!("{} を起動できませんでした: {e}", b.name))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Windows 以外（開発時の確認用のダミー）
// ---------------------------------------------------------------------------
#[cfg(not(windows))]
mod other {
    use super::{is_web_url, BrowserInfo, VolumeInfo};
    use std::sync::Mutex;
    static VOL: Mutex<(f32, bool)> = Mutex::new((0.5, false));

    pub fn volume_get() -> Result<VolumeInfo, String> {
        let v = VOL.lock().unwrap();
        Ok(VolumeInfo {
            level: v.0,
            muted: v.1,
        })
    }
    pub fn volume_set(level: f32) -> Result<(), String> {
        VOL.lock().unwrap().0 = level.clamp(0.0, 1.0);
        Ok(())
    }
    pub fn volume_set_mute(muted: bool) -> Result<(), String> {
        VOL.lock().unwrap().1 = muted;
        Ok(())
    }
    pub fn launch(path: &str, _args: Option<&str>) -> Result<(), String> {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    pub fn icon_png(_path: &str) -> Result<String, String> {
        Err("この OS ではアイコンを取得できません".into())
    }
    pub fn browsers() -> Vec<BrowserInfo> {
        Vec::new()
    }
    pub fn open_in_browser(_id: Option<&str>, url: &str) -> Result<(), String> {
        if !is_web_url(url) {
            return Err("http:// か https:// で始まる URL だけ開けます".into());
        }
        launch(url, None)
    }
}
#[cfg(not(windows))]
pub use other::*;
