//! Menu klik kanan ikon tray: popup kaca seperti notch (bukan menu Win32 standar).
//! Isi: kepala (logo, versi), aksi cepat, sakelar yang sering dipakai, jadwal sholat,
//! bahasa, folder data, dan keluar. Pengaturan lengkap ada di jendela Pengaturan.
//!
//! Popup ini aktif (agar bisa menerima keyboard dan tahu kapan harus menutup): klik di luar
//! atau Esc menutupnya. Sakelar dan bahasa tidak menutup menu; aksi lain menutupnya dulu.

use std::mem::size_of;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::UI::Composition::Compositor;

use topline_core::{Lang, Language, Settings, Toggle};

use crate::gfx::{Align, Font, Gfx, Painter};
use crate::glass::{self, Glass};
use crate::theme::*;

const WM_MOUSELEAVE: u32 = 0x02A3;

/// Logo untuk kepala menu.
const LOGO_PNG: &[u8] = include_bytes!("../../../assets/branding/topline-icon-128.png");

const W: f32 = 300.0;
const PAD: f32 = 6.0;
const ROW: f32 = 38.0;
const HEADER: f32 = 62.0;
const DIVIDER: f32 = 9.0;
const LANG_ROW: f32 = 44.0;
const FOOTER: f32 = 44.0;

/// Aksi dari menu tray, dijalankan oleh App.
#[derive(Clone, Debug, PartialEq)]
pub enum TAction {
    Open,
    AddTodo,
    Launcher,
    Settings,
    Prayer,
    Toggle(Toggle),
    Autostart,
    Language(Language),
    DataFolder,
    Exit,
}

impl TAction {
    /// Sakelar dan bahasa: menu tetap terbuka setelah diklik.
    pub fn keeps_open(&self) -> bool {
        matches!(self, TAction::Toggle(_) | TAction::Autostart | TAction::Language(_))
    }
}

pub struct View<'a> {
    pub s: &'a Settings,
    pub lang: Lang,
    pub autostart: bool,
    pub prayer_place: String,
}

pub enum Reply {
    Default,
    Done(LRESULT),
    Redraw,
    Act(TAction),
    /// Tutup tanpa aksi (klik di luar, Esc).
    Dismiss,
}

struct Hit {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    action: TAction,
}

/// Tinggi isi menu (DIP); tetap, jadi bisa dihitung sebelum jendela dibuat.
fn height() -> f32 {
    PAD + HEADER + DIVIDER + 4.0 * ROW + DIVIDER + 4.0 * ROW + ROW + DIVIDER + LANG_ROW + DIVIDER + FOOTER + PAD
}

pub struct TrayMenu {
    pub hwnd: HWND,
    glass: Glass,
    logo: Option<ID2D1Bitmap>,
    hits: Vec<Hit>,
    hover: Option<TAction>,
    focus: Option<usize>,
    tracking: bool,
}

impl TrayMenu {
    /// Buat dan tampilkan menu di dekat titik `at` (posisi kursor saat klik kanan ikon tray).
    pub fn open(compositor: &Compositor, gfx: &Gfx, solid: bool, at: POINT, v: &View) -> Result<Self> {
        unsafe {
            // Di atas kursor (taskbar bawah), tetap di dalam area kerja monitor.
            let (hwnd, scale) = glass::create_popup(at, W, height(), glass::Anchor::Above)?;
            let glass = match Glass::new(compositor, gfx, hwnd, W, height(), scale, solid) {
                Ok(g) => g,
                Err(e) => {
                    let _ = DestroyWindow(hwnd);
                    return Err(e);
                }
            };
            let mut m = Self {
                hwnd,
                glass,
                logo: gfx.decode_image(LOGO_PNG).ok(),
                hits: Vec::new(),
                hover: None,
                focus: None,
                tracking: false,
            };
            m.draw(gfx, v);
            let _ = m.glass.appear();
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            Ok(m)
        }
    }

