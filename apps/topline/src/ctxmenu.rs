//! Menu konteks kaca (pengganti TrackPopupMenu Win32), dipakai klik kanan tile launcher.
//! Isi berupa daftar item ber-id; App menerima id yang dipilih lewat `Reply::Pick`.
//! Seperti menu tray: aktif agar bisa menerima keyboard (↑/↓, Enter, Esc) dan menutup
//! sendiri saat pengguna mengklik di luar.

use std::mem::size_of;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::UI::Composition::Compositor;

use crate::gfx::{Align, Font, Gfx, Painter};
use crate::glass::{self, Glass};
use crate::theme::*;

const WM_MOUSELEAVE: u32 = 0x02A3;

const W: f32 = 252.0;
const PAD: f32 = 6.0;
const TITLE: f32 = 32.0;
const ROW: f32 = 36.0;
const SEP: f32 = 9.0;

pub struct Item {
    pub id: usize,
    pub label: String,
    pub icon: char,
    pub enabled: bool,
    /// Aksi yang menghapus sesuatu: ikon amber.
    pub danger: bool,
}

pub enum Entry {
    Item(Item),
    Separator,
}

pub enum Reply {
    Default,
    Done(LRESULT),
    Redraw,
    Pick(usize),
    Dismiss,
}

pub struct ContextMenu {
    pub hwnd: HWND,
    glass: Glass,
    title: String,
    entries: Vec<Entry>,
    /// Kotak setiap item aktif (DIP) dan id-nya, urut dari atas.
    rows: Vec<(f32, f32, usize)>,
    hover: Option<usize>,
    /// Item yang dipilih keyboard (indeks `rows`).
    focus: Option<usize>,
    tracking: bool,
}

fn height(entries: &[Entry]) -> f32 {
    PAD + TITLE + entries.iter().map(|e| if matches!(e, Entry::Item(_)) { ROW } else { SEP }).sum::<f32>() + PAD
}

impl ContextMenu {
    /// Tampilkan menu di kanan bawah titik `at` (posisi kursor). `title`: nama objek (mis. sematan).
    pub fn open(compositor: &Compositor, gfx: &Gfx, solid: bool, at: POINT, title: &str, entries: Vec<Entry>) -> Result<Self> {
        let h = height(&entries);
        let (hwnd, scale) = glass::create_popup(at, W, h, glass::Anchor::Below)?;
        let glass = match Glass::new(compositor, gfx, hwnd, W, h, scale, solid) {
            Ok(g) => g,
            Err(e) => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                return Err(e);
            }
        };
        let mut m = Self { hwnd, glass, title: title.to_string(), entries, rows: Vec::new(), hover: None, focus: None, tracking: false };
        m.draw(gfx);
        let _ = m.glass.appear();
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        Ok(m)
    }

    pub fn close(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    pub fn draw(&mut self, gfx: &Gfx) {
        let mut rows = Vec::new();
        let hover = self.hover.or_else(|| self.focus.and_then(|i| self.rows.get(i)).map(|r| r.2));
        let (title, entries) = (&self.title, &self.entries);
        let r = self.glass.draw(gfx, |p| draw_menu(p, title, entries, hover, &mut rows));
        if r.is_ok() {
            self.rows = rows;
        }
    }

    fn row_at(&self, y: f32) -> Option<usize> {
        self.rows.iter().find(|r| y >= r.0 && y < r.0 + r.1).map(|r| r.2)
    }

    pub fn on_message(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Reply {
        let s = self.glass.scale;
        let y_of = |lp: LPARAM| ((lp.0 >> 16) & 0xffff) as i16 as f32 / s;
        let x_of = |lp: LPARAM| (lp.0 & 0xffff) as i16 as f32 / s;
        match msg {
            WM_ACTIVATE if (wp.0 & 0xffff) as u32 == WA_INACTIVE => Reply::Dismiss,
            WM_MOUSEMOVE => {
                if !self.tracking {
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: self.hwnd, dwHoverTime: 0 };
                    unsafe {
                        let _ = TrackMouseEvent(&mut tme);
                    }
                    self.tracking = true;
                }
                let x = x_of(lp);
                let h = if (PAD..W - PAD).contains(&x) { self.row_at(y_of(lp)) } else { None };
                if h != self.hover {
                    self.hover = h;
                    self.focus = None;
                    return Reply::Redraw;
                }
                Reply::Done(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                if self.hover.take().is_some() {
                    return Reply::Redraw;
                }
                Reply::Done(LRESULT(0))
            }
            WM_LBUTTONUP | WM_RBUTTONUP => match self.row_at(y_of(lp)) {
                Some(id) => Reply::Pick(id),
                None => Reply::Done(LRESULT(0)),
            },
            WM_KEYDOWN => {
                let n = self.rows.len();
                match VIRTUAL_KEY(wp.0 as u16) {
                    VK_ESCAPE => Reply::Dismiss,
                    VK_DOWN | VK_TAB if n > 0 => {
                        self.focus = Some(self.focus.map_or(0, |i| (i + 1) % n));
                        self.hover = None;
                        Reply::Redraw
                    }
                    VK_UP if n > 0 => {
                        self.focus = Some(self.focus.map_or(n - 1, |i| (i + n - 1) % n));
                        self.hover = None;
                        Reply::Redraw
                    }
                    VK_RETURN | VK_SPACE => match self.focus.and_then(|i| self.rows.get(i)) {
                        Some(r) => Reply::Pick(r.2),
                        None => Reply::Done(LRESULT(0)),
                    },
                    _ => Reply::Default,
                }
            }
            WM_SETCURSOR => {
                unsafe {
                    let _ = SetCursor(LoadCursorW(None, IDC_ARROW).unwrap_or_default());
                }
                Reply::Done(LRESULT(1))
            }
            _ => Reply::Default,
        }
    }
}

/// Gambar menu; `rows` diisi kotak (y, tinggi, id) item yang aktif.
fn draw_menu(p: &Painter, title: &str, entries: &[Entry], hover: Option<usize>, rows: &mut Vec<(f32, f32, usize)>) {
    let mut y = PAD;
    p.label(title, PAD + 12.0, y + 9.0, W - PAD * 2.0 - 24.0, 16.0, 11.0, Align::Left, TEXT_3);
    y += TITLE;
    for e in entries {
        match e {
            Entry::Separator => {
                let ly = y + SEP / 2.0;
                p.line(PAD + 10.0, ly, W - PAD - 10.0, ly, Rgba(1.0, 1.0, 1.0, 0.08), 1.0);
                y += SEP;
            }
            Entry::Item(it) => {
                let hov = it.enabled && hover == Some(it.id);
                if hov {
                    p.fill_rrect(PAD, y, W - PAD * 2.0, ROW, 10.0, Rgba(1.0, 1.0, 1.0, 0.08));
                }
                let (icol, tcol) = match (it.enabled, it.danger) {
                    (false, _) => (TEXT_3.with_alpha(0.35), TEXT_3.with_alpha(0.45)),
                    (true, true) => (AMBER, TEXT_1),
                    (true, false) => (if hov { TEXT_1 } else { TEXT_2 }, TEXT_1),
                };
                p.icon(it.icon, PAD + 22.0, y + ROW / 2.0, 13.0, icol);
                p.text(&it.label, PAD + 42.0, y, W - PAD * 2.0 - 54.0, ROW, 13.0, 400, Font::Sans, Align::Left, tcol);
                if it.enabled {
                    rows.push((y, ROW, it.id));
                }
                y += ROW;
            }
        }
    }
}
