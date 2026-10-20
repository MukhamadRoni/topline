//! Jendela Pengaturan: kaca seperti notch (HostBackdropBrush + tint, isi Direct2D di
//! CompositionDrawingSurface), bingkai sendiri (WM_NCCALCSIZE = 0, area judul = HTCAPTION),
//! dan kontrol yang digambar sendiri: sakelar, stepper angka, tombol, chip tab.
//! Berbeda dari notch, jendela ini boleh aktif dan mengambil fokus keyboard:
//! Tab / Shift+Tab pindah kontrol, Enter / Spasi menjalankan, ←/→ mengubah angka,
//! ↑/↓ pindah bagian, Esc menutup.

use std::mem::size_of;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::UI::Composition::Compositor;

use topline_core::{Lang, Language, Number, Settings, Toggle, Widget};

use crate::gfx::{Align, Font, Gfx, Painter};
use crate::glass::{self, Glass};
use crate::theme::*;

pub const CLASS: PCWSTR = w!("ToplineSettings");
const WM_MOUSELEAVE: u32 = 0x02A3;

const W: f32 = 760.0;
const H: f32 = 560.0;
const SIDEBAR: f32 = 212.0;
const TITLE_H: f32 = 56.0;
const CX0: f32 = 236.0;
const CW: f32 = W - CX0 - 28.0;
const ROW: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    General,
    Appearance,
    Focus,
    Water,
    Prayer,
    Lyrics,
    About,
}

impl Section {
    pub const ALL: [Section; 7] = [
        Section::General,
        Section::Appearance,
        Section::Focus,
        Section::Water,
        Section::Prayer,
        Section::Lyrics,
        Section::About,
    ];

    /// Nama bagian untuk `topline settings <bagian>` (Indonesia atau Inggris).
    pub fn from_arg(s: &str) -> Option<Section> {
        Some(match s.to_lowercase().as_str() {
            "umum" | "general" => Section::General,
            "tampilan" | "appearance" => Section::Appearance,
            "fokus" | "focus" => Section::Focus,
            "air" | "water" => Section::Water,
            "sholat" | "prayer" => Section::Prayer,
            "lirik" | "lyrics" => Section::Lyrics,
            "tentang" | "about" => Section::About,
            _ => return None,
        })
    }

    fn title(self, l: Lang) -> &'static str {
        match self {
            Section::General => l.t("Umum", "General"),
            Section::Appearance => l.t("Tampilan", "Appearance"),
            Section::Focus => l.t("Fokus", "Focus"),
            Section::Water => l.t("Air minum", "Water"),
            Section::Prayer => l.t("Jadwal sholat", "Prayer times"),
            Section::Lyrics => l.t("Lirik", "Lyrics"),
            Section::About => l.t("Tentang", "About"),
        }
    }

    fn desc(self, l: Lang) -> &'static str {
        match self {
            Section::General => l.t("Cara Topline muncul dan bereaksi.", "How Topline appears and responds."),
            Section::Appearance => l.t("Kaca, mata Personality, dan tab di notch.", "Glass, Personality eyes, and notch tabs."),
            Section::Focus => l.t(
                "Durasi Pomodoro. Timer yang belum dimulai langsung ikut berubah.",
                "Pomodoro lengths. A timer that hasn't started updates right away.",
            ),
            Section::Water => l.t("Target harian dan jadwal pengingat minum.", "Daily goal and drink reminder schedule."),
            Section::Prayer => l.t("Dihitung offline dengan metode Kemenag.", "Calculated offline with the Kemenag method."),
            Section::Lyrics => l.t("Lirik tersinkron di tab Lirik dan di bawah notch.", "Synced lyrics in the Lyrics tab and under the notch."),
            Section::About => l.t("Topline: notch hidup untuk Windows 11.", "Topline: a living notch for Windows 11."),
        }
    }
}

