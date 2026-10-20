//! Input to-do inline: jendela kecil tanpa bingkai berisi kontrol EDIT standar
//! Windows, diletakkan tepat di atas baris "tugas baru" di dalam notch.
//! Notch sendiri tidak pernah mengambil fokus keyboard; hanya jendela ini.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::embedded;
use crate::theme::EDIT_FILL_RGB;
use crate::{WM_APP_EDIT_BLUR, WM_APP_EDIT_CANCEL, WM_APP_EDIT_SUBMIT};

static NOTCH: AtomicIsize = AtomicIsize::new(0);
static BRUSH: AtomicIsize = AtomicIsize::new(0);

pub struct EditBox {
    host: HWND,
    edit: HWND,
    font: HFONT,
    font_px: i32,
    pub visible: bool,
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wp.0 as *mut _);
            SetTextColor(hdc, rgb(0xF2, 0xF6, 0xFF));
            let (r, g, b) = EDIT_FILL_RGB;
            SetBkColor(hdc, rgb(r, g, b));
            LRESULT(BRUSH.load(Ordering::Relaxed))
        }
        WM_ERASEBKGND => {
            let hdc = HDC(wp.0 as *mut _);
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            FillRect(hdc, &rc, HBRUSH(BRUSH.load(Ordering::Relaxed) as *mut _));
            LRESULT(1)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_ACTIVATE as isize),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    let notch = HWND(NOTCH.load(Ordering::Relaxed) as *mut _);
    match msg {
        WM_KEYDOWN if wp.0 as u16 == VK_RETURN.0 => {
            let _ = PostMessageW(notch, WM_APP_EDIT_SUBMIT, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        WM_KEYDOWN if wp.0 as u16 == VK_ESCAPE.0 => {
            let _ = PostMessageW(notch, WM_APP_EDIT_CANCEL, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        // Cegah bunyi "ding" untuk Enter/Esc di EDIT satu baris.
        WM_CHAR if wp.0 == 0x0D || wp.0 == 0x1B => LRESULT(0),
        WM_KILLFOCUS => {
            let _ = PostMessageW(notch, WM_APP_EDIT_BLUR, WPARAM(0), LPARAM(0));
            DefSubclassProc(hwnd, msg, wp, lp)
        }
        _ => DefSubclassProc(hwnd, msg, wp, lp),
    }
}

impl EditBox {
    pub fn new(notch: HWND) -> Result<Self> {
        NOTCH.store(notch.0 as isize, Ordering::Relaxed);
        unsafe {
            let (r, g, b) = EDIT_FILL_RGB;
            BRUSH.store(CreateSolidBrush(rgb(r, g, b)).0 as isize, Ordering::Relaxed);
            if !embedded::GEIST_REGULAR.is_empty() {
                let mut n = 0u32;
                AddFontMemResourceEx(
                    embedded::GEIST_REGULAR.as_ptr() as *const _,
                    embedded::GEIST_REGULAR.len() as u32,
                    None,
                    &mut n,
                );
            }
            let hinst: HINSTANCE = GetModuleHandleW(None)?.into();
            let class = w!("ToplineEditHost");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(host_proc),
                hInstance: hinst,
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_IBEAM)?,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let host = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                class,
                w!(""),
                WS_POPUP,
                0,
                0,
                10,
                10,
                notch,
                None,
                hinst,
                None,
            )?;
            let edit = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                w!(""),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                0,
                0,
                10,
                10,
                host,
                None,
                hinst,
                None,
            )?;
            let _ = SetWindowSubclass(edit, Some(edit_proc), 1, 0);
            // Batas panjang teks
            SendMessageW(edit, 0x00C5 /* EM_LIMITTEXT */, WPARAM(160), LPARAM(0));
            Ok(Self { host, edit, font: HFONT::default(), font_px: 0, visible: false })
        }
    }

    fn ensure_font(&mut self, px: i32) {
        if px == self.font_px {
            return;
        }
        unsafe {
            if !self.font.is_invalid() {
                let _ = DeleteObject(self.font);
            }
            let face = if embedded::GEIST_REGULAR.is_empty() { w!("Segoe UI Variable Text") } else { w!("Geist") };
            self.font = CreateFontW(
                -px,
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                CLEARTYPE_QUALITY.0 as u32,
                (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
                face,
            );
            SendMessageW(self.edit, WM_SETFONT, WPARAM(self.font.0 as usize), LPARAM(1));
            let cue: Vec<u16> = "Nama tugas… tambahkan jam, mis. 16:30".encode_utf16().chain(std::iter::once(0)).collect();
            // EM_SETCUEBANNER
            SendMessageW(self.edit, 0x1501, WPARAM(1), LPARAM(cue.as_ptr() as isize));
        }
        self.font_px = px;
    }

    /// Tampilkan di posisi layar (piksel fisik) dan ambil fokus.
    pub fn show(&mut self, x: i32, y: i32, w: i32, h: i32, font_px: i32) {
        self.ensure_font(font_px);
        unsafe {
            let line_h = font_px + font_px / 2;
            let pad_y = ((h - line_h) / 2).max(0);
            let _ = SetWindowPos(self.host, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE);
            let _ = SetWindowPos(self.edit, None, 0, pad_y, w, line_h, SWP_NOZORDER | SWP_NOACTIVATE);
            let _ = ShowWindow(self.host, SW_SHOW);
            let _ = SetForegroundWindow(self.host);
            let _ = SetFocus(self.edit);
        }
        self.visible = true;
    }

    pub fn hide(&mut self) {
        if self.visible {
            self.visible = false;
            unsafe {
                let _ = ShowWindow(self.host, SW_HIDE);
            }
        }
    }

    pub fn text(&self) -> String {
        unsafe {
            let n = GetWindowTextLengthW(self.edit);
            if n <= 0 {
                return String::new();
            }
            let mut buf = vec![0u16; n as usize + 1];
            let got = GetWindowTextW(self.edit, &mut buf);
            String::from_utf16_lossy(&buf[..got as usize])
        }
    }

    pub fn clear(&self) {
        unsafe {
            let _ = SetWindowTextW(self.edit, w!(""));
        }
    }

    pub fn has_focus(&self) -> bool {
        unsafe { GetFocus() == self.edit }
    }
}
