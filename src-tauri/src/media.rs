//! 再生中のメディアの情報と操作（Windows の「システムメディアトランスポートコントロール」）
//! Spotify・Apple Music・ブラウザの YouTube など、Windows の音量フライアウトに出るものが対象

use serde::Serialize;

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// 再生しているアプリの ID（例 Spotify.exe、AppleInc.AppleMusicWin_...）
    pub app: String,
    /// "playing" | "paused" | "stopped" | "other"
    pub status: String,
    /// 再生位置と長さ（秒）。分からないときは 0
    pub position: f64,
    pub duration: f64,
    pub can_seek: bool,
    pub can_next: bool,
    pub can_prev: bool,
    /// ジャケット画像が変わったかを見分けるための値
    pub art_key: String,
}

#[cfg(windows)]
mod win {
    use super::MediaInfo;
    use base64::Engine;
    use std::{
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };
    use windows::{
        Foundation::TypedEventHandler,
        Media::Control::{
            GlobalSystemMediaTransportControlsSession as Session,
            GlobalSystemMediaTransportControlsSessionManager as Manager,
            GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
        },
        Storage::Streams::DataReader,
        Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED},
    };

    /// WinRT を使う間だけ、このスレッドを MTA で初期化する
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

    /// マネージャーの取得は重いので一度だけ
    static MANAGER: Mutex<Option<Manager>> = Mutex::new(None);
    /// 今の曲のジャケット（art_key と data URL）
    static ART: Mutex<Option<(String, Option<String>)>> = Mutex::new(None);

    fn manager() -> windows::core::Result<Manager> {
        let mut m = MANAGER.lock().unwrap();
        if let Some(m) = m.as_ref() {
            return Ok(m.clone());
        }
        let mgr = Manager::RequestAsync()?.join()?;
        *m = Some(mgr.clone());
        drop(m);
        watch(&mgr);
        Ok(mgr)
    }

    /// 曲や再生状態が変わったら "media-changed" を送る（ポーリングの代わり）
    fn changed() {
        if let Some(app) = crate::APP.get() {
            use tauri::Emitter;
            let _ = app.emit_to("main", "media-changed", ());
        }
    }

    /// 監視しているセッション（再生しているアプリ）
    static WATCHED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn watch_sessions(mgr: &Manager) {
        let Ok(list) = mgr.GetSessions() else { return };
        let mut watched = WATCHED.lock().unwrap();
        for s in list {
            let id = s
                .SourceAppUserModelId()
                .map(|h| h.to_string())
                .unwrap_or_default();
            if watched.contains(&id) {
                continue;
            }
            let _ = s.MediaPropertiesChanged(&TypedEventHandler::new(|_, _| {
                changed();
                Ok(())
            }));
            let _ = s.PlaybackInfoChanged(&TypedEventHandler::new(|_, _| {
                changed();
                Ok(())
            }));
            watched.push(id);
        }
    }

    fn watch(mgr: &Manager) {
        let _ = mgr.CurrentSessionChanged(&TypedEventHandler::new(|_, _| {
            changed();
            Ok(())
        }));
        let _ = mgr.SessionsChanged(&TypedEventHandler::new(
            |m: windows::core::Ref<Manager>, _| {
                if let Some(m) = m.as_ref() {
                    watch_sessions(m);
                }
                changed();
                Ok(())
            },
        ));
        watch_sessions(mgr);
    }

    fn session() -> Result<Option<Session>, String> {
        let mgr =
            manager().map_err(|e| format!("メディアの情報を取得できません: {}", e.message()))?;
        Ok(mgr.GetCurrentSession().ok())
    }

    /// WinRT の DateTime（1601 年からの 100ns 単位）→ UNIX 秒
    fn winrt_to_unix(ticks: i64) -> f64 {
        (ticks as f64 / 10_000_000.0) - 11_644_473_600.0
    }

    fn key_of(title: &str, artist: &str, album: &str) -> String {
        format!("{title}\u{1f}{artist}\u{1f}{album}")
    }

    pub fn get() -> Result<Option<MediaInfo>, String> {
        let _mta = Mta::new();
        let Some(s) = session()? else { return Ok(None) };
        let mut info = MediaInfo {
            app: s
                .SourceAppUserModelId()
                .map(|h| h.to_string())
                .unwrap_or_default(),
            ..Default::default()
        };
        if let Ok(p) = s.TryGetMediaPropertiesAsync().and_then(|op| op.join()) {
            info.title = p.Title().map(|h| h.to_string()).unwrap_or_default();
            info.artist = p.Artist().map(|h| h.to_string()).unwrap_or_default();
            info.album = p.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
        }
        info.art_key = key_of(&info.title, &info.artist, &info.album);

        if let Ok(pb) = s.GetPlaybackInfo() {
            info.status = match pb.PlaybackStatus() {
                Ok(Status::Playing) => "playing",
                Ok(Status::Paused) => "paused",
                Ok(Status::Stopped) | Ok(Status::Closed) => "stopped",
                _ => "other",
            }
            .into();
            if let Ok(c) = pb.Controls() {
                info.can_next = c.IsNextEnabled().unwrap_or(false);
                info.can_prev = c.IsPreviousEnabled().unwrap_or(false);
                info.can_seek = c.IsPlaybackPositionEnabled().unwrap_or(false);
            }
        }
        if let Ok(t) = s.GetTimelineProperties() {
            let secs = |r: windows::core::Result<windows::Foundation::TimeSpan>| {
                r.map(|d| d.Duration as f64 / 10_000_000.0).unwrap_or(0.0)
            };
            let start = secs(t.StartTime());
            let end = secs(t.EndTime());
            let mut pos = secs(t.Position()) - start;
            // 位置は時々しか更新されないので、再生中なら最後に更新されてからの経過を足す
            if info.status == "playing" {
                if let Ok(updated) = t.LastUpdatedTime() {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs_f64())
                        .unwrap_or(0.0);
                    let elapsed = now - winrt_to_unix(updated.UniversalTime);
                    if (0.0..6.0 * 3600.0).contains(&elapsed) {
                        pos += elapsed;
                    }
                }
            }
            info.duration = (end - start).max(0.0);
            info.position = if info.duration > 0.0 {
                pos.clamp(0.0, info.duration)
            } else {
                0.0
            };
        }
        Ok(Some(info))
    }

    /// 今の曲のジャケットを data URL で（なければ None）
    pub fn art() -> Result<Option<String>, String> {
        let _mta = Mta::new();
        let Some(s) = session()? else { return Ok(None) };
        let Ok(p) = s.TryGetMediaPropertiesAsync().and_then(|op| op.join()) else {
            return Ok(None);
        };
        let key = key_of(
            &p.Title().map(|h| h.to_string()).unwrap_or_default(),
            &p.Artist().map(|h| h.to_string()).unwrap_or_default(),
            &p.AlbumTitle().map(|h| h.to_string()).unwrap_or_default(),
        );
        if let Some((k, url)) = ART.lock().unwrap().as_ref() {
            if *k == key {
                return Ok(url.clone());
            }
        }
        let url = (|| -> windows::core::Result<Option<String>> {
            let Ok(thumb) = p.Thumbnail() else {
                return Ok(None);
            };
            let stream = thumb.OpenReadAsync()?.join()?;
            let size = stream.Size()? as u32;
            if size == 0 || size > 8 * 1024 * 1024 {
                return Ok(None);
            }
            let mime = stream
                .ContentType()
                .map(|h| h.to_string())
                .unwrap_or_default();
            let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
            reader.LoadAsync(size)?.join()?;
            let mut buf = vec![0u8; size as usize];
            reader.ReadBytes(&mut buf)?;
            let mime = if mime.starts_with("image/") {
                mime
            } else {
                "image/png".into()
            };
            Ok(Some(format!(
                "data:{mime};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(buf)
            )))
        })()
        .unwrap_or(None);
        *ART.lock().unwrap() = Some((key, url.clone()));
        Ok(url)
    }

    pub fn control(action: &str) -> Result<(), String> {
        let _mta = Mta::new();
        let s = session()?.ok_or("再生中のメディアがありません")?;
        let op = match action {
            "toggle" => s.TryTogglePlayPauseAsync(),
            "play" => s.TryPlayAsync(),
            "pause" => s.TryPauseAsync(),
            "next" => s.TrySkipNextAsync(),
            "prev" => s.TrySkipPreviousAsync(),
            _ => return Err(format!("不明な操作です: {action}")),
        };
        op.and_then(|o| o.join())
            .map(|_| ())
            .map_err(|e| format!("操作できませんでした: {}", e.message()))
    }

    pub fn seek(seconds: f64) -> Result<(), String> {
        let _mta = Mta::new();
        let s = session()?.ok_or("再生中のメディアがありません")?;
        let start = s
            .GetTimelineProperties()
            .and_then(|t| t.StartTime())
            .map(|d| d.Duration)
            .unwrap_or(0);
        let ticks = start + (seconds.max(0.0) * 10_000_000.0) as i64;
        s.TryChangePlaybackPositionAsync(ticks)
            .and_then(|o| o.join())
            .map(|_| ())
            .map_err(|e| format!("再生位置を変えられませんでした: {}", e.message()))
    }
}

#[cfg(windows)]
pub use win::{art, control, get, seek};

#[cfg(not(windows))]
pub fn get() -> Result<Option<MediaInfo>, String> {
    Ok(None)
}
#[cfg(not(windows))]
pub fn art() -> Result<Option<String>, String> {
    Ok(None)
}
#[cfg(not(windows))]
pub fn control(_action: &str) -> Result<(), String> {
    Err("この OS では使えません".into())
}
#[cfg(not(windows))]
pub fn seek(_seconds: f64) -> Result<(), String> {
    Err("この OS では使えません".into())
}