/// Aksi dari jendela Pengaturan, dijalankan oleh App.
#[derive(Clone, Debug, PartialEq)]
pub enum SAction {
    Nav(Section),
    Toggle(Toggle),
    Step(Number, i32),
    Tab(Widget),
    Language(Language),
    Autostart,
    ChooseCity,
    Locate,
    LyricsFolder,
    ClearLyrics,
    DataFolder,
    Close,
}

/// Data yang ditampilkan (dibaca ulang setiap kali digambar).
pub struct View<'a> {
    pub s: &'a Settings,
    pub lang: Lang,
    pub autostart: bool,
    pub prayer_place: String,
    pub lyrics_cache: usize,
}

/// Hasil penanganan pesan.
pub enum Reply {
    /// Teruskan ke DefWindowProc.
    Default,
    Done(LRESULT),
    /// Gambar ulang (hover/fokus berubah).
    Redraw,
    Act(SAction),
}

struct Hit {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    action: SAction,
}

impl Hit {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

pub struct SettingsWindow {
    pub hwnd: HWND,
    glass: Glass,
    pub section: Section,
    hits: Vec<Hit>,
    hover: Option<SAction>,
    /// Kontrol yang difokuskan keyboard (indeks `hits`).
    focus: Option<usize>,
    tracking: bool,
}

impl SettingsWindow {
    pub fn open(compositor: &Compositor, gfx: &Gfx, solid: bool, section: Section) -> Result<Self> {
        unsafe {
            let hinst: HINSTANCE = GetModuleHandleW(None)?.into();
            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_APPWINDOW,
                CLASS,
                w!("Pengaturan Topline"),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                100,
                100,
                None,
                None,
                hinst,
                None,
            )?;
            glass::prepare_window(hwnd);
            let dpi = GetDpiForWindow(hwnd);
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            Self::center(hwnd, scale);
            let glass = match Glass::new(compositor, gfx, hwnd, W, H, scale, solid) {
                Ok(g) => g,
                Err(e) => {
                    let _ = DestroyWindow(hwnd);
                    return Err(e);
                }
            };
            Ok(Self { hwnd, glass, section, hits: Vec::new(), hover: None, focus: None, tracking: false })
        }
    }

