//! Clipboard: pendengar perubahan (WM_CLIPBOARDUPDATE), baca teks, tulis teks.
//! Teks dari pengelola sandi dan aplikasi yang menandai isinya rahasia diabaikan.

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::CF_UNICODETEXT;

/// Aktif/nonaktifkan WM_CLIPBOARDUPDATE untuk jendela ini.
pub fn listen(hwnd: HWND, on: bool) {
    unsafe {
        let _ = if on { AddClipboardFormatListener(hwnd) } else { RemoveClipboardFormatListener(hwnd) };
    }
}

/// Format yang menandai isi clipboard tidak boleh dicatat (konvensi Windows /
/// pengelola sandi seperti KeePass, 1Password, Bitwarden).
fn is_private() -> bool {
    unsafe {
        for name in [w!("ExcludeClipboardContentFromMonitorProcessing"), w!("Clipboard Viewer Ignore")] {
            let f = RegisterClipboardFormatW(name);
            if f != 0 && IsClipboardFormatAvailable(f).is_ok() {
                return true;
            }
        }
        // "CanIncludeInClipboardHistory" = 0 berarti jangan masuk riwayat.
        let f = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
        if f != 0 && IsClipboardFormatAvailable(f).is_ok() {
            if let Ok(h) = GetClipboardData(f) {
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u32;
                let allow = p.is_null() || *p != 0;
                let _ = GlobalUnlock(g);
                if !allow {
                    return true;
                }
            }
        }
        false
    }
}

/// Teks saat ini di clipboard, atau None jika bukan teks / rahasia / sedang dikunci.
pub fn read_text(hwnd: HWND) -> Option<String> {
    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT.0 as u32).is_err() {
            return None;
        }
        // Pendengar clipboard lain bisa sedang membukanya; coba sebentar lagi.
        let mut opened = false;
        for _ in 0..5 {
            if OpenClipboard(hwnd).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        if !opened {
            return None;
        }
        let text = (|| {
            if is_private() {
                return None;
            }
            let h = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let max = GlobalSize(g) / 2;
            let len = (0..max).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            let _ = GlobalUnlock(g);
            Some(s)
        })();
        let _ = CloseClipboard();
        text
    }
}

/// Tulis teks ke clipboard.
pub fn set_text(hwnd: HWND, text: &str) {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        if !(0..5).any(|i| {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            OpenClipboard(hwnd).is_ok()
        }) {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
            let p = GlobalLock(g) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                let _ = GlobalUnlock(g);
                // Setelah berhasil, memori dimiliki sistem.
                let _ = SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(g.0));
            }
        }
        let _ = CloseClipboard();
    }
}
