//! Tata letak dan penggambaran semua tampilan notch (idle, peek, widget).
//! Semua koordinat dalam DIP, relatif terhadap jendela visual 640x540.

use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

use topline_core::*;

use crate::gfx::{Align, Font, Painter};
use crate::launcher::Icons;
use crate::media::{MediaCmd, MediaInfo};
use crate::theme::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Cmd(Command),
    Media(MediaCmd),
    /// Tambah 250 ml langsung dari peek pengingat.
    PeekWater,
    /// Menu konteks tile launcher.
    PinMenu(u64),
    OpenSettings,
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub action: Action,
    /// Aksi klik kanan (mis. hapus tugas).
    pub alt: Option<Action>,
    /// Tile launcher yang bisa diseret untuk mengatur ulang urutan.
    pub drag: Option<u64>,
}

impl Hit {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

pub struct Ctx<'a> {
    pub core: &'a Core,
    pub now: &'a Clock,
    pub media: Option<&'a MediaInfo>,
    pub cover: Option<&'a ID2D1Bitmap>,
    pub hover: Option<&'a Action>,
    pub icons: &'a Icons,
    /// Sematan yang targetnya tidak ditemukan (dicek saat launcher digambar).
    pub missing: &'a [u64],
    /// Tile yang sedang diseret dan indeks tujuannya.
    pub reorder: Option<(u64, usize)>,
}

pub struct Frame {
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub hits: Vec<Hit>,
    /// Posisi kolom EDIT untuk tugas baru (DIP), jika sedang menambah.
    pub edit: Option<(f32, f32, f32, f32)>,
    /// Equalizer DWM ditampilkan (idle + musik diputar).
    pub eq: bool,
    /// Mata Personality (visual DWM di notch.rs) dan ekspresinya; None = disembunyikan.
    pub eyes: Option<EyeMood>,
    /// Area gestur scroll (x, y, w, h, zona); di luar area ini scroll diabaikan.
    pub scroll: Vec<(f32, f32, f32, f32, ScrollZone)>,
    /// Kotak slot tile launcher, urut indeks (untuk drag atur ulang).
    pub slots: Vec<(f32, f32, f32, f32)>,
}

impl Ctx<'_> {
    /// Teks sesuai bahasa UI.
    fn t<'s>(&self, id: &'s str, en: &'s str) -> &'s str {
        self.core.lang.t(id, en)
    }
}

fn todo_body_h(c: &Ctx) -> f32 {
    let rows = c.core.visible_todos(c.now).len().max(1) as f32;
    28.0 + 4.0 + rows * 44.0 + 6.0 + 44.0
}

fn clip_body_h(c: &Ctx) -> f32 {
    let rows = c.core.visible_clips().len().max(1) as f32;
    28.0 + 4.0 + rows * 44.0
}

fn launcher_body_h(c: &Ctx) -> f32 {
    match c.core.data.pins.len() {
        0 => 120.0,
        n if n <= PIN_COLS => PIN_H,
        _ => PIN_H * 2.0 + PIN_GAP,
    }
}

fn body_h(c: &Ctx, w: Widget) -> f32 {
    match w {
        Widget::Lyrics => LYRICS_BODY_H,
        Widget::Launcher => launcher_body_h(c),
        Widget::Music => 164.0,
        Widget::Focus => 120.0,
        Widget::Water => 168.0,
        Widget::Todo => todo_body_h(c),
        Widget::Prayer => 168.0,
        Widget::Clipboard => clip_body_h(c),
    }
}

fn idle_wide(c: &Ctx) -> bool {
    !c.core.data.settings.personality && (c.media.is_some() || c.core.focus.running)
}

/// Baris lirik di bawah pill idle (caption), jika aktif.
fn caption(c: &Ctx) -> Option<String> {
    c.core.caption(c.media?.position_now(c.now.ms))
}

/// Ukuran notch untuk state saat ini.
pub fn size_for(c: &Ctx) -> (f32, f32, f32) {
    match c.core.view {
        View::Idle if caption(c).is_some() => (CAPTION_W, CAPTION_H, IDLE_R),
        View::Idle => {
            let w = if idle_wide(c) { IDLE_W_WIDE } else { IDLE_W_SMALL };
            (w, IDLE_H, IDLE_R)
        }
        View::Peek => (PEEK_W, PEEK_H, PEEK_R),
        View::Expanded(wd) => {
            let h = PAD_TOP + HEADER_H + GAP + body_h(c, wd) + GAP + TABS_H + PAD_BOTTOM;
            (EXP_W, h, EXP_R)
        }
    }
}

struct B<'p, 'a> {
    p: &'p Painter<'a>,
    hits: Vec<Hit>,
    hover: Option<Action>,
}

impl<'p, 'a> B<'p, 'a> {
    fn hit(&mut self, x: f32, y: f32, w: f32, h: f32, action: Action) -> bool {
        let hov = self.hover.as_ref() == Some(&action);
        self.hits.push(Hit { x, y, w, h, action, alt: None, drag: None });
        hov
    }
    fn hit_alt(&mut self, x: f32, y: f32, w: f32, h: f32, action: Action, alt: Action) -> bool {
        let hov = self.hover.as_ref() == Some(&action);
        self.hits.push(Hit { x, y, w, h, action, alt: Some(alt), drag: None });
        hov
    }

    /// Tombol pill sekunder (kaca).
    fn btn_glass(&mut self, x: f32, y: f32, w: f32, h: f32, label: &str, glyph: Option<char>, action: Action) {
        let hov = self.hit(x, y, w, h, action);
        self.p.fill_rrect(x, y, w, h, h / 2.0, if hov { GLASS_FILL_HOVER } else { GLASS_FILL });
        self.p.stroke_rrect(x, y, w, h, h / 2.0, GLASS_BORDER, 1.0);
        self.pill_label(x, y, w, h, label, glyph, TEXT_1);
    }

    /// Tombol pill utama (putih, teks navy).
    fn btn_primary(&mut self, x: f32, y: f32, w: f32, h: f32, label: &str, glyph: Option<char>, action: Action) {
        let hov = self.hit(x, y, w, h, action);
        self.p.fill_rrect(x, y, w, h, h / 2.0, if hov { ICE.with_alpha(0.88) } else { ICE });
        self.pill_label(x, y, w, h, label, glyph, NAVY);
    }

    fn pill_label(&self, x: f32, y: f32, w: f32, h: f32, label: &str, glyph: Option<char>, c: Rgba) {
        match glyph {
            Some(g) if !label.is_empty() => {
                let tw = self.p.gfx.measure(label, 14.0, 500, Font::Sans);
                let total = 14.0 + 6.0 + tw;
                let sx = x + (w - total) / 2.0;
                self.p.icon(g, sx + 7.0, y + h / 2.0, 12.0, c);
                self.p.text(label, sx + 20.0, y, tw + 4.0, h, 14.0, 500, Font::Sans, Align::Left, c);
            }
            Some(g) => self.p.icon(g, x + w / 2.0, y + h / 2.0, 14.0, c),
            None => self.p.text(label, x, y, w, h, 14.0, 500, Font::Sans, Align::Center, c),
        }
    }

    fn icon_btn(&mut self, cx: f32, cy: f32, d: f32, glyph: char, size: f32, action: Action) {
        let hov = self.hit(cx - d / 2.0, cy - d / 2.0, d, d, action);
        if hov {
            self.p.fill_circle(cx, cy, d / 2.0, GLASS_FILL_HOVER);
        }
        self.p.icon(glyph, cx, cy, size, TEXT_1);
    }
}

