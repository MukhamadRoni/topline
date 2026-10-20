//! Ikon di system tray dan menunya.

use std::mem::size_of;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use topline_core::prayer;

use crate::WM_APP_TRAY;

pub const ID_OPEN: usize = 1;
pub const ID_ADD_TODO: usize = 2;
pub const ID_HOVER: usize = 3;
pub const ID_PERSONALITY: usize = 4;
pub const ID_AUTOSTART: usize = 5;
pub const ID_DATA: usize = 6;
pub const ID_SOLID: usize = 7;
pub const ID_EXIT: usize = 9;
pub const ID_ADZAN: usize = 10;
pub const ID_CLIPBOARD: usize = 11;
pub const ID_PRAYER_OFF: usize = 12;
pub const ID_PRAYER_LOCATE: usize = 13;
pub const ID_SCROLL: usize = 14;
pub const ID_LAUNCHER: usize = 15;
/// ID_CITY + indeks di `prayer::CITIES`.
pub const ID_CITY: usize = 100;

pub struct Tray {
    host: HWND,
    icon: HICON,
    added: bool,
}

pub struct MenuState {
    pub hover: bool,
    pub personality: bool,
    pub autostart: bool,
    pub solid: bool,
    pub prayer_city: String,
    /// Jadwal memakai "Lokasi saat ini".
    pub prayer_located: bool,
    /// Label tempat ("Surabaya" / "sekitar Surabaya").
    pub prayer_place: String,
    pub adzan: bool,
    pub clipboard: bool,
    pub scroll: bool,
}

impl Tray {
    pub fn new(host: HWND, icon: HICON) -> Self {
        let mut t = Self { host, icon, added: false };
        t.add();
        t
    }

    fn data(&self) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.host,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_APP_TRAY,
            hIcon: self.icon,
            ..Default::default()
        };
        let tip: Vec<u16> = "Topline · Win+Alt+T".encode_utf16().collect();
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        nid
    }

    /// Dipanggil juga saat Explorer restart (pesan "TaskbarCreated").
    pub fn add(&mut self) {
        let nid = self.data();
        unsafe {
            self.added = Shell_NotifyIconW(NIM_ADD, &nid).as_bool();
        }
    }

    pub fn remove(&mut self) {
        if self.added {
            let nid = self.data();
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            }
            self.added = false;
        }
    }

    /// Tampilkan menu dan kembalikan id item yang dipilih (0 = batal).
    pub fn show_menu(&self, st: &MenuState) -> usize {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return 0 };
            let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(std::iter::once(0)).collect() };
            let add = |m: HMENU, id: usize, text: &str, checked: bool| {
                let w = wide(text);
                let flags = MF_STRING | if checked { MF_CHECKED } else { MF_UNCHECKED };
                let _ = AppendMenuW(m, flags, id, PCWSTR(w.as_ptr()));
            };
            let item = |id: usize, text: &str, checked: bool| add(menu, id, text, checked);
            let sep_in = |m: HMENU| {
                let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
            };
            let sep = || sep_in(menu);
            let popup = |parent: HMENU, sub: HMENU, text: &str, checked: bool| {
                let w = wide(text);
                let flags = MF_POPUP | if checked { MF_CHECKED } else { MF_UNCHECKED };
                let _ = AppendMenuW(parent, flags, sub.0 as usize, PCWSTR(w.as_ptr()));
            };
            item(ID_OPEN, "Buka Topline\tWin+Alt+T", false);
            item(ID_ADD_TODO, "Tambah tugas\tWin+Alt+N", false);
            item(ID_LAUNCHER, "Buka launcher", false);
            sep();
            item(ID_HOVER, "Buka saat kursor mendekat", st.hover);
            item(ID_PERSONALITY, "Mata saat idle (Personality)", st.personality);
            item(ID_SCROLL, "Gestur scroll di notch", st.scroll);
            item(ID_SOLID, "Mode tanpa kaca (lebih hemat GPU)", st.solid);
            item(ID_AUTOSTART, "Mulai bersama Windows", st.autostart);
            item(ID_CLIPBOARD, "Riwayat clipboard (hanya di memori)", st.clipboard);
            // Jadwal sholat: kota per wilayah
            if let Ok(pm) = CreatePopupMenu() {
                add(pm, ID_PRAYER_OFF, "Nonaktif", st.prayer_city.is_empty());
                add(pm, ID_PRAYER_LOCATE, "Lokasi saat ini (perbarui)", st.prayer_located);
                sep_in(pm);
                for region in prayer::REGIONS {
                    let Ok(rm) = CreatePopupMenu() else { continue };
                    let mut here = false;
                    for (i, c) in prayer::CITIES.iter().enumerate().filter(|(_, c)| c.region == region) {
                        let on = !st.prayer_located && c.name == st.prayer_city;
                        here |= on;
                        add(rm, ID_CITY + i, c.name, on);
                    }
                    popup(pm, rm, region, here);
                }
                sep_in(pm);
                add(pm, ID_ADZAN, "Suara adzan (adzan.wav di folder data)", st.adzan);
                let title = if st.prayer_city.is_empty() { "Jadwal sholat".to_string() } else { format!("Jadwal sholat · {}", st.prayer_place) };
                popup(menu, pm, &title, false);
            }
            sep();
            item(ID_DATA, "Buka folder data", false);
            sep();
            item(ID_EXIT, "Keluar", false);

            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            let _ = SetForegroundWindow(self.host);
            let cmd = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                p.x,
                p.y,
                0,
                self.host,
                None,
            );
            let _ = PostMessageW(self.host, WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
            cmd.0 as usize
        }
    }
}
