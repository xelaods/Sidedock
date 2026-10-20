//! 最近使ったファイル（Windows の「最近使った項目」フォルダのショートカットを読む）

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RecentFile {
    pub name: String,
    pub path: String,
    /// 最後に使った時刻（UNIX ミリ秒）
    pub at: u64,
    pub is_dir: bool,
}

#[cfg(windows)]
pub fn recent(limit: usize) -> Vec<RecentFile> {
    use std::{fs, path::Path, time::UNIX_EPOCH};
    use windows::{
        core::{Interface, HSTRING},
        Win32::{
            Storage::FileSystem::WIN32_FIND_DATAW,
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
            },
            UI::Shell::{IShellLinkW, ShellLink, SLGP_UNCPRIORITY},
        },
    };

    let Ok(appdata) = std::env::var("APPDATA") else {
        return Vec::new();
    };
    let dir = Path::new(&appdata).join(r"Microsoft\Windows\Recent");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut links: Vec<(u64, std::path::PathBuf)> = entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("lnk"))
        })
        .filter_map(|e| {
            let t = e
                .metadata()
                .ok()?
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_millis() as u64;
            Some((t, e.path()))
        })
        .collect();
    links.sort_by_key(|b| std::cmp::Reverse(b.0));

    let inited = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let mut out: Vec<RecentFile> = Vec::new();
    for (at, lnk) in links.into_iter().take(limit * 3) {
        if out.len() >= limit {
            break;
        }
        let target = unsafe {
            (|| -> windows::core::Result<String> {
                let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
                let pf: IPersistFile = link.cast()?;
                pf.Load(&HSTRING::from(lnk.as_os_str()), STGM_READ)?;
                let mut buf = [0u16; 1024];
                let mut fd = WIN32_FIND_DATAW::default();
                link.GetPath(&mut buf, &mut fd, SLGP_UNCPRIORITY.0 as u32)?;
                let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                Ok(String::from_utf16_lossy(&buf[..n]))
            })()
        };
        let Ok(path) = target else { continue };
        let p = Path::new(&path);
        if path.is_empty() || !p.exists() || out.iter().any(|f| f.path.eq_ignore_ascii_case(&path))
        {
            continue;
        }
        out.push(RecentFile {
            name: p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.clone()),
            is_dir: p.is_dir(),
            path,
            at,
        });
    }
    if inited {
        unsafe { CoUninitialize() };
    }
    out
}

#[cfg(not(windows))]
pub fn recent(_limit: usize) -> Vec<RecentFile> {
    Vec::new()
}

/// ファイルが今もあるか（置き場に預けたものが動かされていないか）
pub fn exist(paths: &[String]) -> Vec<bool> {
    paths
        .iter()
        .map(|p| std::path::Path::new(p).exists())
        .collect()
}

/// ファイルをつかんで、エクスプローラーや他のアプリへドラッグで渡す。
/// マウスのボタンが押されている間に呼ぶ。ドロップされるまで戻らない。
/// 戻り値は相手が受け取った方法："copy" | "move" | "link" | "none"
#[cfg(windows)]
pub fn start_drag(app: &tauri::AppHandle, paths: Vec<String>) -> Result<String, String> {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    // OLE のドラッグは、ウィンドウのある（STA の）メインスレッドで行う
    app.run_on_main_thread(move || {
        let _ = tx.send(drag_on_main(&paths));
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

#[cfg(windows)]
fn drag_on_main(paths: &[String]) -> Result<String, String> {
    use windows::{
        core::HSTRING,
        Win32::{
            System::{
                Com::IDataObject,
                Ole::{
                    IDropSource, OleInitialize, DROPEFFECT_COPY, DROPEFFECT_LINK,
                    DROPEFFECT_MOVE,
                },
            },
            UI::Shell::{Common::ITEMIDLIST, ILCreateFromPathW, ILFree, SHCreateDataObject, SHDoDragDrop},
        },
    };
    unsafe {
        let _ = OleInitialize(None);
        let pidls: Vec<*const ITEMIDLIST> = paths
            .iter()
            .filter(|p| std::path::Path::new(p).exists())
            .map(|p| ILCreateFromPathW(&HSTRING::from(p.as_str())) as *const ITEMIDLIST)
            .filter(|p| !p.is_null())
            .collect();
        if pidls.is_empty() {
            return Err("ファイルが見つかりません".into());
        }
        // 親を指定しないと、それぞれの場所を絶対位置として扱う
        let data: windows::core::Result<IDataObject> =
            SHCreateDataObject(None, Some(&pidls), None::<&IDataObject>);
        let result = data.and_then(|data| {
            // 送り元を省くと、Windows がアイコン付きの標準のドラッグを用意してくれる
            SHDoDragDrop(
                None,
                &data,
                None::<&IDropSource>,
                DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK,
            )
        });
        for p in pidls {
            ILFree(Some(p));
        }
        let effect = result.map_err(|e| e.to_string())?;
        Ok(if effect.0 & DROPEFFECT_MOVE.0 != 0 {
            "move"
        } else if effect.0 & DROPEFFECT_COPY.0 != 0 {
            "copy"
        } else if effect.0 & DROPEFFECT_LINK.0 != 0 {
            "link"
        } else {
            "none"
        }
        .into())
    }
}

#[cfg(not(windows))]
pub fn start_drag(_app: &tauri::AppHandle, _paths: Vec<String>) -> Result<String, String> {
    Err("Windows 専用です".into())
}