pub fn draw(p: &Painter, c: &Ctx) -> Frame {
    let (w, h, r) = size_for(c);
    let nx = (WIN_W - w) / 2.0;
    let mut b = B { p, hits: Vec::new(), hover: c.hover.cloned() };
    let mut edit = None;
    let mut eq = false;
    let mut scroll = Vec::new();
    let mut slots = Vec::new();

    // Garis tepi kaca + highlight (sudut atas berada di luar layar).
    p.stroke_rrect(nx, -r, w, h + r, r, GLASS_BORDER, 1.0);

    match c.core.view {
        View::Idle => {
            let cap = caption(c);
            // Dengan caption, isi pill tetap di tengah atas; lirik di baris kedua.
            let pw = if cap.is_none() { w } else if idle_wide(c) { IDLE_W_WIDE } else { IDLE_W_SMALL };
            eq = draw_idle(&mut b, c, (WIN_W - pw) / 2.0, pw);
            if let Some(t) = cap {
                p.text(&t, nx + 18.0, IDLE_H - 6.0, w - 36.0, CAPTION_H - IDLE_H, 13.0, 500, Font::Sans, Align::Center, TEXT_1.with_alpha(0.92));
            }
            scroll.push((nx, 0.0, w, h, ScrollZone::Notch));
        }
        View::Peek => {
            draw_peek(&mut b, c, nx, w);
            scroll.push((nx, 0.0, w, h, ScrollZone::Notch));
        }
        View::Expanded(wd) => {
            draw_header(&mut b, c, nx);
            let x0 = nx + PAD_X;
            let y0 = PAD_TOP + HEADER_H + GAP;
            scroll.push((nx, h - PAD_BOTTOM - TABS_H, w, TABS_H + PAD_BOTTOM, ScrollZone::Tabs));
            if wd == Widget::Music {
                scroll.push((nx, y0 - GAP / 2.0, w, body_h(c, wd) + GAP, ScrollZone::Music));
            }
            match wd {
                Widget::Music => draw_music(&mut b, c, x0, y0),
                Widget::Focus => draw_focus(&mut b, c, x0, y0),
                Widget::Water => draw_water(&mut b, c, x0, y0),
                Widget::Todo => edit = draw_todo(&mut b, c, x0, y0),
                Widget::Prayer => draw_prayer(&mut b, c, x0, y0),
                Widget::Clipboard => draw_clipboard(&mut b, c, x0, y0),
                Widget::Launcher => slots = draw_launcher(&mut b, c, x0, y0),
                Widget::Lyrics => draw_lyrics(&mut b, c, x0, y0),
            }
            draw_tabs(&mut b, c, wd, h);
        }
    }
    let eyes = (c.core.view == View::Idle && c.core.data.settings.personality).then(|| c.core.eye_mood(c.now));
    Frame { w, h, r, hits: b.hits, edit, eq, eyes, scroll, slots }
}

// ------------------------------------------------------------------ idle

fn draw_idle(b: &mut B, c: &Ctx, nx: f32, w: f32) -> bool {
    let p = b.p;
    let cy = IDLE_H / 2.0;
    // Titik privasi di ujung kanan: hijau = kamera, oranye = mikrofon.
    let mut dx = nx + w - 9.0;
    for (on, col) in [(c.core.mic.is_some(), AMBER), (c.core.camera.is_some(), MINT)] {
        if on {
            p.fill_circle(dx, cy, 3.0, col);
            dx -= 9.0;
        }
    }
    if c.core.data.settings.personality {
        // Mata digambar dan dianimasikan oleh DWM (notch.rs), bukan Direct2D.
        return false;
    }
    let clock = c.now.hhmm();
    if !idle_wide(c) {
        p.text(&clock, nx, 0.0, w, IDLE_H, 14.0, 500, Font::Sans, Align::Center, TEXT_1);
        return false;
    }
    p.text(&clock, nx, 0.0, w, IDLE_H, 14.0, 500, Font::Sans, Align::Center, TEXT_1);
    let lx = nx + 14.0;
    if c.core.focus.running {
        p.icon(G_TIMER, lx + 10.0, cy, 13.0, SKY);
        let mins = (c.core.focus.remaining(c.now.ms) + 59_999) / 60_000;
        let lbl = format!("{}m", mins);
        p.label(&lbl, nx + w - 14.0 - 44.0, cy - 8.0, 44.0, 16.0, 12.0, Align::Right, TEXT_2);
        false
    } else if let Some(m) = c.media {
        if let Some(bmp) = c.cover {
            p.bitmap_rrect(bmp, lx, cy - 10.0, 20.0, 20.0, 6.0);
        } else {
            p.gradient_rrect(lx, cy - 10.0, 20.0, 20.0, 6.0, &[(0.0, SKY), (1.0, ULTRAMARINE)], (0.0, 0.0), (1.0, 1.0));
            p.icon(G_MUSIC, lx + 10.0, cy, 10.0, ICE);
        }
        if !m.playing {
            p.icon(G_PAUSE, nx + w - 14.0 - 9.0, cy, 11.0, TEXT_2);
        }
        m.playing
    } else {
        false
    }
}

// ------------------------------------------------------------------ peek

