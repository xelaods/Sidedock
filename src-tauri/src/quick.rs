//! クイック設定：ネットワークの状態、Wi-Fi の一覧と接続、Wi-Fi / Bluetooth のオン・オフ、
//! 画面の明るさ、ダークモード

use serde::Serialize;

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct NetStatus {
    /// "wifi" | "ethernet" | "none"
    pub kind: String,
    pub ssid: String,
    /// 電波の強さ（0〜5）
    pub bars: u8,
    /// インターネットにつながっているか
    pub internet: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetwork {
    pub ssid: String,
    /// 0〜100
    pub signal: u32,
    pub secured: bool,
    pub connected: bool,
    /// 保存済み（パスワードを入れずにつなげる）
    pub saved: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RadioInfo {
    /// "wifi" | "bluetooth"
    pub kind: String,
    pub on: bool,
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use super::{NetStatus, RadioInfo, WifiNetwork};
    use std::{
        os::windows::process::CommandExt,
        sync::atomic::{AtomicBool, Ordering},
    };
    use tauri::Emitter;
    use windows::{
        core::{w, PCWSTR},
        Devices::Radios::{Radio, RadioKind, RadioState},
        Networking::Connectivity::{
            NetworkConnectivityLevel, NetworkInformation, NetworkStatusChangedEventHandler,
        },
        Win32::{
            Devices::Display::{
                DestroyPhysicalMonitors, GetMonitorBrightness,
                GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
                SetMonitorBrightness, PHYSICAL_MONITOR,
            },
            Foundation::{HANDLE, LPARAM, POINT, WPARAM},
            Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY},
            NetworkManagement::WiFi::{
                dot11_BSS_type_any, wlan_connection_mode_profile, WlanCloseHandle, WlanConnect,
                WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList, WlanOpenHandle,
                WlanScan, WLAN_AVAILABLE_NETWORK_LIST, WLAN_CONNECTION_PARAMETERS,
                WLAN_INTERFACE_INFO_LIST,
            },
            System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED},
            UI::WindowsAndMessaging::{
                SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
            },
        },
    };

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const WLAN_AVAILABLE_NETWORK_CONNECTED: u32 = 1;
    const WLAN_AVAILABLE_NETWORK_HAS_PROFILE: u32 = 2;

    struct Mta(bool);
    impl Mta {
        fn new() -> Self {
            Mta(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
        }
    }
    impl Drop for Mta {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    // ------------------------------------------------------------ ネットワークの状態

    static WATCHING: AtomicBool = AtomicBool::new(false);

    /// 状態が変わったら "network-changed" を送るようにする（ポーリングの代わり）
    fn watch() {
        if WATCHING.swap(true, Ordering::SeqCst) {
            return;
        }
        let handler = NetworkStatusChangedEventHandler::new(|_| {
            if let Some(app) = crate::APP.get() {
                let _ = app.emit_to("main", "network-changed", ());
            }
            Ok(())
        });
        if NetworkInformation::NetworkStatusChanged(&handler).is_err() {
            WATCHING.store(false, Ordering::SeqCst);
        }
    }

    pub fn status() -> NetStatus {
        let _mta = Mta::new();
        watch();
        let Ok(p) = NetworkInformation::GetInternetConnectionProfile() else {
            return NetStatus {
                kind: "none".into(),
                ..Default::default()
            };
        };
        let internet = p
            .GetNetworkConnectivityLevel()
            .map(|l| l == NetworkConnectivityLevel::InternetAccess)
            .unwrap_or(false);
        if p.IsWlanConnectionProfile().unwrap_or(false) {
            let ssid = p
                .WlanConnectionProfileDetails()
                .and_then(|d| d.GetConnectedSsid())
                .map(|h| h.to_string())
                .unwrap_or_default();
            let bars = p.GetSignalBars().and_then(|r| r.Value()).unwrap_or(0);
            return NetStatus {
                kind: "wifi".into(),
                ssid,
                bars,
                internet,
            };
        }
        let name = p.ProfileName().map(|h| h.to_string()).unwrap_or_default();
        NetStatus {
            kind: "ethernet".into(),
            ssid: name,
            bars: 5,
            internet,
        }
    }

    // ------------------------------------------------------------ Wi-Fi の一覧と接続

    struct Wlan(HANDLE);
    impl Wlan {
        fn open() -> Result<Self, String> {
            let mut ver = 0u32;
            let mut h = HANDLE::default();
            let r = unsafe { WlanOpenHandle(2, None, &mut ver, &mut h) };
            if r != 0 {
                return Err("Wi-Fi が使えません（無線 LAN のない PC かもしれません）".into());
            }
            Ok(Wlan(h))
        }
        fn first_interface(&self) -> Result<windows::core::GUID, String> {
            unsafe {
                let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
                if WlanEnumInterfaces(self.0, None, &mut list) != 0 || list.is_null() {
                    return Err("Wi-Fi の機器が見つかりません".into());
                }
                let l = &*list;
                let guid = (l.dwNumberOfItems > 0).then(|| l.InterfaceInfo[0].InterfaceGuid);
                WlanFreeMemory(list as *const _);
                guid.ok_or_else(|| "Wi-Fi の機器が見つかりません".into())
            }
        }
    }
    impl Drop for Wlan {
        fn drop(&mut self) {
            unsafe {
                WlanCloseHandle(self.0, None);
            }
        }
    }

    fn utf16z(buf: &[u16]) -> String {
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..n])
    }

    /// 見えている Wi-Fi の一覧（強い順）。呼ぶたびに裏で探し直しを頼む
    pub fn wifi_list() -> Result<Vec<WifiNetwork>, String> {
        let w = Wlan::open()?;
        let guid = w.first_interface()?;
        let mut out: Vec<WifiNetwork> = Vec::new();
        unsafe {
            let _ = WlanScan(w.0, &guid, None, None, None);
            let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
            if WlanGetAvailableNetworkList(w.0, &guid, 0, None, &mut list) != 0 || list.is_null() {
                return Err("Wi-Fi の一覧を取得できませんでした".into());
            }
            let l = &*list;
            let nets = std::slice::from_raw_parts(l.Network.as_ptr(), l.dwNumberOfItems as usize);
            for n in nets {
                let len = (n.dot11Ssid.uSSIDLength as usize).min(32);
                let ssid = String::from_utf8_lossy(&n.dot11Ssid.ucSSID[..len]).to_string();
                if ssid.is_empty() {
                    continue;
                }
                let connected = n.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0;
                let saved = n.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0;
                if let Some(e) = out.iter_mut().find(|e| e.ssid == ssid) {
                    e.signal = e.signal.max(n.wlanSignalQuality);
                    e.connected |= connected;
                    e.saved |= saved;
                    continue;
                }
                out.push(WifiNetwork {
                    ssid,
                    signal: n.wlanSignalQuality,
                    secured: n.bSecurityEnabled.as_bool(),
                    connected,
                    saved,
                });
            }
            WlanFreeMemory(list as *const _);
        }
        out.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
        Ok(out)
    }

    /// 保存済みの Wi-Fi につなぐ。保存されていなければ Err("unsaved")
    pub fn wifi_connect(ssid: &str) -> Result<(), String> {
        let w = Wlan::open()?;
        let guid = w.first_interface()?;
        unsafe {
            let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
            if WlanGetAvailableNetworkList(w.0, &guid, 0, None, &mut list) != 0 || list.is_null() {
                return Err("Wi-Fi の一覧を取得できませんでした".into());
            }
            let l = &*list;
            let nets = std::slice::from_raw_parts(l.Network.as_ptr(), l.dwNumberOfItems as usize);
            let profile = nets
                .iter()
                .find(|n| {
                    let len = (n.dot11Ssid.uSSIDLength as usize).min(32);
                    String::from_utf8_lossy(&n.dot11Ssid.ucSSID[..len]) == ssid
                        && n.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0
                })
                .map(|n| utf16z(&n.strProfileName));
            WlanFreeMemory(list as *const _);
            let Some(profile) = profile else {
                return Err("unsaved".into());
            };
            let wide: Vec<u16> = profile.encode_utf16().chain(std::iter::once(0)).collect();
            let params = WLAN_CONNECTION_PARAMETERS {
                wlanConnectionMode: wlan_connection_mode_profile,
                strProfile: PCWSTR(wide.as_ptr()),
                pDot11Ssid: std::ptr::null_mut(),
                pDesiredBssidList: std::ptr::null_mut(),
                dot11BssType: dot11_BSS_type_any,
                dwFlags: 0,
            };
            let r = WlanConnect(w.0, &guid, &params, None);
            if r != 0 {
                return Err(format!("{ssid} につなげませんでした"));
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ Wi-Fi / Bluetooth のオン・オフ

    fn kind_name(k: RadioKind) -> Option<&'static str> {
        if k == RadioKind::WiFi {
            Some("wifi")
        } else if k == RadioKind::Bluetooth {
            Some("bluetooth")
        } else {
            None
        }
    }

    pub fn radios() -> Vec<RadioInfo> {
        let _mta = Mta::new();
        let _ = Radio::RequestAccessAsync().and_then(|o| o.join());
        let Ok(list) = Radio::GetRadiosAsync().and_then(|o| o.join()) else {
            return Vec::new();
        };
        let mut out: Vec<RadioInfo> = Vec::new();
        for r in list {
            let Some(kind) = r.Kind().ok().and_then(kind_name) else {
                continue;
            };
            if out.iter().any(|x| x.kind == kind) {
                continue;
            }
            out.push(RadioInfo {
                kind: kind.into(),
                on: r.State().map(|s| s == RadioState::On).unwrap_or(false),
            });
        }
        out
    }

    pub fn radio_set(kind: &str, on: bool) -> Result<(), String> {
        let _mta = Mta::new();
        let _ = Radio::RequestAccessAsync().and_then(|o| o.join());
        let list = Radio::GetRadiosAsync()
            .and_then(|o| o.join())
            .map_err(|e| e.message().to_string())?;
        let mut done = false;
        for r in list {
            if r.Kind().ok().and_then(kind_name) != Some(kind) {
                continue;
            }
            let state = if on { RadioState::On } else { RadioState::Off };
            r.SetStateAsync(state)
                .and_then(|o| o.join())
                .map_err(|e| format!("切り替えられませんでした: {}", e.message()))?;
            done = true;
        }
        if done {
            Ok(())
        } else {
            Err("この PC にはありません".into())
        }
    }

    // ------------------------------------------------------------ 画面の明るさ

    /// 外部モニターは DDC/CI、ノート PC の内蔵画面は WMI で
    fn with_monitor<T>(f: impl FnOnce(HANDLE) -> Option<T>) -> Option<T> {
        unsafe {
            let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut n = 0u32;
            if GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut n).is_err() || n == 0 {
                return None;
            }
            let mut arr = vec![PHYSICAL_MONITOR::default(); n as usize];
            if GetPhysicalMonitorsFromHMONITOR(hmon, &mut arr).is_err() {
                return None;
            }
            let r = f(arr[0].hPhysicalMonitor);
            let _ = DestroyPhysicalMonitors(&arr);
            r
        }
    }

    fn wmi(script: &str) -> Option<String> {
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// 0〜100。変えられない画面なら None
    pub fn brightness_get() -> Option<u32> {
        let ddc = with_monitor(|h| unsafe {
            let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
            (GetMonitorBrightness(h, &mut min, &mut cur, &mut max) != 0 && max > min)
                .then(|| ((cur - min) * 100) / (max - min))
        });
        if ddc.is_some() {
            return ddc;
        }
        wmi("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness -ErrorAction Stop | Select-Object -First 1).CurrentBrightness")
            .and_then(|s| s.parse().ok())
    }

    pub fn brightness_set(value: u32) -> Result<(), String> {
        let v = value.min(100);
        let ddc = with_monitor(|h| unsafe {
            let (mut min, mut cur, mut max) = (0u32, 0u32, 0u32);
            if GetMonitorBrightness(h, &mut min, &mut cur, &mut max) == 0 || max <= min {
                return None;
            }
            (SetMonitorBrightness(h, min + (max - min) * v / 100) != 0).then_some(())
        });
        if ddc.is_some() {
            return Ok(());
        }
        wmi(&format!(
            "Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods -ErrorAction Stop | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Timeout=0;Brightness={v}}} | Out-Null"
        ))
        .map(|_| ())
        .ok_or_else(|| "この画面は明るさを変えられません".into())
    }

    // ------------------------------------------------------------ ダークモード

    const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

    pub fn dark_get() -> bool {
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(PERSONALIZE)
            .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
            .map(|v| v == 0)
            .unwrap_or(false)
    }

    /// アプリとタスクバーなどの色をまとめて切り替え、全体に知らせる
    pub fn dark_set(dark: bool) -> Result<(), String> {
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(PERSONALIZE)
            .map_err(|e| e.to_string())?;
        let v: u32 = if dark { 0 } else { 1 };
        key.set_value("AppsUseLightTheme", &v)
            .map_err(|e| e.to_string())?;
        key.set_value("SystemUsesLightTheme", &v)
            .map_err(|e| e.to_string())?;
        std::thread::spawn(|| unsafe {
            let mut r = 0usize;
            let _ = SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(w!("ImmersiveColorSet").as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                200,
                Some(&mut r),
            );
        });
        Ok(())
    }
}

#[cfg(not(windows))]
mod other {
    use super::{NetStatus, RadioInfo, WifiNetwork};
    pub fn status() -> NetStatus {
        NetStatus {
            kind: "ethernet".into(),
            ssid: String::new(),
            bars: 5,
            internet: true,
        }
    }
    pub fn wifi_list() -> Result<Vec<WifiNetwork>, String> {
        Ok(Vec::new())
    }
    pub fn wifi_connect(_ssid: &str) -> Result<(), String> {
        Err("この OS では使えません".into())
    }
    pub fn radios() -> Vec<RadioInfo> {
        Vec::new()
    }
    pub fn radio_set(_kind: &str, _on: bool) -> Result<(), String> {
        Err("この OS では使えません".into())
    }
    pub fn brightness_get() -> Option<u32> {
        None
    }
    pub fn brightness_set(_v: u32) -> Result<(), String> {
        Err("この OS では使えません".into())
    }
    pub fn dark_get() -> bool {
        false
    }
    pub fn dark_set(_dark: bool) -> Result<(), String> {
        Ok(())
    }
}
#[cfg(not(windows))]
pub use other::*;
