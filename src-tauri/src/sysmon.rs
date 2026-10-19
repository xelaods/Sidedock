//! CPU・メモリ・ディスク・バッテリーの状態と、電源の操作

use serde::Serialize;
use std::sync::Mutex;
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    /// CPU 使用率（0〜100）
    pub cpu: f32,
    pub cpu_name: String,
    pub cores: usize,
    pub mem_used: u64,
    pub mem_total: u64,
    /// システムドライブ（C:）
    pub disk_used: u64,
    pub disk_total: u64,
    /// バッテリー残量（0〜100）。バッテリーがなければ None
    pub battery: Option<u8>,
    pub charging: bool,
    /// 起動してからの秒数
    pub uptime: u64,
}

static SYS: Mutex<Option<System>> = Mutex::new(None);

pub fn stats() -> Stats {
    let mut guard = SYS.lock().unwrap();
    let sys = guard.get_or_insert_with(|| {
        System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                .with_memory(MemoryRefreshKind::nothing().with_ram()),
        )
    });
    // CPU 使用率は前回からの差で出るので、呼ぶたびに更新する（1 回目は 0 になる）
    sys.refresh_cpu_usage();
    sys.refresh_memory();

    let cpus = sys.cpus();
    let cpu_name = cpus
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_default();

    let disks = Disks::new_with_refreshed_list();
    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let disk = disks
        .list()
        .iter()
        .find(|d| {
            d.mount_point()
                .to_string_lossy()
                .to_uppercase()
                .starts_with(&system_drive.to_uppercase())
        })
        .or_else(|| disks.list().first());

    let (battery, charging) = battery();
    Stats {
        cpu: sys.global_cpu_usage(),
        cpu_name,
        cores: cpus.len(),
        mem_used: sys.used_memory(),
        mem_total: sys.total_memory(),
        disk_used: disk
            .map(|d| d.total_space() - d.available_space())
            .unwrap_or(0),
        disk_total: disk.map(|d| d.total_space()).unwrap_or(0),
        battery,
        charging,
        uptime: System::uptime(),
    }
}

#[cfg(windows)]
fn battery() -> (Option<u8>, bool) {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return (None, false);
    }
    // 128 はバッテリーなし、255 は不明
    if s.BatteryFlag & 128 != 0 || s.BatteryLifePercent > 100 {
        return (None, s.ACLineStatus == 1);
    }
    (Some(s.BatteryLifePercent), s.ACLineStatus == 1)
}

#[cfg(not(windows))]
fn battery() -> (Option<u8>, bool) {
    (None, false)
}

/// 電源の操作："lock" | "sleep" | "restart" | "shutdown" | "signout"
pub fn power(action: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let run = |args: &[&str]| {
            std::process::Command::new("shutdown")
                .args(args)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        };
        match action {
            "lock" => unsafe { windows::Win32::System::Shutdown::LockWorkStation() }
                .map_err(|e| e.message().to_string()),
            "sleep" => {
                // 休止状態ではなくスリープ（1 つ目の false）
                let ok =
                    unsafe { windows::Win32::System::Power::SetSuspendState(false, false, false) };
                if ok {
                    Ok(())
                } else {
                    Err("スリープできませんでした".into())
                }
            }
            "restart" => run(&["/r", "/t", "0"]),
            "shutdown" => run(&["/s", "/t", "0"]),
            "signout" => run(&["/l"]),
            _ => Err(format!("不明な操作です: {action}")),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = action;
        Err("この OS では使えません".into())
    }
}