fn draw_peek(b: &mut B, c: &Ctx, nx: f32, w: f32) {
    let Some(peek) = &c.core.peek else { return };
    let p = b.p;
    let h = PEEK_H;
    let cy = h / 2.0;
    let ix = nx + 26.0;
    let tx = nx + 46.0;
    let right = nx + w - 16.0;
    // seluruh peek bisa diklik
    b.hits.push(Hit { x: nx, y: 0.0, w, h, action: Action::Cmd(Command::ActivatePeek), alt: Some(Action::Cmd(Command::DismissPeek)), drag: None });
    let text = |s: &str, c: Rgba, width: f32| p.text(s, tx, 0.0, width, h, 14.0, 400, Font::Sans, Align::Left, c);
    let value = |s: &str, c: Rgba| p.text(s, right - 80.0, 0.0, 80.0, h, 14.0, 500, Font::Sans, Align::Right, c);
    match &peek.kind {
        PeekKind::Volume { percent, muted } => {
            p.icon(if *muted { G_MUTE } else { G_VOLUME }, ix, cy, 15.0, TEXT_1);
            let bx = tx;
            let bw = w - 46.0 - 16.0 - 44.0;
            p.fill_rrect(bx, cy - 3.0, bw, 6.0, 3.0, TRACK);
            let f = if *muted { 0.0 } else { *percent as f32 / 100.0 };
            if f > 0.0 {
                p.fill_rrect(bx, cy - 3.0, (bw * f).max(6.0), 6.0, 3.0, SKY);
            }
            value(&format!("{}", if *muted { 0 } else { *percent }), TEXT_1);
        }
        PeekKind::Charging { percent } => {
            p.icon(G_BOLT, ix, cy, 15.0, MINT);
            text(c.t("Mengisi daya", "Charging"), TEXT_1, 180.0);
            value(&format!("{}%", percent), MINT);
        }
        PeekKind::Unplugged { percent } => {
            p.icon(G_BOLT, ix, cy, 15.0, TEXT_2);
            text(c.t("Charger dilepas", "Charger unplugged"), TEXT_1, 180.0);
            value(&format!("{}%", percent), TEXT_1);
        }
        PeekKind::BatteryLow { percent } => {
            p.icon(G_BOLT, ix, cy, 15.0, AMBER);
            text(c.t("Baterai lemah", "Battery low"), AMBER, 180.0);
            value(&format!("{}%", percent), AMBER);
        }
        PeekKind::NetworkLost => {
            p.icon(G_WIFI, ix, cy, 15.0, AMBER);
            text(c.t("Koneksi terputus", "Connection lost"), AMBER, 220.0);
        }
        PeekKind::NetworkBack => {
            p.icon(G_WIFI, ix, cy, 15.0, SKY);
            text(c.t("Terhubung kembali", "Back online"), TEXT_1, 220.0);
        }
        PeekKind::CapsLock { on } => {
            p.icon(G_KEYBOARD, ix, cy, 15.0, TEXT_1);
            text("Caps Lock", TEXT_1, 160.0);
            let (lbl, bg, fg) = if *on { (c.t("Aktif", "On"), ICE, NAVY) } else { (c.t("Mati", "Off"), GLASS_FILL, TEXT_2) };
            p.fill_rrect(right - 64.0, cy - 12.0, 64.0, 24.0, 12.0, bg);
            p.label(lbl, right - 64.0, cy - 12.0, 64.0, 24.0, 11.0, Align::Center, fg);
        }
        PeekKind::FocusDone => {
            p.icon(G_TIMER, ix, cy, 15.0, SKY);
            let m = c.core.data.settings.break_min;
            text(&if c.core.lang.is_en() { format!("Focus done · {} min break", m) } else { format!("Fokus selesai · istirahat {} mnt", m) }, TEXT_1, w - 62.0);
        }
        PeekKind::BreakDone => {
            p.icon(G_TIMER, ix, cy, 15.0, MINT);
            text(c.t("Istirahat selesai · siap fokus lagi", "Break over · ready to focus"), TEXT_1, w - 62.0);
        }
        PeekKind::WaterReminder => {
            p.droplet(ix, cy, 16.0, SKY, true);
            text(c.t("Waktunya minum", "Time to drink water"), TEXT_1, 160.0);
            let bw = 84.0;
            let bx = right - bw + 6.0;
            let hov = b.hover.as_ref() == Some(&Action::PeekWater);
            // tombol ditambahkan setelah hit peek agar menang saat dicari dari belakang
            b.hits.push(Hit { x: bx, y: cy - 14.0, w: bw, h: 28.0, action: Action::PeekWater, alt: None, drag: None });
            p.fill_rrect(bx, cy - 14.0, bw, 28.0, 14.0, if hov { ICE.with_alpha(0.88) } else { ICE });
            p.text("+250 ml", bx, cy - 14.0, bw, 28.0, 13.0, 500, Font::Sans, Align::Center, NAVY);
        }
        PeekKind::TodoDue { text: t, time } => {
            p.icon(G_BELL, ix, cy, 15.0, TEXT_1);
            text(t, TEXT_1, w - 62.0 - 64.0);
            p.label(time, right - 60.0, cy - 8.0, 60.0, 16.0, 12.0, Align::Right, TEXT_2);
        }
        PeekKind::Prayer { name, time } => {
            p.icon(G_MOON, ix, cy, 15.0, SKY);
            text(&format!("{} {}", c.t("Waktunya sholat", "Time for"), name), TEXT_1, w - 62.0 - 64.0);
            p.label(time, right - 60.0, cy - 8.0, 60.0, 16.0, 12.0, Align::Right, TEXT_2);
        }
        PeekKind::CamMic { camera, app } => {
            let (g, col, what) = if *camera { (G_CAMERA, MINT, c.t("Kamera", "Camera")) } else { (G_MIC, AMBER, c.t("Mikrofon", "Microphone")) };
            p.icon(g, ix, cy, 15.0, col);
            text(&format!("{} {} · {}", what, c.t("dipakai", "in use"), app), TEXT_1, w - 62.0);
        }
        PeekKind::Progress { label, percent } => {
            p.icon(G_CODE, ix, cy, 15.0, SKY);
            match percent {
                Some(pc) => {
                    text(label, TEXT_1, 120.0);
                    let bx = tx + 128.0;
                    let bw = right - 48.0 - bx;
                    p.fill_rrect(bx, cy - 3.0, bw, 6.0, 3.0, TRACK);
                    if *pc > 0 {
                        p.fill_rrect(bx, cy - 3.0, (bw * *pc as f32 / 100.0).max(6.0), 6.0, 3.0, SKY);
                    }
                    value(&format!("{}%", pc), TEXT_1);
                }
                None => {
                    text(label, TEXT_1, w - 62.0 - 90.0);
                    p.label(c.t("berjalan", "running"), right - 86.0, cy - 8.0, 86.0, 16.0, 11.0, Align::Right, TEXT_2);
                }
            }
        }
        PeekKind::ProgressDone { label, ok } => {
            let (g, col, what) = if *ok { (G_CHECK, MINT, c.t("selesai", "done")) } else { (G_CANCEL, AMBER, c.t("gagal", "failed")) };
            p.icon(g, ix, cy, 15.0, col);
            text(&format!("{} {}", label, what), if *ok { TEXT_1 } else { AMBER }, w - 62.0);
        }
        PeekKind::Location { place } => match place {
            Some(pl) => {
                p.icon(G_LOCATION, ix, cy, 15.0, SKY);
                text(&format!("{}: {}", c.t("Jadwal sholat", "Prayer times"), pl), TEXT_1, w - 62.0);
            }
            None => {
                p.icon(G_LOCATION, ix, cy, 15.0, AMBER);
                text(c.t("Lokasi tidak tersedia · cek izin Lokasi", "Location unavailable · check Location permission"), AMBER, w - 62.0);
            }
        },
        PeekKind::DownloadDone { name } => {
            p.icon(G_DOWNLOAD, ix, cy, 15.0, SKY);
            text(&format!("{} · {}", c.t("Unduhan selesai", "Download complete"), name), TEXT_1, w - 62.0);
        }
        PeekKind::Track { title, next } => {
            p.icon(if *next { G_NEXT } else { G_PREV }, ix, cy, 14.0, SKY);
            let t = match (title.is_empty(), *next) {
                (false, _) => title.as_str(),
                (true, true) => c.t("Lagu berikutnya", "Next track"),
                (true, false) => c.t("Lagu sebelumnya", "Previous track"),
            };
            text(t, TEXT_1, w - 62.0);
        }
        PeekKind::LaunchFailed { name } => {
            p.icon(G_CANCEL, ix, cy, 15.0, AMBER);
            text(&format!("{} {}", c.t("Tidak bisa membuka", "Couldn't open"), name), TEXT_1, w - 62.0);
        }
    }
}

// ------------------------------------------------------------------ expanded

fn draw_header(b: &mut B, c: &Ctx, nx: f32) {
    let p = b.p;
    let y = PAD_TOP;
    match (&c.core.pin_notice, c.core.widget()) {
        // pesan launcher (mis. batas sematan) menggantikan label tanggal sementara
        (Some((msg, _)), Some(Widget::Launcher)) => {
            p.text(msg, nx + PAD_X, y, 236.0, HEADER_H, 12.0, 500, Font::Sans, Align::Left, AMBER)
        }
        _ => p.label(&c.now.short_date(c.core.lang), nx + PAD_X, y + 12.0, 200.0, 16.0, 12.0, Align::Left, TEXT_2),
    }
    p.text(&c.now.hhmm(), nx, y, EXP_W, HEADER_H, 15.0, 500, Font::Sans, Align::Center, TEXT_1);
    let bx = nx + EXP_W - PAD_X - 40.0;
    let hov = b.hit(bx, y, 40.0, 40.0, Action::Cmd(Command::Close));
    p.fill_circle(bx + 20.0, y + 20.0, 20.0, if hov { GLASS_FILL_HOVER } else { GLASS_FILL });
    p.stroke_circle(bx + 20.0, y + 20.0, 19.5, GLASS_BORDER, 1.0, false);
    p.icon(G_CHEVRON_UP, bx + 20.0, y + 20.0, 13.0, TEXT_1);
    // Pengaturan: ikon roda gigi tanpa lingkaran, agar tombol tutup tetap yang utama.
    let gx = bx - 44.0;
    let hov = b.hit(gx, y, 40.0, 40.0, Action::OpenSettings);
    if hov {
        p.fill_circle(gx + 20.0, y + 20.0, 20.0, GLASS_FILL_HOVER);
    }
    p.icon(G_SETTINGS, gx + 20.0, y + 20.0, 14.0, if hov { TEXT_1 } else { TEXT_2 });
}