    pub fn close(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    pub fn set_solid(&self, solid: bool) {
        let _ = self.glass.set_solid(solid);
    }

    pub fn draw(&mut self, gfx: &Gfx, v: &View) {
        let mut hits = Vec::new();
        let (hover, focus, logo) = (self.hover.clone(), self.focus, self.logo.as_ref());
        let r = self.glass.draw(gfx, |p| {
            let mut b = B { p, hits: Vec::new(), hover };
            draw_menu(&mut b, v, logo);
            if let Some(h) = focus.and_then(|i| b.hits.get(i)) {
                p.stroke_rrect(h.x - 1.0, h.y - 1.0, h.w + 2.0, h.h + 2.0, 11.0, SKY, 1.5);
            }
            hits = b.hits;
        });
        if r.is_ok() {
            self.hits = hits;
        }
    }

    pub fn on_message(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Reply {
        let s = self.glass.scale;
        let client = |lp: LPARAM| ((lp.0 & 0xffff) as i16 as f32 / s, ((lp.0 >> 16) & 0xffff) as i16 as f32 / s);
        let hit_at = |hits: &[Hit], x: f32, y: f32| {
            hits.iter().rev().find(|h| x >= h.x && x < h.x + h.w && y >= h.y && y < h.y + h.h).map(|h| h.action.clone())
        };
        match msg {
            // Klik di luar menu: jendela lain aktif → tutup.
            WM_ACTIVATE if (wp.0 & 0xffff) as u32 == WA_INACTIVE => Reply::Dismiss,
            WM_MOUSEMOVE => {
                if !self.tracking {
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: self.hwnd, dwHoverTime: 0 };
                    unsafe {
                        let _ = TrackMouseEvent(&mut tme);
                    }
                    self.tracking = true;
                }
                let (x, y) = client(lp);
                let h = hit_at(&self.hits, x, y);
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
            WM_LBUTTONUP => match hit_at(&self.hits, client(lp).0, client(lp).1) {
                Some(a) => Reply::Act(a),
                None => Reply::Done(LRESULT(0)),
            },
            WM_KEYDOWN => {
                let n = self.hits.len();
                match VIRTUAL_KEY(wp.0 as u16) {
                    VK_ESCAPE => Reply::Dismiss,
                    VK_DOWN | VK_TAB if n > 0 => {
                        self.focus = Some(self.focus.map_or(0, |i| (i + 1) % n));
                        Reply::Redraw
                    }
                    VK_UP if n > 0 => {
                        self.focus = Some(self.focus.map_or(n - 1, |i| (i + n - 1) % n));
                        Reply::Redraw
                    }
                    VK_RETURN | VK_SPACE => match self.focus.and_then(|i| self.hits.get(i)) {
                        Some(h) => Reply::Act(h.action.clone()),
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

// ------------------------------------------------------------------ penggambaran

struct B<'p, 'a> {
    p: &'p Painter<'a>,
    hits: Vec<Hit>,
    hover: Option<TAction>,
}

/// Ikon kecil di kiri baris.
#[derive(Clone, Copy)]
enum Icon {
    Notch,
    Add,
    Grid,
    Gear,
    Moon,
    Folder,
    Power,
}

impl B<'_, '_> {
    /// Baris yang bisa diklik; kembalikan true jika sedang di-hover.
    fn row(&mut self, x: f32, y: f32, w: f32, h: f32, action: TAction) -> bool {
        let hov = self.hover.as_ref() == Some(&action);
        if hov {
            self.p.fill_rrect(x, y, w, h, 10.0, Rgba(1.0, 1.0, 1.0, 0.08));
        }
        self.hits.push(Hit { x, y, w, h, action });
        hov
    }

    fn icon(&self, icon: Icon, cx: f32, cy: f32, col: Rgba) {
        let p = self.p;
        match icon {
            // notch mini: pill membulat bawah di tepi atas
            Icon::Notch => {
                p.stroke_rrect(cx - 8.0, cy - 6.0, 16.0, 12.0, 3.0, col, 1.3);
                p.fill_rrect(cx - 4.0, cy - 6.0, 8.0, 3.5, 1.5, col);
            }
            Icon::Add => p.icon(G_ADD, cx, cy, 12.0, col),
            Icon::Grid => crate::view::grid_icon(p, cx, cy, 13.0, col),
            Icon::Gear => p.icon(G_SETTINGS, cx, cy, 13.0, col),
            Icon::Moon => p.icon(G_MOON, cx, cy, 13.0, col),
            Icon::Folder => p.icon(G_FOLDER, cx, cy, 13.0, col),
            Icon::Power => p.icon(G_POWER, cx, cy, 13.0, col),
        }
    }

    fn action(&mut self, y: f32, icon: Icon, label: &str, hint: &str, action: TAction) {
        let (x, w) = (PAD, W - PAD * 2.0);
        let hov = self.row(x, y, w, ROW, action);
        self.icon(icon, x + 22.0, y + ROW / 2.0, if hov { TEXT_1 } else { TEXT_2 });
        self.p.text(label, x + 42.0, y, w - 42.0 - 110.0, ROW, 14.0, 400, Font::Sans, Align::Left, TEXT_1);
        if !hint.is_empty() {
            self.p.label(hint, x + w - 120.0, y + 11.0, 108.0, 16.0, 11.0, Align::Right, TEXT_3);
        }
    }

    fn toggle(&mut self, y: f32, label: &str, on: bool, action: TAction) {
        let (x, w) = (PAD, W - PAD * 2.0);
        self.row(x, y, w, ROW, action);
        self.p.text(label, x + 14.0, y, w - 70.0, ROW, 13.0, 400, Font::Sans, Align::Left, TEXT_1);
        let (sx, sy) = (x + w - 14.0 - 34.0, y + (ROW - 20.0) / 2.0);
        if on {
            self.p.fill_rrect(sx, sy, 34.0, 20.0, 10.0, SKY);
            self.p.fill_circle(sx + 34.0 - 10.0, sy + 10.0, 7.0, ICE);
        } else {
            self.p.fill_rrect(sx, sy, 34.0, 20.0, 10.0, Rgba(1.0, 1.0, 1.0, 0.10));
            self.p.stroke_rrect(sx, sy, 34.0, 20.0, 10.0, Rgba(1.0, 1.0, 1.0, 0.24), 1.0);
            self.p.fill_circle(sx + 10.0, sy + 10.0, 6.5, TEXT_2);
        }
    }

    fn divider(&self, y: f32) {
        let ly = y + DIVIDER / 2.0;
        self.p.line(PAD + 10.0, ly, W - PAD - 10.0, ly, Rgba(1.0, 1.0, 1.0, 0.08), 1.0);
    }
}

fn draw_menu(b: &mut B, v: &View, logo: Option<&ID2D1Bitmap>) {
    let p = b.p;
    let l = v.lang;
    let mut y = PAD;

    // kepala: logo, nama, versi + pintasan
    match logo {
        Some(bmp) => p.bitmap(bmp, PAD + 12.0, y + 13.0, 36.0, 36.0, 1.0),
        None => p.fill_rrect(PAD + 12.0, y + 13.0, 36.0, 36.0, 10.0, ULTRAMARINE),
    }
    p.text("Topline", PAD + 60.0, y + 12.0, 180.0, 20.0, 15.0, 500, Font::Sans, Align::Left, TEXT_1);
    p.label(
        &format!("v{} · Win + Alt + T", env!("CARGO_PKG_VERSION")),
        PAD + 60.0,
        y + 34.0,
        200.0,
        16.0,
        11.0,
        Align::Left,
        TEXT_3,
    );
    y += HEADER;
    b.divider(y);
    y += DIVIDER;

    // aksi
    b.action(y, Icon::Notch, l.t("Buka Topline", "Open Topline"), "", TAction::Open);
    y += ROW;
    b.action(y, Icon::Add, l.t("Tambah tugas", "Add task"), "Win+Alt+N", TAction::AddTodo);
    y += ROW;
    b.action(y, Icon::Grid, l.t("Buka launcher", "Open launcher"), "", TAction::Launcher);
    y += ROW;
    b.action(y, Icon::Gear, l.t("Pengaturan", "Settings"), "", TAction::Settings);
    y += ROW;
    b.divider(y);
    y += DIVIDER;

    // sakelar cepat
    let s = v.s;
    for (label, t) in [
        (l.t("Mata Personality", "Personality eyes"), Toggle::Personality),
        (l.t("Buka saat kursor mendekat", "Open on hover"), Toggle::OpenOnHover),
        (l.t("Mode tanpa kaca", "No-glass mode"), Toggle::Solid),
    ] {
        b.toggle(y, label, t.get(s), TAction::Toggle(t));
        y += ROW;
    }
    b.toggle(y, l.t("Mulai bersama Windows", "Start with Windows"), v.autostart, TAction::Autostart);
    y += ROW;

    // jadwal sholat → Pengaturan bagian Sholat
    let (x, w) = (PAD, W - PAD * 2.0);
    let hov = b.row(x, y, w, ROW, TAction::Prayer);
    b.icon(Icon::Moon, x + 22.0, y + ROW / 2.0, if hov { TEXT_1 } else { TEXT_2 });
    p.text(l.t("Jadwal sholat", "Prayer times"), x + 42.0, y, 120.0, ROW, 14.0, 400, Font::Sans, Align::Left, TEXT_1);
    let place = if s.prayer_city.is_empty() { l.t("Nonaktif", "Off").to_string() } else { v.prayer_place.clone() };
    p.text(&place, x + 150.0, y, w - 150.0 - 30.0, ROW, 12.0, 400, Font::Sans, Align::Right, TEXT_2);
    p.icon(G_CHEVRON_RIGHT, x + w - 16.0, y + ROW / 2.0, 10.0, TEXT_3);
    y += ROW;
    b.divider(y);
    y += DIVIDER;

    // bahasa: segmen kecil
    p.text(l.t("Bahasa", "Language"), PAD + 14.0, y, 100.0, LANG_ROW, 13.0, 400, Font::Sans, Align::Left, TEXT_2);
    let opts = [(Language::Auto, l.t("Auto", "Auto")), (Language::Id, "ID"), (Language::En, "EN")];
    let sw = 52.0;
    let total = sw * 3.0 + 6.0;
    let (mut sx, sy) = (W - PAD - 10.0 - total, y + (LANG_ROW - 30.0) / 2.0);
    p.fill_rrect(sx, sy, total, 30.0, 15.0, GLASS_FILL);
    p.stroke_rrect(sx, sy, total, 30.0, 15.0, GLASS_BORDER, 1.0);
    sx += 3.0;
    for (lang, text) in opts {
        let active = s.language == lang;
        let action = TAction::Language(lang);
        let hov = b.hover.as_ref() == Some(&action);
        b.hits.push(Hit { x: sx, y: sy + 3.0, w: sw, h: 24.0, action });
        if active {
            p.fill_rrect(sx, sy + 3.0, sw, 24.0, 12.0, ICE);
        } else if hov {
            p.fill_rrect(sx, sy + 3.0, sw, 24.0, 12.0, GLASS_FILL_HOVER);
        }
        p.text(text, sx, sy + 3.0, sw, 24.0, 12.0, 500, Font::Sans, Align::Center, if active { NAVY } else { TEXT_2 });
        sx += sw;
    }
    y += LANG_ROW;
    b.divider(y);
    y += DIVIDER;

    // kaki: folder data dan keluar
    let half = (W - PAD * 2.0 - 6.0) / 2.0;
    for (i, (icon, label, action)) in [
        (Icon::Folder, l.t("Folder data", "Data folder"), TAction::DataFolder),
        (Icon::Power, l.t("Keluar", "Exit"), TAction::Exit),
    ]
    .into_iter()
    .enumerate()
    {
        let x = PAD + i as f32 * (half + 6.0);
        let (by, bh) = (y + 3.0, FOOTER - 6.0);
        let hov = b.row(x, by, half, bh, action);
        let col = if hov { TEXT_1 } else { TEXT_2 };
        let tw = p.gfx.measure(label, 13.0, 400, Font::Sans);
        let start = x + (half - (16.0 + 8.0 + tw)) / 2.0;
        b.icon(icon, start + 8.0, by + bh / 2.0, col);
        p.text(label, start + 24.0, by, tw + 4.0, bh, 13.0, 400, Font::Sans, Align::Left, col);
    }
}
