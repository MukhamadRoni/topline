//! Pengamat tanpa polling, masing-masing di thread sendiri yang tidur sampai
//! Windows membangunkannya:
//! - kamera/mikrofon: `RegNotifyChangeKeyValue` pada ConsentStore
//!   (CapabilityAccessManager). Nilai `LastUsedTimeStop == 0` = sedang dipakai.
//! - unduhan selesai: `ReadDirectoryChangesW` pada folder Downloads; file
//!   sementara (.crdownload/.part/...) yang di-rename ke nama akhir = selesai.
//! - lokasi saat ini (sekali jalan, atas permintaan): `Geolocator` WinRT.

use std::path::PathBuf;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Storage::FileSystem::*;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use topline_core::GeoPoint;
use windows::Devices::Geolocation::Geolocator;
use windows::Foundation::TimeSpan;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};

use crate::{WM_APP_DOWNLOAD, WM_APP_LOCATION, WM_APP_PRIVACY};

const STORE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore";

pub struct Watchers {
    stop: HANDLE,
}

impl Watchers {
    pub fn start(hwnd: HWND) -> Self {
        let stop = unsafe { CreateEventW(None, TRUE, FALSE, None).unwrap_or_default() };
        let (raw, stop_raw) = (hwnd.0 as isize, stop.0 as isize);
        let _ = std::thread::Builder::new().name("privacy".into()).spawn(move || privacy_thread(raw, stop_raw));
        if let Some(dir) = downloads_dir() {
            let _ = std::thread::Builder::new().name("downloads".into()).spawn(move || downloads_thread(raw, dir));
        }
        Self { stop }
    }

    pub fn stop(&mut self) {
        if !self.stop.is_invalid() {
            unsafe {
                let _ = SetEvent(self.stop);
            }
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_key(path: &str, access: REG_SAM_FLAGS) -> Option<HKEY> {
    let p = wide(path);
    let mut k = HKEY::default();
    unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(p.as_ptr()), 0, access, &mut k).is_ok().then_some(k) }
}

// ---------------------------------------------------------------- kamera/mikrofon

fn privacy_thread(hwnd: isize, stop: isize) {
    let keys: Vec<HKEY> = ["webcam", "microphone"]
        .iter()
        .filter_map(|n| open_key(&format!("{STORE}\\{n}"), KEY_NOTIFY))
        .collect();
    if keys.is_empty() {
        return;
    }
    unsafe {
        let events: Vec<HANDLE> = keys.iter().filter_map(|_| CreateEventW(None, FALSE, FALSE, None).ok()).collect();
        let mut waits = events.clone();
        waits.push(HANDLE(stop as *mut _));
        loop {
            for (k, e) in keys.iter().zip(&events) {
                let _ = RegNotifyChangeKeyValue(*k, TRUE, REG_NOTIFY_CHANGE_LAST_SET | REG_NOTIFY_CHANGE_NAME, *e, TRUE);
            }
            let r = WaitForMultipleObjects(&waits, FALSE, INFINITE);
            let i = r.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            if i >= events.len() {
                break; // stop, atau gagal
            }
            let _ = PostMessageW(HWND(hwnd as *mut _), WM_APP_PRIVACY, WPARAM(0), LPARAM(0));
        }
        for e in events {
            let _ = CloseHandle(e);
        }
        for k in keys {
            let _ = RegCloseKey(k);
        }
    }
}

fn subkeys(k: HKEY) -> Vec<Vec<u16>> {
    let mut out = Vec::new();
    for i in 0.. {
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let r = unsafe { RegEnumKeyExW(k, i, PWSTR(buf.as_mut_ptr()), &mut len, None, PWSTR::null(), None, None) };
        if r.is_err() {
            break;
        }
        out.push(buf[..len as usize].iter().copied().chain(std::iter::once(0)).collect());
    }
    out
}

fn qword(k: HKEY, sub: &[u16], name: PCWSTR) -> Option<u64> {
    let mut v = 0u64;
    let mut size = 8u32;
    unsafe {
        RegGetValueW(k, PCWSTR(sub.as_ptr()), name, RRF_RT_REG_QWORD, None, Some(&mut v as *mut u64 as *mut _), Some(&mut size))
            .is_ok()
            .then_some(v)
    }
}

fn in_use(k: HKEY, sub: &[u16]) -> bool {
    qword(k, sub, w!("LastUsedTimeStop")) == Some(0) && qword(k, sub, w!("LastUsedTimeStart")).map_or(false, |v| v != 0)
}

/// Nama aplikasi yang mudah dibaca dari nama subkey ConsentStore:
/// "Microsoft.WindowsCamera_8wekyb3d8bbwe" -> "WindowsCamera",
/// "C:#Program Files#Zoom#bin#Zoom.exe" -> "Zoom".
fn app_name(key: &[u16], packaged: bool) -> String {
    let s = String::from_utf16_lossy(&key[..key.len().saturating_sub(1)]);
    if packaged {
        let base = s.split('_').next().unwrap_or(&s);
        base.rsplit('.').next().unwrap_or(base).to_string()
    } else {
        let file = s.rsplit('#').next().unwrap_or(&s);
        file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file).to_string()
    }
}