fn fmt_ms(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn draw_music(b: &mut B, c: &Ctx, x0: f32, y0: f32) {
    let p = b.p;
    let inner = EXP_W - PAD_X * 2.0;
    let Some(m) = c.media else {
        p.gradient_rrect(x0, y0, 96.0, 96.0, 20.0, &[(0.0, SKY), (0.45, ELECTRIC), (1.0, NAVY)], (1.0, 1.0), (0.0, 0.0));
        p.icon(G_MUSIC, x0 + 48.0, y0 + 48.0, 30.0, ICE.with_alpha(0.9));
        p.text(c.t("Tidak ada musik yang diputar", "No music playing"), x0 + 112.0, y0 + 24.0, inner - 112.0, 26.0, 18.0, 500, Font::Sans, Align::Left, TEXT_1);
        p.text(c.t("Putar lagu di Spotify, YouTube, atau pemutar lain.", "Play a song in Spotify, YouTube, or any other player."), x0 + 112.0, y0 + 52.0, inner - 112.0, 20.0, 13.0, 400, Font::Sans, Align::Left, TEXT_2);
        return;
    };
    // cover
    if let Some(bmp) = c.cover {
        p.bitmap_rrect(bmp, x0, y0, 96.0, 96.0, 20.0);
    } else {
        p.gradient_rrect(x0, y0, 96.0, 96.0, 20.0, &[(0.0, SKY), (0.45, ELECTRIC), (1.0, NAVY)], (1.0, 1.0), (0.0, 0.0));
        p.icon(G_MUSIC, x0 + 48.0, y0 + 48.0, 30.0, ICE.with_alpha(0.9));
    }
    let x1 = x0 + 112.0;
    let tw = inner - 112.0;
    // status pill
    if !m.app.is_empty() {
        let lw = p.gfx.measure(&m.app, 12.0, 400, Font::Sans) + 30.0;
        let (bg, dot, fg) = if m.playing {
            (MINT.with_alpha(0.16), MINT, Rgba::hex(0x7FE0BE, 1.0))
        } else {
            (GLASS_FILL, TEXT_3, TEXT_2)
        };
        p.fill_rrect(x1, y0, lw, 22.0, 11.0, bg);
        p.fill_circle(x1 + 12.0, y0 + 11.0, 3.0, dot);
        p.text(&m.app, x1 + 20.0, y0, lw - 22.0, 22.0, 12.0, 400, Font::Sans, Align::Left, fg);
    }
    p.text(&m.title, x1, y0 + 26.0, tw, 26.0, 18.0, 500, Font::Sans, Align::Left, TEXT_1);
    p.text(&m.artist, x1, y0 + 52.0, tw, 20.0, 13.0, 400, Font::Sans, Align::Left, TEXT_2);
    // progress
    let pos = m.position_now(c.now.ms);
    p.fill_rrect(x1, y0 + 80.0, tw, 4.0, 2.0, TRACK);
    if m.duration_ms > 0 {
        let f = (pos as f32 / m.duration_ms as f32).clamp(0.0, 1.0);
        if f > 0.0 {
            p.fill_rrect(x1, y0 + 80.0, (tw * f).max(4.0), 4.0, 2.0, ICE);
        }
        p.label(&fmt_ms(pos), x1, y0 + 88.0, 80.0, 14.0, 11.0, Align::Left, TEXT_2);
        p.label(&fmt_ms(m.duration_ms), x1 + tw - 80.0, y0 + 88.0, 80.0, 14.0, 11.0, Align::Right, TEXT_2);
    }
    // kontrol
    let cy = y0 + 112.0 + 26.0;
    let cx = x0 + inner / 2.0;
    b.icon_btn(cx - 26.0 - 20.0 - 22.0, cy, 44.0, G_PREV, 16.0, Action::Media(MediaCmd::Prev));
    let hov = b.hit(cx - 26.0, cy - 26.0, 52.0, 52.0, Action::Media(MediaCmd::PlayPause));
    p.fill_circle(cx, cy, 26.0, if hov { ICE.with_alpha(0.88) } else { ICE });
    p.icon(if m.playing { G_PAUSE } else { G_PLAY }, cx, cy, 16.0, NAVY);
    b.icon_btn(cx + 26.0 + 20.0 + 22.0, cy, 44.0, G_NEXT, 16.0, Action::Media(MediaCmd::Next));
}

fn draw_focus(b: &mut B, c: &Ctx, x0: f32, y0: f32) {
    let p = b.p;
    let inner = EXP_W - PAD_X * 2.0;
    let f = &c.core.focus;
    let s = &c.core.data.settings;
    let total = match f.phase {
        FocusPhase::Focus => s.focus_min as i64 * 60_000,
        FocusPhase::Break => s.break_min as i64 * 60_000,
    }
    .max(1);
    let rem = f.remaining(c.now.ms);
    let accent = if f.phase == FocusPhase::Focus { SKY } else { MINT };
    let (rcx, rcy) = (x0 + 56.0, y0 + 60.0);
    p.stroke_circle(rcx, rcy, 46.0, TRACK, 8.0, false);
    p.arc(rcx, rcy, 46.0, 1.0 - rem as f32 / total as f32, 8.0, accent);
    p.icon(G_TIMER, rcx, rcy, 22.0, accent);

    let mx = x0 + 136.0;
    let lbl = match f.phase {
        FocusPhase::Focus => format!("{} {} / {}", c.t("Sesi", "Session"), (f.sessions_done + 1).min(s.sessions_target), s.sessions_target),
        FocusPhase::Break => c.t("Istirahat", "Break").to_string(),
    };
    p.label(&lbl, mx, y0 + 4.0, 200.0, 16.0, 12.0, Align::Left, TEXT_2);
    p.text(&f.label(c.now.ms), mx - 2.0, y0 + 22.0, 220.0, 60.0, 56.0, 400, Font::Sans, Align::Left, TEXT_1);
    // segmen
    let sy = y0 + 88.0;
    let seg_w = [76.0, 96.0];
    let total_w = seg_w[0] + seg_w[1] + 6.0;
    p.fill_rrect(mx, sy, total_w, 32.0, 16.0, GLASS_FILL);
    p.stroke_rrect(mx, sy, total_w, 32.0, 16.0, GLASS_BORDER, 1.0);
    let labels = [format!("{} {}", c.t("Fokus", "Focus"), s.focus_min), format!("{} {}", c.t("Istirahat", "Break"), s.break_min)];
    let mut sx = mx + 3.0;
    for (i, l) in labels.iter().enumerate() {
        let active = (i == 0) == (f.phase == FocusPhase::Focus);
        if active {
            p.fill_rrect(sx, sy + 3.0, seg_w[i], 26.0, 13.0, ICE);
        }
        p.text(l, sx, sy + 3.0, seg_w[i], 26.0, 12.0, 500, Font::Sans, Align::Center, if active { NAVY } else { TEXT_2 });
        sx += seg_w[i];
    }
    // tombol
    let bx = x0 + inner - 116.0;
    let start_lbl = if f.running { c.t("Jeda", "Pause") } else if rem < total { c.t("Lanjut", "Resume") } else { c.t("Mulai", "Start") };
    b.btn_primary(bx, y0 + 10.0, 116.0, 44.0, start_lbl, None, Action::Cmd(Command::FocusStartPause));
    if f.phase == FocusPhase::Break {
        b.btn_glass(bx, y0 + 64.0, 116.0, 44.0, c.t("Lewati", "Skip"), Some(G_SKIP), Action::Cmd(Command::FocusSkip));
    } else {
        b.btn_glass(bx, y0 + 64.0, 116.0, 44.0, c.t("Ulang", "Reset"), Some(G_RESET), Action::Cmd(Command::FocusReset));
    }
}

fn draw_water(b: &mut B, c: &Ctx, x0: f32, y0: f32) {
    let p = b.p;
    let inner = EXP_W - PAD_X * 2.0;
    let s = &c.core.data.settings;
    let total = c.core.data.water.total_ml;
    let pct = c.core.water_percent();
    let lw = inner - 196.0 - 24.0;
    p.label(c.t("Air minum hari ini", "Water today"), x0, y0, lw, 16.0, 12.0, Align::Left, TEXT_2);
    let big = c.core.lang.decimal(&format!("{:.2}", total as f32 / 1000.0));
    let bw = p.gfx.measure(&big, 48.0, 400, Font::Sans);
    p.text(&big, x0 - 2.0, y0 + 20.0, bw + 8.0, 56.0, 48.0, 400, Font::Sans, Align::Left, TEXT_1);
    let goal = c.core.lang.decimal(&format!("/ {:.1} L", s.water_goal_ml as f32 / 1000.0));
    p.text(&goal, x0 + bw + 10.0, y0 + 36.0, 120.0, 30.0, 15.0, 400, Font::Sans, Align::Left, TEXT_2);
    p.fill_rrect(x0, y0 + 82.0, lw, 8.0, 4.0, TRACK);
    if pct > 0 {
        let fw = (lw * pct as f32 / 100.0).max(8.0);
        p.gradient_rrect(x0, y0 + 82.0, fw, 8.0, 4.0, &[(0.0, ELECTRIC), (1.0, SKY)], (0.0, 0.5), (1.0, 0.5));
    }
    let next = match c.core.next_water_reminder() {
        Some(m) => format!("{}% {} {:02}:{:02}", pct, c.t("dari target · pengingat berikutnya", "of goal · next reminder"), m / 60, m % 60),
        None if pct >= 100 => c.t("Target hari ini tercapai", "Today's goal reached").to_string(),
        None => format!("{}% {}", pct, c.t("dari target", "of goal")),
    };
    p.text(&next, x0, y0 + 96.0, lw, 20.0, 13.0, 400, Font::Sans, Align::Left, TEXT_2);
    b.btn_primary(x0, y0 + 124.0, 112.0, 44.0, "250 ml", Some(G_ADD), Action::Cmd(Command::AddWater(250)));
    b.btn_glass(x0 + 120.0, y0 + 124.0, 112.0, 44.0, "+ 500 ml", None, Action::Cmd(Command::AddWater(500)));
    if !c.core.data.water.entries.is_empty() {
        b.btn_glass(x0 + 240.0, y0 + 124.0, 44.0, 44.0, "", Some(G_RESET), Action::Cmd(Command::UndoWater));
    }

    // grafik per jam (8 jam terakhir)
    let cx = x0 + inner - 196.0;
    p.fill_rrect(cx, y0, 196.0, 168.0, 20.0, CARD_FILL);
    p.stroke_rrect(cx, y0, 196.0, 168.0, 20.0, CARD_BORDER, 1.0);
    let end_h = c.now.hour.max(7);
    let start_h = end_h.saturating_sub(7);
    let vals = c.core.data.water.hourly(start_h, end_h);
    let maxv = vals.iter().copied().max().unwrap_or(0).max(400) as f32;
    let (gx, gy, gw, gh) = (cx + 16.0, y0 + 18.0, 196.0 - 32.0, 110.0);
    let n = vals.len().max(1) as f32;
    let bar_w = (gw - (n - 1.0) * 7.0) / n;
    for (i, v) in vals.iter().enumerate() {
        let bh = ((*v as f32 / maxv) * gh).max(6.0);
        let col = if i == vals.len() - 1 { SKY } else { Rgba(1.0, 1.0, 1.0, 0.22) };
        p.fill_rrect(gx + i as f32 * (bar_w + 7.0), gy + gh - bh, bar_w, bh, bar_w / 2.0, col);
    }
    for (i, hh) in [start_h, start_h + 3, start_h + 5, end_h].iter().enumerate() {
        let align = match i {
            0 => Align::Left,
            3 => Align::Right,
            _ => Align::Center,
        };
        let lx = match i {
            0 => gx,
            3 => gx + gw - 30.0,
            _ => gx + (*hh - start_h) as f32 * (bar_w + 7.0) - 8.0,
        };
        p.label(&format!("{:02}", hh), lx, gy + gh + 10.0, 30.0, 14.0, 11.0, align, TEXT_3);
    }
}

fn draw_todo(b: &mut B, c: &Ctx, x0: f32, y0: f32) -> Option<(f32, f32, f32, f32)> {
    let p = b.p;
    let inner = EXP_W - PAD_X * 2.0;
    let todos = c.core.visible_todos(c.now);
    let done = todos.iter().filter(|t| t.done).count();
    p.text(c.t("Hari ini", "Today"), x0, y0, 200.0, 28.0, 18.0, 500, Font::Sans, Align::Left, TEXT_1);
    p.label(&format!("{} / {} {}", done, todos.len(), c.t("selesai", "done")), x0 + inner - 200.0, y0 + 6.0, 200.0, 16.0, 12.0, Align::Right, TEXT_2);
    let mut y = y0 + 32.0;
    if todos.is_empty() {
        p.text(c.t("Belum ada tugas. Tambahkan satu di bawah.", "No tasks yet. Add one below."), x0 + 10.0, y, inner - 20.0, 44.0, 14.0, 400, Font::Sans, Align::Left, TEXT_2);
        y += 44.0;
    }
    for t in &todos {
        let hov = b.hit_alt(x0, y, inner, 44.0, Action::Cmd(Command::ToggleTodo(t.id)), Action::Cmd(Command::DeleteTodo(t.id)));
        if hov {
            p.fill_rrect(x0, y, inner, 44.0, 16.0, Rgba(1.0, 1.0, 1.0, 0.06));
        }
        let ccx = x0 + 20.0;
        let ccy = y + 22.0;
        if t.done {
            p.fill_circle(ccx, ccy, 10.0, ELECTRIC);
            p.icon(G_CHECK, ccx, ccy, 10.0, ICE);
        } else {
            p.stroke_circle(ccx, ccy, 9.25, TEXT_3, 1.5, false);
        }
        let (col, w) = if t.done { (TEXT_3, 400) } else { (TEXT_1, 400) };
        let tw = inner - 44.0 - 70.0;
        p.text(&t.text, x0 + 44.0, y, tw, 44.0, 15.0, w, Font::Sans, Align::Left, col);
        if t.done {
            let lw = p.gfx.measure(&t.text, 15.0, 400, Font::Sans).min(tw);
            p.line(x0 + 44.0, ccy + 1.0, x0 + 44.0 + lw, ccy + 1.0, TEXT_3, 1.2);
        }
        p.label(&t.time_label(), x0 + inner - 70.0, y + 14.0, 60.0, 16.0, 12.0, Align::Right, TEXT_2);
        y += 44.0;
    }
    y += 6.0;
    if c.core.adding_todo {
        p.fill_rrect(x0 - 4.0, y - 4.0, inner + 8.0, 52.0, 20.0, ELECTRIC.with_alpha(0.18));
        p.fill_rrect(x0, y, inner, 44.0, 16.0, EDIT_FILL);
        p.stroke_rrect(x0, y, inner, 44.0, 16.0, SKY.with_alpha(0.7), 1.0);
        p.stroke_circle(x0 + 20.0, y + 22.0, 9.25, TEXT_3, 1.5, true);
        p.label("Enter ↵ · Esc", x0 + inner - 130.0, y + 14.0, 118.0, 16.0, 11.0, Align::Right, TEXT_2);
        Some((x0 + 42.0, y + 8.0, inner - 42.0 - 136.0, 28.0))
    } else {
        let hov = b.hit(x0, y, inner, 44.0, Action::Cmd(Command::StartAddTodo));
        p.fill_rrect(x0, y, inner, 44.0, 16.0, if hov { Rgba(1.0, 1.0, 1.0, 0.08) } else { Rgba(1.0, 1.0, 1.0, 0.04) });
        p.stroke_rrect_dashed(x0, y, inner, 44.0, 16.0, Rgba(1.0, 1.0, 1.0, 0.26), 1.0);
        p.icon(G_ADD, x0 + 22.0, y + 22.0, 12.0, TEXT_1);
        p.text(c.t("Tambah tugas", "Add task"), x0 + 38.0, y, 200.0, 44.0, 14.0, 500, Font::Sans, Align::Left, TEXT_1);
        p.label("Win + Alt + N", x0 + inner - 150.0, y + 14.0, 136.0, 16.0, 11.0, Align::Right, TEXT_2);
        None
    }
}

// ------------------------------------------------------------------ sholat

fn draw_prayer(b: &mut B, c: &Ctx, x0: f32, y0: f32) {
    let p = b.p;
    let inner = EXP_W - PAD_X * 2.0;
    let Some(pt) = &c.core.prayer else { return };
    let lw = inner - 220.0 - 24.0;
    p.label(&format!("{} · {}", c.t("Sholat berikutnya", "Next prayer"), c.core.prayer_place()), x0, y0, lw, 16.0, 12.0, Align::Left, TEXT_2);
    let now_min = c.now.minute_of_day();
    let next = c.core.next_prayer(c.now);
    let (idx, sub) = match next {
        Some(i) => {
            let left = pt.min[i] - now_min;
            let s = match (left >= 60, c.core.lang.is_en()) {
                (true, false) => format!("dalam {} j {} mnt", left / 60, left % 60),
                (true, true) => format!("in {} h {} min", left / 60, left % 60),
                (false, false) => format!("dalam {} mnt", left.max(1)),
                (false, true) => format!("in {} min", left.max(1)),
            };
            (i, s)
        }
        None => (1, c.t("besok pagi", "tomorrow morning").to_string()),
    };
    p.text(prayer::name(idx, c.core.lang), x0, y0 + 20.0, lw, 26.0, 18.0, 500, Font::Sans, Align::Left, TEXT_1);
    p.text(&pt.label(idx), x0 - 2.0, y0 + 44.0, lw, 56.0, 48.0, 400, Font::Sans, Align::Left, TEXT_1);
    p.text(&sub, x0, y0 + 98.0, lw, 20.0, 13.0, 400, Font::Sans, Align::Left, TEXT_2);
    let on = c.core.data.settings.adzan_sound;
    let lbl = if on { c.t("Suara adzan aktif", "Adhan sound on") } else { c.t("Suara adzan mati", "Adhan sound off") };
    b.btn_glass(x0, y0 + 124.0, 176.0, 44.0, lbl, Some(if on { G_BELL } else { G_MUTE }), Action::Cmd(Command::SetAdzanSound(!on)));

    // daftar hari ini
    let cx = x0 + inner - 220.0;
    p.fill_rrect(cx, y0, 220.0, 168.0, 20.0, CARD_FILL);
    p.stroke_rrect(cx, y0, 220.0, 168.0, 20.0, CARD_BORDER, 1.0);
    for (i, name) in prayer::NAMES.iter().enumerate() {
        let ry = y0 + 7.0 + i as f32 * 22.0;
        let is_next = next == Some(i);
        let past = pt.min[i] <= now_min;
        if is_next {
            p.fill_rrect(cx + 8.0, ry, 204.0, 22.0, 11.0, SKY.with_alpha(0.18));
        }
        let col = if is_next { TEXT_1 } else if past { TEXT_3 } else { TEXT_2 };
        p.text(name, cx + 20.0, ry, 110.0, 22.0, 13.0, if is_next { 500 } else { 400 }, Font::Sans, Align::Left, col);
        p.label(&pt.label(i), cx + 220.0 - 20.0 - 70.0, ry + 3.0, 70.0, 16.0, 12.0, Align::Right, col);
    }
}

// ------------------------------------------------------------------ clipboard

fn draw_clipboard(b: &mut B, c: &Ctx, x0: f32, y0: f32) {
    let p = b.p;
    let inner = EXP_W - PAD_X * 2.0;
    let clips = c.core.visible_clips();
    p.text(c.t("Riwayat clipboard", "Clipboard history"), x0, y0, 260.0, 28.0, 18.0, 500, Font::Sans, Align::Left, TEXT_1);
    if c.core.clips.iter().any(|c| !c.pinned) {
        let bx = x0 + inner - 120.0;
        let hov = b.hit(bx, y0, 120.0, 28.0, Action::Cmd(Command::ClearClips));
        p.label(c.t("Hapus semua", "Clear all"), bx, y0 + 6.0, 120.0, 16.0, 11.0, Align::Right, if hov { TEXT_1 } else { TEXT_2 });
    }
    let mut y = y0 + 32.0;
    if clips.is_empty() {
        p.text(c.t("Belum ada teks yang disalin. Riwayat hanya disimpan di memori.", "Nothing copied yet. History is kept in memory only."), x0 + 10.0, y, inner - 20.0, 44.0, 14.0, 400, Font::Sans, Align::Left, TEXT_2);
        return;
    }
    for clip in clips {
        let hov = b.hit_alt(x0, y, inner, 44.0, Action::Cmd(Command::CopyClip(clip.id)), Action::Cmd(Command::DeleteClip(clip.id)));
        if hov {
            p.fill_rrect(x0, y, inner, 44.0, 16.0, Rgba(1.0, 1.0, 1.0, 0.06));
        }
        let copied = c.core.clip_copied.map_or(false, |(id, _)| id == clip.id);
        let (g, col) = if copied { (G_CHECK, MINT) } else { (G_PASTE, TEXT_3) };
        p.icon(g, x0 + 20.0, y + 22.0, 13.0, col);
        let one_line = clip.text.split_whitespace().collect::<Vec<_>>().join(" ");
        p.text(&one_line, x0 + 44.0, y, inner - 44.0 - 110.0, 44.0, 14.0, 400, Font::Sans, Align::Left, TEXT_1);
        if copied {
            p.label(c.t("Disalin", "Copied"), x0 + inner - 110.0, y + 14.0, 64.0, 16.0, 11.0, Align::Right, MINT);
        }
        // tombol sematkan (ditambahkan setelah baris agar menang saat dicari dari belakang)
        let px = x0 + inner - 40.0;
        let phov = b.hit(px, y + 6.0, 32.0, 32.0, Action::Cmd(Command::PinClip(clip.id)));
        if phov {
            p.fill_circle(px + 16.0, y + 22.0, 16.0, GLASS_FILL_HOVER);
        }
        p.icon(G_PIN, px + 16.0, y + 22.0, 13.0, if clip.pinned { SKY } else if hov || phov { TEXT_2 } else { TEXT_3.with_alpha(0.4) });
        y += 44.0;
    }
}

// ------------------------------------------------------------------ lirik

fn draw_lyrics(b: &mut B, c: &Ctx, x0: f32, y0: f32) {
    let p = b.p;
    let inner = EXP_W - PAD_X * 2.0;
    if let Some(m) = c.media {
        let head = if m.artist.is_empty() { m.title.clone() } else { format!("{} · {}", m.title, m.artist) };
        p.text(&head, x0, y0, inner - 120.0, 20.0, 12.0, 500, Font::Sans, Align::Left, TEXT_2);
    }
    let src = match &c.core.lyrics {
        LyricsState::Ready { source: LyricsSource::Online, .. } => "LRCLIB",
        LyricsState::Ready { source: LyricsSource::Local, .. } => c.t("File lokal", "Local file"),
        _ => "",
    };
    p.label(src, x0 + inner - 120.0, y0 + 2.0, 120.0, 16.0, 11.0, Align::Right, TEXT_3);
    let msg = |title: &str, sub: &str| {
        p.text(title, x0, y0 + 60.0, inner, 28.0, 18.0, 500, Font::Sans, Align::Center, TEXT_1);
        p.text(sub, x0, y0 + 92.0, inner, 20.0, 13.0, 400, Font::Sans, Align::Center, TEXT_2);
    };
    let lyrics = match &c.core.lyrics {
        LyricsState::NoMedia => {
            return msg(
                c.t("Tidak ada musik yang diputar", "No music playing"),
                c.t("Lirik muncul otomatis untuk lagu yang sedang diputar.", "Lyrics appear automatically for the song that's playing."),
            )
        }
        LyricsState::Loading => return msg(c.t("Mencari lirik…", "Searching for lyrics…"), ""),
        LyricsState::NotFound => {
            return msg(
                c.t("Lirik tidak ditemukan", "Lyrics not found"),
                c.t("Taruh file .lrc di folder lirik (Pengaturan → Lirik).", "Put an .lrc file in the lyrics folder (Settings → Lyrics)."),
            )
        }
        LyricsState::Offline => return msg(c.t("Lirik online nonaktif", "Online lyrics are off"), c.t("Aktifkan di Pengaturan → Lirik.", "Turn them on in Settings → Lyrics.")),
        LyricsState::Failed => {
            return msg(c.t("Tidak bisa terhubung ke LRCLIB", "Couldn't reach LRCLIB"), c.t("Dicoba lagi saat lagu berganti.", "Will retry when the song changes."))
        }
        LyricsState::Ready { lyrics, .. } if lyrics.instrumental || lyrics.lines.is_empty() => {
            return msg("Instrumental", c.t("Lagu ini tidak punya lirik.", "This song has no lyrics."))
        }
        LyricsState::Ready { lyrics, .. } => lyrics,
    };
    const ROWS: i64 = 5;
    const RH: f32 = 32.0;
    let top = y0 + 28.0;
    if !lyrics.synced {
        p.label(c.t("Tidak tersinkron", "Not synced"), x0, y0 + 2.0, 200.0, 16.0, 11.0, Align::Left, TEXT_3);
        for (k, line) in lyrics.lines.iter().take(ROWS as usize).enumerate() {
            p.text(&line.text, x0 + 12.0, top + k as f32 * RH, inner - 24.0, RH, 15.0, 400, Font::Sans, Align::Center, TEXT_2);
        }
        return;
    }
    // baris aktif di tengah, dua sebelum dan dua sesudah; klik baris untuk lompat
    let pos = c.media.map_or(0, |m| m.position_now(c.now.ms));
    let act = lyrics.active(pos).map_or(-1, |i| i as i64);
    for k in 0..ROWS {
        let i = act + k - ROWS / 2;
        if i < 0 || i >= lyrics.lines.len() as i64 {
            continue;
        }
        let line = &lyrics.lines[i as usize];
        let y = top + k as f32 * RH;
        let active = i == act;
        let hov = b.hit(x0, y, inner, RH, Action::Media(MediaCmd::Seek(line.ms)));
        if hov {
            p.fill_rrect(x0, y, inner, RH, 14.0, Rgba(1.0, 1.0, 1.0, 0.06));
        }
        let t = if line.text.is_empty() { "♪" } else { line.text.as_str() };
        let (size, weight, col) = if active { (20.0, 500, TEXT_1) } else { (15.0, 400, TEXT_3) };
        p.text(t, x0 + 12.0, y, inner - 24.0, RH, size, weight, Font::Sans, Align::Center, col);
    }
}

// ------------------------------------------------------------------ launcher

/// Ikon kotak 2x2 (tab Launcher, aplikasi tanpa ikon) digambar dengan rrect.
pub(crate) fn grid_icon(p: &Painter, cx: f32, cy: f32, size: f32, col: Rgba) {
    let cell = size * 0.42;
    let gap = size - cell * 2.0;
    for (i, j) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let x = cx - size / 2.0 + i * (cell + gap);
        let y = cy - size / 2.0 + j * (cell + gap);
        p.fill_rrect(x, y, cell, cell, cell * 0.3, col);
    }
}

