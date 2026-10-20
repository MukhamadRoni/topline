//! Utilitas Windows: jam lokal, folder data, autostart (registry HKCU\...\Run), ikon.

use std::path::PathBuf;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::SystemInformation::{GetLocalTime, GetTickCount};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::*;

use topline_core::Clock;

pub fn now() -> Clock {
    let st = unsafe { GetLocalTime() };
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Clock {
        ms,
        year: st.wYear as i32,
        month: st.wMonth as u32,
        day: st.wDay as u32,
        hour: st.wHour as u32,
        minute: st.wMinute as u32,
        second: st.wSecond as u32,
        weekday: st.wDayOfWeek as u32,
    }
}

/// Waktu input pengguna terakhir dalam satuan `Clock::ms` (GetLastInputInfo).
pub fn last_input_ms(now: &Clock) -> i64 {
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if !GetLastInputInfo(&mut li).as_bool() {
            return now.ms;
        }
        now.ms - GetTickCount().wrapping_sub(li.dwTime) as i64
    }
}

/// Pengaturan Windows "Animation effects" (SPI_GETCLIENTAREAANIMATION).
pub fn animations_enabled() -> bool {
    let mut on = TRUE;
    unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut on as *mut BOOL as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
            .map_or(true, |_| on.as_bool())
    }
}

/// Mode log diagnostik aktif (file `debug` ada di folder data).
pub fn debug_enabled() -> bool {
    data_dir().join("debug").exists()
}

/// Demo ekspresi mata (TOPLINE_EYE_DEMO=1): hanya untuk build debug atau mode log diagnostik.
pub fn eye_demo_enabled() -> bool {
    std::env::var_os("TOPLINE_EYE_DEMO").map_or(false, |v| v == "1") && (cfg!(debug_assertions) || debug_enabled())
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Topline")
}

/// Log diagnostik ke %LOCALAPPDATA%\Topline\debug.log (hanya jika file `debug` ada di folder data).
pub fn log(msg: &str) {
    use std::io::Write;
    if !debug_enabled() {
        return;
    }
    let dir = data_dir();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("debug.log")) {
        let c = now();
        let _ = writeln!(f, "{:02}:{:02}:{:02}.{:03} {}", c.hour, c.minute, c.second, c.ms % 1000, msg);
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Topline");

pub fn autostart_enabled() -> bool {
    unsafe {
        let mut size = 0u32;
        RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, RUN_VALUE, RRF_RT_REG_SZ, None, None, Some(&mut size)).is_ok()
    }
}

pub fn set_autostart(on: bool) {
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .is_err()
        {
            return;
        }
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let v = wide(&format!("\"{}\"", exe.display()));
                let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
                let _ = RegSetValueExW(key, RUN_VALUE, 0, REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(key, RUN_VALUE);
        }
        let _ = RegCloseKey(key);
    }
}

/// Ikon dari file .ico yang dibundel (ukuran terdekat).
pub fn icon_from_ico(bytes: &[u8], size: i32) -> HICON {
    unsafe {
        let off = LookupIconIdFromDirectoryEx(bytes.as_ptr(), TRUE, size, size, LR_DEFAULTCOLOR);
        if off <= 0 || off as usize >= bytes.len() {
            return LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
        }
        CreateIconFromResourceEx(&bytes[off as usize..], TRUE, 0x0003_0000, size, size, LR_DEFAULTCOLOR)
            .unwrap_or_else(|_| LoadIconW(None, IDI_APPLICATION).unwrap_or_default())
    }
}