fn scan_store(name: &str) -> Option<String> {
    let k = open_key(&format!("{STORE}\\{name}"), KEY_READ)?;
    let mut found = None;
    for sub in subkeys(k) {
        if String::from_utf16_lossy(&sub).trim_end_matches('\0') == "NonPackaged" {
            if let Some(np) = open_key(&format!("{STORE}\\{name}\\NonPackaged"), KEY_READ) {
                found = subkeys(np).into_iter().find(|s| in_use(np, s)).map(|s| app_name(&s, false));
                unsafe {
                    let _ = RegCloseKey(np);
                }
            }
        } else if in_use(k, &sub) {
            found = Some(app_name(&sub, true));
        }
        if found.is_some() {
            break;
        }
    }
    unsafe {
        let _ = RegCloseKey(k);
    }
    found
}

/// (aplikasi yang memakai kamera, aplikasi yang memakai mikrofon)
pub fn privacy_scan() -> (Option<String>, Option<String>) {
    (scan_store("webcam"), scan_store("microphone"))
}

// ---------------------------------------------------------------- lokasi

/// Minta lokasi sekali dari layanan lokasi Windows (butuh Settings > Privacy &
/// security > Location > "Let desktop apps access your location"). Hasilnya dikirim
/// lewat WM_APP_LOCATION; zona waktu diambil dari pengaturan jam Windows.
pub fn locate(hwnd: HWND) {
    let raw = hwnd.0 as isize;
    let _ = std::thread::Builder::new().name("location".into()).spawn(move || {
        let point = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let p = (|| -> Result<GeoPoint> {
                let geo = Geolocator::new()?;
                let ts = |s: i64| TimeSpan { Duration: s * 10_000_000 };
                let pos = geo.GetGeopositionAsyncWithAgeAndTimeout(ts(600), ts(20))?.get()?;
                let b = pos.Coordinate()?.Point()?.Position()?;
                let mut tzi = TIME_ZONE_INFORMATION::default();
                GetTimeZoneInformation(&mut tzi);
                Ok(GeoPoint { lat: b.Latitude, lon: b.Longitude, tz: -(tzi.Bias as f64) / 60.0 })
            })()
            .ok();
            CoUninitialize();
            p
        };
        let lp = point.map_or(0, |p| Box::into_raw(Box::new(p)) as isize);
        unsafe {
            if PostMessageW(HWND(raw as *mut _), WM_APP_LOCATION, WPARAM(0), LPARAM(lp)).is_err() && lp != 0 {
                drop(Box::from_raw(lp as *mut GeoPoint));
            }
        }
    });
}

// ---------------------------------------------------------------- unduhan

pub fn downloads_dir() -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

fn is_partial(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [".crdownload", ".part", ".partial", ".download", ".opdownload"].iter().any(|e| n.ends_with(e))
}

fn downloads_thread(hwnd: isize, dir: PathBuf) {
    let path = wide(&dir.display().to_string());
    let Ok(h) = (unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }) else {
        return;
    };
    // u32 agar FILE_NOTIFY_INFORMATION selalu sejajar 4 byte.
    let mut buf = vec![0u32; 4096];
    let mut old_name = String::new();
    loop {
        let mut got = 0u32;
        let ok = unsafe {
            ReadDirectoryChangesW(h, buf.as_mut_ptr() as *mut _, (buf.len() * 4) as u32, FALSE, FILE_NOTIFY_CHANGE_FILE_NAME, Some(&mut got), None, None)
        };
        if ok.is_err() {
            break;
        }
        let base = buf.as_ptr() as *const u8;
        let mut off = 0usize;
        while got > 0 && off < got as usize {
            let info = unsafe { &*(base.add(off) as *const FILE_NOTIFY_INFORMATION) };
            let name = unsafe {
                let p = std::ptr::addr_of!(info.FileName) as *const u16;
                String::from_utf16_lossy(std::slice::from_raw_parts(p, info.FileNameLength as usize / 2))
            };
            if info.Action == FILE_ACTION_RENAMED_OLD_NAME {
                old_name = name;
            } else if info.Action == FILE_ACTION_RENAMED_NEW_NAME {
                if is_partial(&old_name) && !is_partial(&name) {
                    let boxed = Box::into_raw(Box::new(name));
                    unsafe {
                        if PostMessageW(HWND(hwnd as *mut _), WM_APP_DOWNLOAD, WPARAM(0), LPARAM(boxed as isize)).is_err() {
                            drop(Box::from_raw(boxed));
                        }
                    }
                }
                old_name.clear();
            }
            if info.NextEntryOffset == 0 {
                break;
            }
            off += info.NextEntryOffset as usize;
        }
    }
    unsafe {
        let _ = CloseHandle(h);
    }
}