/// Grid sematan 4 x 2. Mengembalikan kotak slot (untuk drag atur ulang).
fn draw_launcher(b: &mut B, c: &Ctx, x0: f32, y0: f32) -> Vec<(f32, f32, f32, f32)> {
    let p = b.p;
    let inner = EXP_W - PAD_X * 2.0;
    let bh = launcher_body_h(c);
    let mut pins: Vec<&Pin> = c.core.data.pins.iter().collect();
    if let Some((id, to)) = c.reorder {
        if let Some(i) = pins.iter().position(|p| p.id == id) {
            let moving = pins.remove(i);
            pins.insert(to.min(pins.len()), moving);
        }
    }
    let slots: Vec<(f32, f32, f32, f32)> = (0..pins.len())
        .map(|i| {
            let (col, row) = ((i % PIN_COLS) as f32, (i / PIN_COLS) as f32);
            (x0 + col * (PIN_W + PIN_GAP), y0 + row * (PIN_H + PIN_GAP), PIN_W, PIN_H)
        })
        .collect();

    if pins.is_empty() && !c.core.dragging {
        // keadaan kosong: zona putus-putus besar
        p.fill_rrect(x0, y0, inner, bh, PIN_R, Rgba(1.0, 1.0, 1.0, 0.04));
        p.stroke_rrect_dashed(x0, y0, inner, bh, PIN_R, Rgba(1.0, 1.0, 1.0, 0.26), 1.0);
        grid_icon(p, x0 + inner / 2.0, y0 + 44.0, 22.0, TEXT_2);
        p.text(c.t("Seret aplikasi, file, atau folder ke sini.", "Drag apps, files, or folders here."), x0, y0 + 64.0, inner, 28.0, 14.0, 400, Font::Sans, Align::Center, TEXT_2);
        return slots;
    }

    for (pin, &(x, y, w, h)) in pins.iter().zip(&slots) {
        let missing = c.missing.contains(&pin.id);
        let moving = c.reorder.map_or(false, |(id, _)| id == pin.id);
        if c.core.pin_confirm == Some(pin.id) {
            // "Lepas sematan?" untuk target yang hilang
            p.fill_rrect(x, y, w, h, PIN_R, CARD_FILL);
            p.stroke_rrect(x, y, w, h, PIN_R, AMBER.with_alpha(0.5), 1.0);
            p.text(c.t("Lepas sematan?", "Unpin?"), x, y + 14.0, w, 20.0, 12.0, 500, Font::Sans, Align::Center, TEXT_1);
            p.text(&pin.name, x + 10.0, y + 34.0, w - 20.0, 18.0, 11.0, 400, Font::Sans, Align::Center, TEXT_3);
            let bw = (w - 8.0 * 2.0 - 6.0) / 2.0;
            let by = y + h - 12.0 - 28.0;
            let hov = b.hit(x + 8.0, by, bw, 28.0, Action::Cmd(Command::RemovePin(pin.id)));
            p.fill_rrect(x + 8.0, by, bw, 28.0, 14.0, if hov { ICE.with_alpha(0.88) } else { ICE });
            p.text(c.t("Lepas", "Unpin"), x + 8.0, by, bw, 28.0, 12.0, 500, Font::Sans, Align::Center, NAVY);
            let bx = x + 8.0 + bw + 6.0;
            let hov = b.hit(bx, by, bw, 28.0, Action::Cmd(Command::AskRemovePin(None)));
            p.fill_rrect(bx, by, bw, 28.0, 14.0, if hov { GLASS_FILL_HOVER } else { GLASS_FILL });
            p.stroke_rrect(bx, by, bw, 28.0, 14.0, GLASS_BORDER, 1.0);
            p.text(c.t("Batal", "Cancel"), bx, by, bw, 28.0, 12.0, 500, Font::Sans, Align::Center, TEXT_1);
            continue;
        }
        let action = if missing { Action::Cmd(Command::AskRemovePin(Some(pin.id))) } else { Action::Cmd(Command::LaunchPin(pin.id)) };
        let hov = b.hover.as_ref() == Some(&action);
        b.hits.push(Hit { x, y, w, h, action, alt: Some(Action::PinMenu(pin.id)), drag: Some(pin.id) });
        p.fill_rrect(x, y, w, h, PIN_R, if hov || moving { GLASS_FILL_HOVER } else { CARD_FILL });
        p.stroke_rrect(x, y, w, h, PIN_R, if moving { SKY } else { CARD_BORDER }, if moving { 1.5 } else { 1.0 });
        let (ix, iy) = (x + (w - PIN_ICON) / 2.0, y + 14.0);
        let alpha = if missing { 0.35 } else { 1.0 };
        match c.icons.get(&pin.target) {
            Some(bmp) => p.bitmap(bmp, ix, iy, PIN_ICON, PIN_ICON, alpha),
            None => {
                let col = TEXT_2.with_alpha(0.72 * alpha);
                let (cx, cy) = (ix + PIN_ICON / 2.0, iy + PIN_ICON / 2.0);
                match pin.kind {
                    PinKind::Folder => p.icon(G_FOLDER, cx, cy, 30.0, col),
                    PinKind::File => p.icon(G_DOCUMENT, cx, cy, 30.0, col),
                    PinKind::App => grid_icon(p, cx, cy, 28.0, col),
                }
            }
        }
        if missing {
            p.text(&pin.name, x + 8.0, y + 64.0, w - 16.0, 18.0, 12.0, 400, Font::Sans, Align::Center, TEXT_3);
            p.label(c.t("Tidak ditemukan", "Not found"), x, y + 82.0, w, 14.0, 11.0, Align::Center, AMBER.with_alpha(0.8));
        } else {
            p.text(&pin.name, x + 8.0, y + 70.0, w - 16.0, 20.0, 12.0, 400, Font::Sans, Align::Center, TEXT_1);
        }
    }

    if c.core.dragging {
        // zona jatuh disorot di atas seluruh isi
        let full = pins.len() >= MAX_PINS;
        let col = if full { AMBER } else { SKY };
        p.fill_rrect(x0, y0, inner, bh, PIN_R, Rgba(0.04, 0.07, 0.18, 0.86));
        p.fill_rrect(x0, y0, inner, bh, PIN_R, col.with_alpha(0.12));
        p.stroke_rrect_dashed(x0, y0, inner, bh, PIN_R, col, 1.5);
        let msg = if full { c.t("Maksimal 8. Lepas satu sematan dulu.", "Up to 8. Unpin one first.") } else { c.t("Lepaskan untuk menyematkan", "Release to pin") };
        grid_icon(p, x0 + inner / 2.0, y0 + bh / 2.0 - 16.0, 20.0, col);
        p.text(msg, x0, y0 + bh / 2.0 + 2.0, inner, 28.0, 14.0, 500, Font::Sans, Align::Center, TEXT_1);
    }
    slots
}