    /// Tampilkan dan bawa ke depan (dipanggil setelah gambar pertama).
    pub fn show(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNORMAL);
            let _ = SetForegroundWindow(self.hwnd);
        }
    }

    fn center(hwnd: HWND, scale: f32) {
        unsafe {
            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTOPRIMARY);
            let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let r = mi.rcWork;
            let (w, h) = ((W * scale).round() as i32, (H * scale).round() as i32);
            let x = r.left + ((r.right - r.left) - w) / 2;
            let y = r.top + ((r.bottom - r.top) - h) / 2;
            let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    /// Judul jendela (taskbar, Alt+Tab) sesuai bahasa UI.
    pub fn set_title(&self, l: Lang) {
        let t = crate::util::wide(l.t("Pengaturan Topline", "Topline Settings"));
        unsafe {
            let _ = SetWindowTextW(self.hwnd, PCWSTR(t.as_ptr()));
        }
    }

    pub fn set_solid(&mut self, solid: bool) -> Result<()> {
        self.glass.set_solid(solid)
    }

    pub fn close(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    // ---------------------------------------------------------------- gambar

    pub fn draw(&mut self, gfx: &Gfx, v: &View) {
        let mut hits = Vec::new();
        let (hover, focus, section) = (self.hover.clone(), self.focus, self.section);
        let r = self.glass.draw(gfx, |p| {
            let mut b = B { p, hits: Vec::new(), hover };
            draw_all(&mut b, section, v);
            // cincin fokus keyboard
            if let Some(h) = focus.and_then(|i| b.hits.get(i)) {
                let r = (h.h / 2.0).min(16.0);
                p.stroke_rrect(h.x - 3.0, h.y - 3.0, h.w + 6.0, h.h + 6.0, r + 3.0, SKY, 2.0);
            }
            hits = b.hits;
        });
        if r.is_ok() {
            self.hits = hits;
        }
    }

    // ---------------------------------------------------------------- pesan

    pub fn on_message(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Reply {
        let s = self.glass.scale;
        let client = |lp: LPARAM| ((lp.0 & 0xffff) as i16 as f32 / s, ((lp.0 >> 16) & 0xffff) as i16 as f32 / s);
        match msg {
            WM_NCCALCSIZE if wp.0 != 0 => Reply::Done(LRESULT(0)),
            WM_NCHITTEST => {
                let mut pt = POINT { x: (lp.0 & 0xffff) as i16 as i32, y: ((lp.0 >> 16) & 0xffff) as i16 as i32 };
                unsafe {
                    let _ = ScreenToClient(self.hwnd, &mut pt);
                }
                let (x, y) = (pt.x as f32 / s, pt.y as f32 / s);
                let on_hit = self.hits.iter().any(|h| h.contains(x, y));
                Reply::Done(LRESULT(if y < TITLE_H && !on_hit { HTCAPTION as isize } else { HTCLIENT as isize }))
            }
            WM_MOUSEMOVE => {
                if !self.tracking {
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: self.hwnd, dwHoverTime: 0 };
                    unsafe {
                        let _ = TrackMouseEvent(&mut tme);
                    }
                    self.tracking = true;
                }
                let (x, y) = client(lp);
                let h = self.hits.iter().rev().find(|h| h.contains(x, y)).map(|h| h.action.clone());
                if h != self.hover {
                    self.hover = h;
                    self.set_cursor();
                    return Reply::Redraw;
                }
                Reply::Done(LRESULT(0))
            }
            WM_SETCURSOR if (lp.0 & 0xffff) as u32 == HTCLIENT => {
                self.set_cursor();
                Reply::Done(LRESULT(1))
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                if self.hover.take().is_some() {
                    return Reply::Redraw;
                }
                Reply::Done(LRESULT(0))
            }
            WM_LBUTTONUP => {
                let (x, y) = client(lp);
                match self.hits.iter().rev().find(|h| h.contains(x, y)) {
                    Some(h) => {
                        self.focus = None;
                        Reply::Act(h.action.clone())
                    }
                    None => Reply::Done(LRESULT(0)),
                }
            }
            WM_KEYDOWN => self.on_key(wp.0 as u16),
            WM_CLOSE => Reply::Act(SAction::Close),
            WM_DPICHANGED => {
                let dpi = (wp.0 >> 16) & 0xffff;
                let _ = self.glass.set_scale(dpi as f32 / 96.0);
                let r = unsafe { &*(lp.0 as *const RECT) };
                unsafe {
                    let _ = SetWindowPos(self.hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                Reply::Redraw
            }
            _ => Reply::Default,
        }
    }

    fn set_cursor(&self) {
        unsafe {
            let _ = SetCursor(LoadCursorW(None, if self.hover.is_some() { IDC_HAND } else { IDC_ARROW }).unwrap_or_default());
        }
    }

    fn on_key(&mut self, vk: u16) -> Reply {
        let n = self.hits.len();
        let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
        match VIRTUAL_KEY(vk) {
            VK_ESCAPE => Reply::Act(SAction::Close),
            VK_TAB if n > 0 => {
                self.focus = Some(match (self.focus, shift) {
                    (None, false) => 0,
                    (None, true) => n - 1,
                    (Some(i), false) => (i + 1) % n,
                    (Some(i), true) => (i + n - 1) % n,
                });
                Reply::Redraw
            }
            VK_RETURN | VK_SPACE => match self.focus.and_then(|i| self.hits.get(i)) {
                Some(h) => Reply::Act(h.action.clone()),
                None => Reply::Done(LRESULT(0)),
            },
            VK_LEFT | VK_RIGHT => match self.focus.and_then(|i| self.hits.get(i)).map(|h| h.action.clone()) {
                Some(SAction::Step(num, _)) => Reply::Act(SAction::Step(num, if VIRTUAL_KEY(vk) == VK_LEFT { -1 } else { 1 })),
                _ => Reply::Done(LRESULT(0)),
            },
            VK_UP | VK_DOWN => {
                let i = Section::ALL.iter().position(|s| *s == self.section).unwrap_or(0);
                let j = if VIRTUAL_KEY(vk) == VK_UP { i.saturating_sub(1) } else { (i + 1).min(Section::ALL.len() - 1) };
                Reply::Act(SAction::Nav(Section::ALL[j]))
            }
            _ => Reply::Default,
        }
    }

    /// Pindah bagian: fokus keyboard tetap di menu samping (hit pertama = item menu).
    pub fn nav(&mut self, s: Section) {
        self.section = s;
        if self.focus.is_some() {
            self.focus = Section::ALL.iter().position(|x| *x == s);
        }
    }
}

// ------------------------------------------------------------------ penggambaran

struct B<'p, 'a> {
    p: &'p Painter<'a>,
    hits: Vec<Hit>,
    hover: Option<SAction>,
}

impl B<'_, '_> {
    fn hit(&mut self, x: f32, y: f32, w: f32, h: f32, action: SAction) -> bool {
        let hov = self.hover.as_ref() == Some(&action);
        self.hits.push(Hit { x, y, w, h, action });
        hov
    }

    /// Label dan keterangan di kiri baris.
    fn row_text(&self, y: f32, label: &str, desc: &str, right: f32) {
        let tw = CW - 36.0 - right;
        if desc.is_empty() {
            self.p.text(label, CX0 + 18.0, y, tw, ROW, 14.0, 500, Font::Sans, Align::Left, TEXT_1);
        } else {
            self.p.text(label, CX0 + 18.0, y + 10.0, tw, 20.0, 14.0, 500, Font::Sans, Align::Left, TEXT_1);
            self.p.text(desc, CX0 + 18.0, y + 31.0, tw, 18.0, 12.0, 400, Font::Sans, Align::Left, TEXT_2);
        }
    }

    fn toggle_row(&mut self, y: f32, label: &str, desc: &str, on: bool, action: SAction) {
        let hov = self.hit(CX0, y, CW, ROW, action);
        if hov {
            self.p.fill_rrect(CX0 + 4.0, y + 4.0, CW - 8.0, ROW - 8.0, 12.0, Rgba(1.0, 1.0, 1.0, 0.04));
        }
        self.row_text(y, label, desc, 60.0);
        let (sx, sy) = (CX0 + CW - 18.0 - 44.0, y + (ROW - 24.0) / 2.0);
        if on {
            self.p.fill_rrect(sx, sy, 44.0, 24.0, 12.0, SKY);
            self.p.fill_circle(sx + 44.0 - 12.0, sy + 12.0, 8.5, ICE);
        } else {
            self.p.fill_rrect(sx, sy, 44.0, 24.0, 12.0, Rgba(1.0, 1.0, 1.0, if hov { 0.16 } else { 0.10 }));
            self.p.stroke_rrect(sx, sy, 44.0, 24.0, 12.0, Rgba(1.0, 1.0, 1.0, 0.24), 1.0);
            self.p.fill_circle(sx + 12.0, sy + 12.0, 8.0, TEXT_2);
        }
    }

    /// Pilihan bahasa: tiga segmen (Otomatis / Indonesia / English).
    fn language_row(&mut self, y: f32, l: Lang, cur: Language) {
        let auto = l.t("Otomatis", "Automatic");
        let opts = [(Language::Auto, auto), (Language::Id, "Indonesia"), (Language::En, "English")];
        let widths: Vec<f32> = opts.iter().map(|(_, t)| self.p.gfx.measure(t, 13.0, 500, Font::Sans) + 28.0).collect();
        let total: f32 = widths.iter().sum::<f32>() + 6.0;
        self.row_text(y, l.t("Bahasa", "Language"), l.t("Otomatis = ikut Windows", "Automatic = follow Windows"), total + 8.0);
        let (mut x, sy) = (CX0 + CW - 18.0 - total, y + (ROW - 34.0) / 2.0);
        self.p.fill_rrect(x, sy, total, 34.0, 17.0, GLASS_FILL);
        self.p.stroke_rrect(x, sy, total, 34.0, 17.0, GLASS_BORDER, 1.0);
        x += 3.0;
        for ((lang, text), w) in opts.into_iter().zip(widths) {
            let active = lang == cur;
            let hov = self.hit(x, sy + 3.0, w, 28.0, SAction::Language(lang));
            if active {
                self.p.fill_rrect(x, sy + 3.0, w, 28.0, 14.0, ICE);
            } else if hov {
                self.p.fill_rrect(x, sy + 3.0, w, 28.0, 14.0, GLASS_FILL_HOVER);
            }
            self.p.text(text, x, sy + 3.0, w, 28.0, 13.0, 500, Font::Sans, Align::Center, if active { NAVY } else { TEXT_2 });
            x += w;
        }
    }

    fn number_row(&mut self, y: f32, label: &str, desc: &str, n: Number, s: &Settings, l: Lang) {
        self.row_text(y, label, desc, 150.0);
        let v = n.get(s);
        let (lo, hi, _) = n.range();
        let cy = y + ROW / 2.0;
        let plus_x = CX0 + CW - 18.0 - 32.0;
        let minus_x = plus_x - 76.0 - 32.0;
        for (x, d, glyph, enabled) in [(minus_x, -1, G_MINUS, v > lo), (plus_x, 1, G_ADD, v < hi)] {
            let hov = self.hit(x, cy - 16.0, 32.0, 32.0, SAction::Step(n, d));
            self.p.fill_circle(x + 16.0, cy, 16.0, if hov && enabled { GLASS_FILL_HOVER } else { GLASS_FILL });
            self.p.stroke_circle(x + 16.0, cy, 15.5, GLASS_BORDER, 1.0, false);
            self.p.icon(glyph, x + 16.0, cy, 11.0, if enabled { TEXT_1 } else { TEXT_3.with_alpha(0.35) });
        }
        self.p.text(&n.format(v, l), minus_x + 32.0, y, 76.0, ROW, 14.0, 500, Font::Sans, Align::Center, TEXT_1);
    }

    fn button_row(&mut self, y: f32, label: &str, desc: &str, button: &str, action: SAction) {
        let bw = self.p.gfx.measure(button, 13.0, 500, Font::Sans) + 32.0;
        self.row_text(y, label, desc, bw + 8.0);
        let (bx, by) = (CX0 + CW - 18.0 - bw, y + (ROW - 32.0) / 2.0);
        let hov = self.hit(bx, by, bw, 32.0, action);
        self.p.fill_rrect(bx, by, bw, 32.0, 16.0, if hov { GLASS_FILL_HOVER } else { GLASS_FILL });
        self.p.stroke_rrect(bx, by, bw, 32.0, 16.0, GLASS_BORDER, 1.0);
        self.p.text(button, bx, by, bw, 32.0, 13.0, 500, Font::Sans, Align::Center, TEXT_1);
    }

    fn info_row(&self, y: f32, label: &str, value: &str) {
        let vw = 260.0;
        self.row_text(y, label, "", vw);
        self.p.text(value, CX0 + CW - 18.0 - vw, y, vw, ROW, 13.0, 400, Font::Sans, Align::Right, TEXT_2);
    }

    /// Kartu berisi `rows` baris dengan garis pemisah; kembalikan y baris pertama.
    fn card(&self, y: f32, rows: usize) -> f32 {
        let h = rows as f32 * ROW;
        self.p.fill_rrect(CX0, y, CW, h, 16.0, CARD_FILL);
        self.p.stroke_rrect(CX0, y, CW, h, 16.0, CARD_BORDER, 1.0);
        for i in 1..rows {
            let ly = y + i as f32 * ROW;
            self.p.line(CX0 + 16.0, ly, CX0 + CW - 16.0, ly, Rgba(1.0, 1.0, 1.0, 0.07), 1.0);
        }
        y
    }
}

fn draw_all(b: &mut B, section: Section, v: &View) {
    let p = b.p;
    let l = v.lang;
    // menu samping
    p.fill_rrect(0.0, 0.0, SIDEBAR, H, 0.0, Rgba(0.0, 0.0, 0.0, 0.16));
    p.line(SIDEBAR, 0.0, SIDEBAR, H, Rgba(1.0, 1.0, 1.0, 0.08), 1.0);
    p.text("Topline", 28.0, 22.0, 160.0, 30.0, 20.0, 500, Font::Sans, Align::Left, TEXT_1);
    p.label(l.t("Pengaturan", "Settings"), 28.0, 52.0, 160.0, 16.0, 11.0, Align::Left, TEXT_3);
    for (i, s) in Section::ALL.iter().enumerate() {
        let (x, y, w, h) = (14.0, 88.0 + i as f32 * 44.0, SIDEBAR - 28.0, 40.0);
        let active = *s == section;
        let hov = b.hit(x, y, w, h, SAction::Nav(*s));
        if active {
            p.fill_rrect(x, y, w, h, 20.0, GLASS_FILL_HOVER);
        } else if hov {
            p.fill_rrect(x, y, w, h, 20.0, Rgba(1.0, 1.0, 1.0, 0.07));
        }
        let col = if active { TEXT_1 } else { TEXT_2 };
        let (ix, iy) = (x + 22.0, y + h / 2.0);
        match s {
            Section::Water => p.droplet(ix, iy, 15.0, col, false),
            _ => p.icon(section_glyph(*s), ix, iy, 14.0, col),
        }
        p.text(s.title(l), x + 44.0, y, w - 50.0, h, 14.0, if active { 500 } else { 400 }, Font::Sans, Align::Left, col);
    }
    p.label(&format!("{} {}", l.t("Versi", "Version"), env!("CARGO_PKG_VERSION")), 28.0, H - 38.0, 160.0, 16.0, 11.0, Align::Left, TEXT_3);

    // tombol tutup
    let (cx, cy) = (W - 34.0, 30.0);
    let hov = b.hit(cx - 16.0, cy - 16.0, 32.0, 32.0, SAction::Close);
    if hov {
        p.fill_circle(cx, cy, 16.0, GLASS_FILL_HOVER);
    }
    p.icon(G_CANCEL, cx, cy, 11.0, TEXT_1);

    // judul bagian
    p.text(section.title(l), CX0, 22.0, CW - 60.0, 32.0, 22.0, 500, Font::Sans, Align::Left, TEXT_1);
    p.text(section.desc(l), CX0, 54.0, CW, 20.0, 13.0, 400, Font::Sans, Align::Left, TEXT_2);

    let s = v.s;
    let y0 = 92.0;
    let row = |i: usize| y0 + i as f32 * ROW;
    let t = |b: &mut B, i: usize, label: &str, desc: &str, tg: Toggle| b.toggle_row(row(i), label, desc, tg.get(s), SAction::Toggle(tg));
    match section {
        Section::General => {
            b.card(y0, 6);
            b.language_row(row(0), l, s.language);
            b.toggle_row(
                row(1),
                l.t("Mulai bersama Windows", "Start with Windows"),
                l.t("Jalankan Topline saat Anda masuk ke Windows", "Run Topline when you sign in to Windows"),
                v.autostart,
                SAction::Autostart,
            );
            t(b, 2, l.t("Buka saat kursor mendekat", "Open on hover"), l.t("Notch terbuka saat kursor diam di atasnya", "The notch opens when the cursor rests on it"), Toggle::OpenOnHover);
            t(
                b,
                3,
                l.t("Gestur scroll di notch", "Scroll gestures on the notch"),
                l.t("Volume, ganti lagu, pindah tab; klik tengah untuk play/pause", "Volume, change track, switch tabs; middle-click to play/pause"),
                Toggle::ScrollGestures,
            );
            t(b, 4, l.t("Peek sistem", "System peeks"), l.t("Volume, baterai, Wi-Fi, Caps Lock, kamera dan mikrofon", "Volume, battery, Wi-Fi, Caps Lock, camera and microphone"), Toggle::SystemPeeks);
            t(
                b,
                5,
                l.t("Riwayat clipboard", "Clipboard history"),
                l.t("Hanya di memori; isi dari pengelola sandi diabaikan", "Memory only; password manager content is ignored"),
                Toggle::ClipboardHistory,
            );
        }
        Section::Appearance => {
            b.card(y0, 3);
            t(b, 0, l.t("Mode tanpa kaca", "No-glass mode"), l.t("Latar navy pekat, lebih hemat GPU", "Solid navy background, lighter on the GPU"), Toggle::Solid);
            t(b, 1, l.t("Mata Personality", "Personality eyes"), l.t("Sepasang mata menggantikan jam saat idle", "A pair of eyes replaces the clock when idle"), Toggle::Personality);
            b.number_row(
                row(2),
                l.t("Mata tertidur setelah", "Eyes fall asleep after"),
                l.t("Tanpa input selama ini (malam hari: 1 menit)", "This long without input (at night: 1 minute)"),
                Number::EyeSleepMinutes,
                s,
                l,
            );
            let ly = row(3) + 18.0;
            p.label(l.t("Tab yang tampil", "Visible tabs"), CX0, ly, CW, 16.0, 11.0, Align::Left, TEXT_2);
            let (mut x, mut y) = (CX0, ly + 26.0);
            for w in Widget::ALL.into_iter().filter(|w| *w != Widget::Music) {
                let shown = !s.hidden_tabs.iter().any(|k| k == w.key());
                let cw = p.gfx.measure(w.label(l), 13.0, 500, Font::Sans) + if shown { 50.0 } else { 32.0 };
                if x + cw > CX0 + CW {
                    x = CX0;
                    y += 42.0;
                }
                let hov = b.hit(x, y, cw, 34.0, SAction::Tab(w));
                let fill = if shown { SKY.with_alpha(if hov { 0.30 } else { 0.20 }) } else if hov { GLASS_FILL_HOVER } else { GLASS_FILL };
                p.fill_rrect(x, y, cw, 34.0, 17.0, fill);
                p.stroke_rrect(x, y, cw, 34.0, 17.0, if shown { SKY.with_alpha(0.7) } else { GLASS_BORDER }, 1.0);
                if shown {
                    p.icon(G_CHECK, x + 20.0, y + 17.0, 11.0, ICE);
                    p.text(w.label(l), x + 34.0, y, cw - 40.0, 34.0, 13.0, 500, Font::Sans, Align::Left, TEXT_1);
                } else {
                    p.text(w.label(l), x, y, cw, 34.0, 13.0, 500, Font::Sans, Align::Center, TEXT_2);
                }
                x += cw + 8.0;
            }
            p.text(
                l.t(
                    "Musik selalu tampil. Sholat butuh kota, Clipboard butuh riwayat aktif.",
                    "Music is always shown. Prayer needs a city, Clipboard needs history on.",
                ),
                CX0,
                y + 44.0,
                CW,
                18.0,
                12.0,
                400,
                Font::Sans,
                Align::Left,
                TEXT_3,
            );
        }
        Section::Focus => {
            b.card(y0, 3);
            b.number_row(row(0), l.t("Durasi fokus", "Focus length"), l.t("Satu sesi kerja", "One work session"), Number::FocusMin, s, l);
            b.number_row(row(1), l.t("Durasi istirahat", "Break length"), l.t("Setelah setiap sesi fokus", "After each focus session"), Number::BreakMin, s, l);
            b.number_row(
                row(2),
                l.t("Target sesi", "Session goal"),
                l.t("Jumlah sesi sebelum hitungan diulang", "Sessions before the count resets"),
                Number::SessionsTarget,
                s,
                l,
            );
        }
        Section::Water => {
            b.card(y0, 4);
            b.number_row(
                row(0),
                l.t("Target harian", "Daily goal"),
                l.t("Pengingat berhenti saat target tercapai", "Reminders stop once the goal is reached"),
                Number::WaterGoalMl,
                s,
                l,
            );
            b.number_row(
                row(1),
                l.t("Interval pengingat", "Reminder interval"),
                l.t("Dihitung dari minum atau pengingat terakhir", "Counted from the last drink or reminder"),
                Number::WaterIntervalMin,
                s,
                l,
            );
            b.number_row(row(2), l.t("Pengingat mulai", "Reminders start"), "", Number::WaterStartHour, s, l);
            b.number_row(row(3), l.t("Pengingat selesai", "Reminders end"), "", Number::WaterEndHour, s, l);
        }
        Section::Prayer => {
            b.card(y0, 3);
            let place = if s.prayer_city.is_empty() { l.t("Nonaktif", "Off").to_string() } else { v.prayer_place.clone() };
            b.button_row(row(0), l.t("Kota", "City"), &place, l.t("Pilih kota", "Choose city"), SAction::ChooseCity);
            b.button_row(
                row(1),
                l.t("Lokasi saat ini", "Current location"),
                l.t("Dibaca sekali dari layanan lokasi Windows", "Read once from Windows location services"),
                l.t("Perbarui lokasi", "Update location"),
                SAction::Locate,
            );
            t(
                b,
                2,
                l.t("Suara adzan", "Adhan sound"),
                l.t("adzan.wav di folder data, atau suara pengingat Windows", "adzan.wav in the data folder, or the Windows reminder sound"),
                Toggle::AdzanSound,
            );
        }
        Section::Lyrics => {
            b.card(y0, 4);
            t(
                b,
                0,
                l.t("Cari lirik online", "Find lyrics online"),
                l.t("Dari lrclib.net; yang dikirim hanya judul, artis, durasi", "From lrclib.net; only title, artist, and length are sent"),
                Toggle::LyricsOnline,
            );
            t(b, 1, l.t("Caption di bawah notch", "Caption under the notch"), l.t("Satu baris lirik saat musik diputar", "One line of lyrics while music plays"), Toggle::LyricsCaption);
            b.button_row(
                row(2),
                l.t("Folder lirik", "Lyrics folder"),
                l.t("Taruh 'Artis - Judul.lrc' untuk lirik sendiri", "Add 'Artist - Title.lrc' for your own lyrics"),
                l.t("Buka folder", "Open folder"),
                SAction::LyricsFolder,
            );
            let cache = match l {
                Lang::Id => format!("{} lagu tersimpan di komputer ini", v.lyrics_cache),
                Lang::En => format!("{} songs saved on this PC", v.lyrics_cache),
            };
            b.button_row(row(3), l.t("Cache lirik", "Lyrics cache"), &cache, l.t("Hapus cache", "Clear cache"), SAction::ClearLyrics);
        }
        Section::About => {
            b.card(y0, 4);
            b.info_row(row(0), l.t("Versi", "Version"), &format!("Topline {}", env!("CARGO_PKG_VERSION")));
            b.info_row(row(1), l.t("Buka / tutup notch", "Open / close notch"), "Win + Alt + T");
            b.info_row(row(2), l.t("Tambah tugas", "Add task"), "Win + Alt + N");
            b.button_row(
                row(3),
                l.t("Folder data", "Data folder"),
                l.t("data.json, ikon launcher, cache lirik", "data.json, launcher icons, lyrics cache"),
                l.t("Buka folder", "Open folder"),
                SAction::DataFolder,
            );
            p.text(
                l.t("Rust native · Windows.UI.Composition · tanpa WebView.", "Native Rust · Windows.UI.Composition · no WebView."),
                CX0,
                row(4) + 18.0,
                CW,
                18.0,
                12.0,
                400,
                Font::Sans,
                Align::Left,
                TEXT_3,
            );
        }
    }
}

fn section_glyph(s: Section) -> char {
    match s {
        Section::General => G_SETTINGS,
        Section::Appearance => G_COLOR,
        Section::Focus => G_TIMER,
        Section::Water => G_TIMER,
        Section::Prayer => G_MOON,
        Section::Lyrics => G_MUSIC,
        Section::About => G_INFO,
    }
}
