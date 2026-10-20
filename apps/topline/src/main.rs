//! Topline: notch hidup untuk Windows 11 (Rust native, Windows.UI.Composition).
#![windows_subsystem = "windows"]

mod app;
mod clip;
mod editbox;
mod gfx;
mod glass;
mod launcher;
mod lyrics;
mod media;
mod notch;
mod settings;
mod sysmon;
mod theme;
mod tray;
mod traymenu;
mod util;
mod view;
mod watch;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}

use windows::Win32::UI::WindowsAndMessaging::WM_APP;

pub const WM_APP_MEDIA: u32 = WM_APP + 1;
pub const WM_APP_VOLUME: u32 = WM_APP + 2;
pub const WM_APP_NET: u32 = WM_APP + 3;
pub const WM_APP_APPBAR: u32 = WM_APP + 4;
pub const WM_APP_TRAY: u32 = WM_APP + 5;
pub const WM_APP_EDIT_SUBMIT: u32 = WM_APP + 6;
pub const WM_APP_EDIT_CANCEL: u32 = WM_APP + 7;
pub const WM_APP_EDIT_BLUR: u32 = WM_APP + 8;
pub const WM_APP_TOGGLE: u32 = WM_APP + 9;
pub const WM_APP_PRIVACY: u32 = WM_APP + 10;
/// LPARAM = Box<String> (nama file), dimiliki penerima.
pub const WM_APP_DOWNLOAD: u32 = WM_APP + 11;
/// LPARAM = Box<GeoPoint> (dimiliki penerima), atau 0 jika lokasi gagal didapat.
pub const WM_APP_LOCATION: u32 = WM_APP + 12;
/// LPARAM = Box<launcher::DropEvent> (dimiliki penerima); hanya jika App sedang dipinjam.
pub const WM_APP_DROP: u32 = WM_APP + 13;
/// LPARAM = Box<lyrics::Reply> (dimiliki penerima).
pub const WM_APP_LYRICS: u32 = WM_APP + 14;
/// `topline settings [bagian]`: buka jendela Pengaturan. WPARAM = indeks bagian.
pub const WM_APP_SETTINGS: u32 = WM_APP + 15;

fn main() {
    app::run();
}