fn draw_tabs(b: &mut B, c: &Ctx, active: Widget, h: f32) {
    let p = b.p;
    let tabs = c.core.tabs();
    let n = tabs.len() as f32;
    let w = n * 52.0 + (n - 1.0) * 4.0 + 8.0;
    let x = (WIN_W - w) / 2.0;
    let y = h - PAD_BOTTOM - TABS_H;
    p.fill_rrect(x, y, w, TABS_H, TABS_H / 2.0, Rgba(1.0, 1.0, 1.0, 0.05));
    p.stroke_rrect(x, y, w, TABS_H, TABS_H / 2.0, Rgba(1.0, 1.0, 1.0, 0.12), 1.0);
    for (i, wd) in tabs.iter().enumerate() {
        let tx = x + 4.0 + i as f32 * 56.0;
        let ty = y + 4.0;
        let is_active = *wd == active;
        let hov = b.hit(tx, ty, 52.0, 40.0, Action::Cmd(Command::Switch(*wd)));
        if is_active {
            p.fill_rrect(tx, ty, 52.0, 40.0, 20.0, GLASS_FILL_HOVER);
        } else if hov {
            p.fill_rrect(tx, ty, 52.0, 40.0, 20.0, Rgba(1.0, 1.0, 1.0, 0.07));
        }
        let col = if is_active { TEXT_1 } else { TEXT_3 };
        let (cx, cy) = (tx + 26.0, ty + 20.0);
        match wd {
            Widget::Music => p.icon(G_MUSIC, cx, cy, 16.0, col),
            Widget::Focus => p.icon(G_TIMER, cx, cy, 16.0, col),
            Widget::Water => p.droplet(cx, cy, 17.0, col, false),
            Widget::Todo => p.icon(G_TODO, cx, cy, 16.0, col),
            Widget::Prayer => p.icon(G_MOON, cx, cy, 16.0, col),
            Widget::Clipboard => p.icon(G_PASTE, cx, cy, 16.0, col),
            Widget::Launcher => grid_icon(p, cx, cy, 15.0, col),
            // tiga baris teks dari rrect (tanpa glyph yang belum terverifikasi)
            Widget::Lyrics => {
                for (i, lw) in [15.0, 10.0, 13.0].into_iter().enumerate() {
                    p.fill_rrect(cx - 7.5, cy - 6.0 + i as f32 * 5.5, lw, 2.2, 1.1, col);
                }
            }
        }
    }
}
