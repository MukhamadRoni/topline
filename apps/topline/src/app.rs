//! Perakitan aplikasi: jendela, message loop, timer, dan penghubung antara
//! event Windows <-> core <-> tampilan.

use std::cell::RefCell;
use std::mem::size_of;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::{IDropTarget, OleInitialize, RegisterDragDrop, RevokeDragDrop};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::System::WinRT::*;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::UI::Color;

use topline_core::*;
use topline_store::Store;

use crate::clip;
use crate::editbox::EditBox;
use crate::gfx::Gfx;
use crate::launcher::{self, DropEvent, Icons};
use crate::lyrics::{self, LyricsHandle};
use crate::settings::{self as setwin, SAction, Section, SettingsWindow};
use crate::traymenu::{self, TAction, TrayMenu};
use crate::media::{self, MediaHandle, MediaInfo};
use crate::notch::Notch;
use crate::sysmon::{self, SysMon};
use crate::theme::*;
use crate::tray::{self, MenuState, Tray};
use crate::util;
use crate::view::{self, Action, Hit};
use crate::watch::{self, Watchers};
use crate::*;

const CLASS_NOTCH: PCWSTR = w!("ToplineNotch");
const CLASS_HOST: PCWSTR = w!("ToplineTrayHost");

const T_CORE: usize = 1;
const T_TICK: usize = 2;
const T_HOVER_OPEN: usize = 3;
const T_HOVER_CLOSE: usize = 4;
const T_REGION: usize = 5;
/// Hanya untuk demo mata (TOPLINE_EYE_DEMO=1, mode debug).
const T_EYE_DEMO: usize = 6;
/// Pergantian baris lirik berikutnya (sekali jalan, hanya saat lirik tampil dan musik diputar).
const T_LYRIC: usize = 7;

const ABN_FULLSCREENAPP: u32 = 2;
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Bit Shift di LOWORD(wParam) WM_MOUSEWHEEL.
const MK_SHIFT: usize = 0x0004;

/// WM_COPYDATA dari `topline progress ...`: dwData = "TLPR", isi UTF-8 "status\nlabel".
const COPYDATA_PROGRESS: usize = 0x544C_5052;

const HK_TOGGLE: i32 = 1;
const HK_ADD: i32 = 2;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

struct App {
    hwnd: HWND,
    host: HWND,
    core: Core,
    store: Store,
    gfx: Gfx,
    notch: Notch,
    sys: SysMon,
    watchers: Watchers,
    tray: Tray,
    edit: EditBox,
    media: MediaHandle,
    media_info: Option<MediaInfo>,
    cover: Option<ID2D1Bitmap>,
    cover_key: String,
    hits: Vec<Hit>,
    /// Area gestur scroll dari frame terakhir (DIP).
    scroll_zones: Vec<(f32, f32, f32, f32, ScrollZone)>,
    hover: Option<Action>,
    tracking: bool,
    last_view: View,
    region: (f32, f32),
    edit_rect: Option<(i32, i32, i32, i32)>,
    taskbar_created: u32,
    origin: (i32, i32),
    /// Windows "Animation effects" aktif; jika mati, mata statis.
    eye_motion: bool,
    /// Indeks EyeMood::ALL yang sedang didemokan.
    eye_demo: Option<usize>,
    /// Drop target OLE yang terdaftar di jendela notch.
    drop_target: Option<IDropTarget>,
    icons: Icons,
    /// Sematan yang targetnya hilang (dicek saat launcher digambar).
    missing: Vec<u64>,
    /// Tile ditekan: (id, x, y) saat tombol kiri turun.
    press: Option<(u64, f32, f32)>,
    /// Drag atur ulang yang sedang berjalan: (id, indeks tujuan).
    reorder: Option<(u64, usize)>,
    /// Kotak slot tile dari frame terakhir.
    slots: Vec<(f32, f32, f32, f32)>,
    /// Sematan yang dibuka setelah notch selesai digambar ulang (menutup).
    pending_launch: Option<Pin>,
    lyrics_worker: LyricsHandle,
    /// Jendela Pengaturan (dibuat saat dibuka, dihancurkan saat ditutup).
    settings_win: Option<SettingsWindow>,
    /// Menu kaca klik kanan ikon tray (hanya selama terbuka).
    tray_menu: Option<TrayMenu>,
}

fn scale_of(hwnd: HWND) -> f32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / 96.0
    }
}

impl App {
    fn new(hwnd: HWND, host: HWND) -> Result<Self> {
        let now = util::now();
        let store = Store::new(util::data_dir());
        let core = Core::new(store.load(), &now);
        let gfx = Gfx::new()?;
        let scale = scale_of(hwnd);
        let notch = Notch::new(hwnd, &gfx, scale)?;
        let sys = SysMon::start(hwnd);
        let watchers = Watchers::start(hwnd);
        let icon = util::icon_from_ico(embedded::APP_ICON, unsafe { GetSystemMetrics(SM_CXSMICON) });
        let tray = Tray::new(host, icon);
        let edit = EditBox::new(hwnd)?;
        let media = media::start(hwnd);
        let lyrics_worker = lyrics::start(hwnd);
        let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };

        let mut app = Self {
            hwnd,
            host,
            core,
            store,
            gfx,
            notch,
            sys,
            watchers,
            tray,
            edit,
            media,
            media_info: None,
            cover: None,
            cover_key: String::new(),
            hits: Vec::new(),
            scroll_zones: Vec::new(),
            hover: None,
            tracking: false,
            last_view: View::Idle,
            region: (0.0, 0.0),
            edit_rect: None,
            taskbar_created,
            origin: (0, 0),
            eye_motion: util::animations_enabled(),
            eye_demo: util::eye_demo_enabled().then_some(0),
            drop_target: None,
            icons: Icons::default(),
            missing: Vec::new(),
            press: None,
            reorder: None,
            slots: Vec::new(),
            pending_launch: None,
            lyrics_worker,
            settings_win: None,
            tray_menu: None,
        };
        // Drag & drop dari Explorer/Start ke notch (UI thread STA, OleInitialize di run()).
        let dt: IDropTarget = launcher::DropTarget::new(hwnd).into();
        match unsafe { RegisterDragDrop(hwnd, &dt) } {
            Ok(()) => app.drop_target = Some(dt),
            Err(e) => util::log(&format!("RegisterDragDrop error: {e}")),
        }
        util::log(&format!(
            "start: scale={} backdrop={} geist={} view={:?}",
            scale, app.notch.has_backdrop, app.gfx.geist, app.core.view
        ));
        let _ = app.notch.set_solid(app.core.data.settings.solid);
        app.apply_language();
        // Status awal (tidak memunculkan peek).
        let (on_ac, pct) = SysMon::power_status();
        app.core.system(SystemEvent::Power { on_ac, percent: pct }, &now);
        app.core.system(SystemEvent::Network { connected: SysMon::network_connected() }, &now);
        let (camera, mic) = watch::privacy_scan();
        app.core.system(SystemEvent::Privacy { camera, mic }, &now);
        clip::listen(hwnd, app.core.data.settings.clipboard_history);
        app.place();
        unsafe {
            let _ = RegisterHotKey(hwnd, HK_TOGGLE, MOD_WIN | MOD_ALT | MOD_NOREPEAT, 'T' as u32);
            let _ = RegisterHotKey(hwnd, HK_ADD, MOD_WIN | MOD_ALT | MOD_NOREPEAT, 'N' as u32);
            if app.eye_demo.is_some() {
                SetTimer(hwnd, T_EYE_DEMO, 3000, None);
            }
        }
        app.refresh();
        Ok(app)
    }

    fn scale(&self) -> f32 {
        self.notch.scale
    }

    /// Letakkan jendela di tengah atas monitor utama.
    fn place(&mut self) {
        unsafe {
            let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let r = mi.rcMonitor;
            let s = self.scale();
            let w = (WIN_W * s).round() as i32;
            let h = (WIN_H * s).round() as i32;
            let x = r.left + ((r.right - r.left) - w) / 2;
            self.origin = (x, r.top);
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, x, r.top, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        self.region = (0.0, 0.0);
    }

    fn set_region(&mut self, w: f32, h: f32) {
        let s = self.scale();
        let ww = (WIN_W * s).round() as i32;
        let rw = ((w + 16.0) * s).ceil() as i32;
        let rh = ((h + 4.0) * s).ceil() as i32;
        let x0 = (ww - rw) / 2;
        unsafe {
            let rgn = CreateRectRgn(x0, 0, x0 + rw, rh);
            SetWindowRgn(self.hwnd, rgn, TRUE);
        }
        self.region = (w, h);
    }

    // ---------------------------------------------------------------- render

    fn refresh(&mut self) {
        let now = util::now();
        let playing = self.media_info.as_ref().map_or(false, |m| m.playing);
        let title = self.media_info.as_ref().map(|m| m.title.clone());
        self.core.system(SystemEvent::Media { playing, title }, &now);
        let view_changed = self.core.view != self.last_view;
        if view_changed {
            self.hover = None;
        }
        if self.core.widget() == Some(Widget::Launcher) {
            // Ikon diekstrak sekali per target; keberadaan target dicek hanya saat launcher digambar.
            self.icons.ensure(&self.gfx, &self.core.data.pins, self.notch.scale);
            self.missing = self.core.data.pins.iter().filter(|p| !launcher::exists(&p.target)).map(|p| p.id).collect();
        }
        let ctx = view::Ctx {
            core: &self.core,
            now: &now,
            media: self.media_info.as_ref(),
            cover: self.cover.as_ref(),
            hover: self.hover.as_ref(),
            icons: &self.icons,
            missing: &self.missing,
            reorder: self.reorder,
        };
        let mut frame = None;
        if let Err(e) = self.notch.draw(&self.gfx, |p| frame = Some(view::draw(p, &ctx))) {
            util::log(&format!("draw error: {e}"));
        }
        let Some(frame) = frame else { return };
        let (cw, ch, _) = self.notch.current();
        if let Err(e) = self.notch.set_size(frame.w, frame.h, frame.r, true) {
            util::log(&format!("set_size error: {e}"));
        }
        util::log(&format!(
            "refresh view={:?} frame={}x{} r={} prev={}x{} eyes={:?}",
            self.core.view, frame.w, frame.h, frame.r, cw, ch, frame.eyes
        ));
        if view_changed {
            let _ = self.notch.fade_in_content();
            // Aplikasi yang baru dibuka (mis. dari launcher) kadang menyusup di atas jendela
            // topmost; tegaskan lagi urutan-z setiap kali tampilan berubah.
            unsafe {
                let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
        let _ = self.notch.show_eq(frame.eq, Color { A: 255, R: 0x40, G: 0xA9, B: 0xFD });
        let eyes = match (self.eye_demo, frame.eyes) {
            (Some(i), Some(_)) => Some(EyeMood::ALL[i % EyeMood::ALL.len()]),
            _ => frame.eyes,
        };
        if let Err(e) = self.notch.set_eyes(eyes, self.eye_motion) {
            util::log(&format!("eyes error: {e}"));
        }
        // Raw Input mouse hanya selama mata tidur (input pertama = sapaan).
        self.sys.watch_mouse(self.core.eye_sleeping && frame.eyes.is_some());
        self.hits = frame.hits;
        self.scroll_zones = frame.scroll;
        self.slots = frame.slots;
        self.last_view = self.core.view;

        // Region: membesar segera, mengecil setelah animasi selesai.
        if frame.w >= cw && frame.h >= ch {
            self.set_region(frame.w, frame.h);
            unsafe {
                let _ = KillTimer(self.hwnd, T_REGION);
            }
        } else {
            self.set_region(frame.w.max(cw), frame.h.max(ch));
            unsafe {
                SetTimer(self.hwnd, T_REGION, ANIM_MS as u32 + 40, None);
            }
        }

        // Kolom input to-do
        match (frame.edit, self.core.adding_todo) {
            (Some((x, y, w, h)), true) => {
                let s = self.scale();
                let r = (
                    self.origin.0 + (x * s).round() as i32,
                    self.origin.1 + (y * s).round() as i32,
                    (w * s).round() as i32,
                    (h * s).round() as i32,
                );
                if self.edit_rect != Some(r) || !self.edit.visible {
                    self.edit.show(r.0, r.1, r.2, r.3, (15.0 * s).round() as i32);
                    self.edit_rect = Some(r);
                }
            }
            _ => {
                self.edit.hide();
                self.edit_rect = None;
            }
        }
        self.schedule(&now);
    }

    fn schedule(&mut self, now: &Clock) {
        // Satu GetLastInputInfo per penjadwalan; deadline tidur dihitung dari sini.
        self.core.set_last_input(util::last_input_ms(now));
        unsafe {
            match self.core.next_deadline_ms(now) {
                Some(d) => {
                    SetTimer(self.hwnd, T_CORE, (d - now.ms).clamp(50, 0x7fff_ffff) as u32, None);
                }
                None => {
                    let _ = KillTimer(self.hwnd, T_CORE);
                }
            }
            let playing = self.media_info.as_ref().map_or(false, |m| m.playing);
            let fast = match self.core.view {
                View::Expanded(Widget::Music) => playing,
                View::Expanded(Widget::Focus) => self.core.focus.running,
                _ => false,
            };
            let tick = if fast {
                Some(1000)
            } else {
                match self.core.view {
                    View::Idle if self.core.data.settings.personality => None,
                    View::Peek => None,
                    _ => Some(now.ms_to_next_minute() as u32 + 30),
                }
            };
            match tick {
                Some(ms) => {
                    SetTimer(self.hwnd, T_TICK, ms, None);
                }
                None => {
                    let _ = KillTimer(self.hwnd, T_TICK);
                }
            }
            // Lirik: satu deadline ke pergantian baris berikutnya, bukan ticker.
            let lyric = match (&self.core.lyrics, self.media_info.as_ref()) {
                (LyricsState::Ready { lyrics, .. }, Some(m)) if self.core.lyrics_visible() => {
                    let pos = m.position_now(now.ms);
                    lyrics.next_change(pos).map(|t| (t - pos).max(30) + 20)
                }
                _ => None,
            };
            match lyric {
                Some(ms) => {
                    SetTimer(self.hwnd, T_LYRIC, ms.min(0x7fff_ffff) as u32, None);
                }
                None => {
                    let _ = KillTimer(self.hwnd, T_LYRIC);
                }
            }
        }
    }

    // ---------------------------------------------------------------- actions

    fn command(&mut self, cmd: Command) {
        let now = util::now();
        let before = (self.core.data.settings.solid, self.core.data.settings.clipboard_history);
        let fx = self.core.apply(cmd, &now);
        self.effects(fx);
        self.refresh();
        self.sync_settings(before);
        // Buka target setelah notch mulai menutup (ShellExecuteEx bisa butuh waktu).
        if let Some(pin) = self.pending_launch.take() {
            self.launch(&pin, false);
        }
    }

    /// Selesaikan `Settings::language` (Auto = ikut Windows) dan terapkan ke semua teks
    /// di luar view: kolom to-do, judul jendela Pengaturan. Mengembalikan true jika berubah.
    fn apply_language(&mut self) -> bool {
        let lang = self.core.data.settings.language.resolve(util::windows_is_indonesian());
        let changed = lang != self.core.lang || self.edit.cue.is_empty();
        self.core.lang = lang;
        if changed {
            self.edit.set_cue(lang.t("Nama tugas… tambahkan jam, mis. 16:30", "Task name… add a time, e.g. 16:30"));
            if let Some(w) = &self.settings_win {
                w.set_title(lang);
            }
        }
        changed
    }

    /// Efek OS dari perubahan pengaturan (dari tray maupun jendela Pengaturan).
    fn sync_settings(&mut self, (solid, clip_on): (bool, bool)) {
        if self.apply_language() {
            self.refresh();
        }
        let s = &self.core.data.settings;
        let (now_solid, now_clip) = (s.solid, s.clipboard_history);
        if now_solid != solid {
            let _ = self.notch.set_solid(now_solid);
            if let Some(w) = self.settings_win.as_mut() {
                let _ = w.set_solid(now_solid);
            }
            if let Some(m) = &self.tray_menu {
                m.set_solid(now_solid);
            }
        }
        if now_clip != clip_on {
            clip::listen(self.hwnd, now_clip);
        }
        self.redraw_settings();
        self.redraw_tray_menu();
    }

    // ---------------------------------------------------------------- menu tray

    fn tray_view(&self) -> traymenu::View<'_> {
        traymenu::View {
            s: &self.core.data.settings,
            lang: self.core.lang,
            autostart: util::autostart_enabled(),
            prayer_place: self.core.prayer_place(),
        }
    }

    fn redraw_tray_menu(&mut self) {
        let Some(mut m) = self.tray_menu.take() else { return };
        m.draw(&self.gfx, &self.tray_view());
        self.tray_menu = Some(m);
    }

    fn tray_action(&mut self, a: TAction) {
        if !a.keeps_open() {
            if let Some(mut m) = self.tray_menu.take() {
                m.close();
            }
        }
        match a {
            TAction::Open => self.command(Command::Open(self.core.last_widget)),
            TAction::AddTodo => self.command(Command::StartAddTodo),
            TAction::Launcher => self.command(Command::Open(Widget::Launcher)),
            TAction::Settings => self.open_settings(),
            TAction::Prayer => self.open_settings_at(Some(Section::Prayer)),
            TAction::Toggle(t) => self.command(Command::SetToggle(t, !t.get(&self.core.data.settings))),
            TAction::Autostart => {
                util::set_autostart(!util::autostart_enabled());
                self.redraw_settings();
                self.redraw_tray_menu();
            }
            TAction::Language(l) => self.command(Command::SetLanguage(l)),
            TAction::DataFolder => util::open_folder(&util::data_dir()),
            TAction::Exit => unsafe {
                let _ = PostMessageW(self.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            },
        }
    }

    fn on_tray_menu_message(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let m = self.tray_menu.as_mut()?;
        match m.on_message(msg, wp, lp) {
            traymenu::Reply::Default => None,
            traymenu::Reply::Done(r) => Some(r),
            traymenu::Reply::Redraw => {
                self.redraw_tray_menu();
                Some(LRESULT(0))
            }
            traymenu::Reply::Act(a) => {
                self.tray_action(a);
                Some(LRESULT(0))
            }
            traymenu::Reply::Dismiss => {
                if let Some(mut m) = self.tray_menu.take() {
                    m.close();
                }
                Some(LRESULT(0))
            }
        }
    }

    fn request_lyrics(&self) {
        if let Some(m) = &self.media_info {
            self.lyrics_worker.request(lyrics::Query {
                key: track_key(m),
                title: m.title.clone(),
                artist: m.artist.clone(),
                duration_ms: m.duration_ms,
                online: self.core.data.settings.lyrics_online,
            });
        }
    }

    // ---------------------------------------------------------------- pengaturan

    fn open_settings(&mut self) {
        self.open_settings_at(None);
    }

    fn open_settings_at(&mut self, section: Option<Section>) {
        if self.core.is_open() {
            self.command(Command::Close);
        }
        if let Some(w) = self.settings_win.as_mut() {
            if let Some(s) = section {
                w.nav(s);
                self.redraw_settings();
            }
            if let Some(w) = &self.settings_win {
                w.show();
            }
            return;
        }
        match SettingsWindow::open(self.notch.compositor(), &self.gfx, self.core.data.settings.solid, section.unwrap_or(Section::General)) {
            Ok(w) => {
                w.set_title(self.core.lang);
                self.settings_win = Some(w);
                self.redraw_settings();
                if let Some(w) = &self.settings_win {
                    w.show();
                }
            }
            Err(e) => util::log(&format!("settings window error: {e}")),
        }
    }

    fn redraw_settings(&mut self) {
        let Some(w) = self.settings_win.as_mut() else { return };
        let v = setwin::View {
            s: &self.core.data.settings,
            lang: self.core.lang,
            autostart: util::autostart_enabled(),
            prayer_place: self.core.prayer_place(),
            lyrics_cache: if w.section == Section::Lyrics { lyrics::cache_count() } else { 0 },
        };
        w.draw(&self.gfx, &v);
    }

    fn settings_action(&mut self, a: SAction) {
        match a {
            SAction::Close => {
                if let Some(mut w) = self.settings_win.take() {
                    w.close();
                }
                return;
            }
            SAction::Nav(s) => {
                if let Some(w) = self.settings_win.as_mut() {
                    w.nav(s);
                }
            }
            // Perubahan core lewat command: menyimpan, menggambar ulang notch dan jendela ini.
            SAction::Toggle(t) => return self.command(Command::SetToggle(t, !t.get(&self.core.data.settings))),
            SAction::Step(n, d) => return self.command(Command::StepNumber(n, d)),
            SAction::Tab(w) => {
                let hidden = self.core.data.settings.hidden_tabs.iter().any(|k| k == w.key());
                return self.command(Command::SetTabVisible(w, hidden));
            }
            SAction::Language(l) => return self.command(Command::SetLanguage(l)),
            SAction::Autostart => util::set_autostart(!util::autostart_enabled()),
            SAction::ChooseCity => {
                let st = self.menu_state();
                let owner = self.settings_win.as_ref().map(|w| w.hwnd).unwrap_or(self.host);
                let id = tray::prayer_menu(owner, &st);
                return self.menu_command(id, &st);
            }
            SAction::Locate => watch::locate(self.hwnd),
            SAction::LyricsFolder => util::open_folder(&lyrics::dir()),
            SAction::ClearLyrics => lyrics::clear_cache(),
            SAction::DataFolder => util::open_folder(&util::data_dir()),
        }
        self.redraw_settings();
    }

    fn on_settings_message(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let w = self.settings_win.as_mut()?;
        match w.on_message(msg, wp, lp) {
            setwin::Reply::Default => None,
            setwin::Reply::Done(r) => Some(r),
            setwin::Reply::Redraw => {
                self.redraw_settings();
                Some(LRESULT(0))
            }
            setwin::Reply::Act(a) => {
                self.settings_action(a);
                Some(LRESULT(0))
            }
        }
    }

    fn launch(&mut self, pin: &Pin, admin: bool) {
        if !launcher::launch(self.hwnd, &pin.target, admin) {
            util::log(&format!("launch gagal: {}", pin.target));
            self.system(SystemEvent::LaunchFailed { name: pin.name.clone() });
        }
    }

    fn pin_menu(&mut self, id: u64) {
        let Some(pin) = self.core.data.pins.iter().find(|p| p.id == id).cloned() else { return };
        let choice = launcher::pin_menu(self.host, &pin, self.core.lang);
        util::log(&format!("pin menu {} -> {}", pin.id, choice));
        match choice {
            launcher::MENU_REVEAL => {
                self.command(Command::Close);
                launcher::reveal(&pin.target);
            }
            launcher::MENU_ADMIN => {
                self.command(Command::Close);
                self.launch(&pin, true);
            }
            launcher::MENU_UNPIN => self.command(Command::RemovePin(id)),
            _ => {}
        }
    }

    fn on_drop(&mut self, ev: DropEvent) {
        match ev {
            DropEvent::Enter => self.command(Command::DragEnter),
            DropEvent::Leave => self.command(Command::DragLeave),
            DropEvent::Drop(items) => {
                util::log(&format!("drop: {:?}", items));
                self.command(Command::AddPins(items));
                // Selama drag OLE tidak ada WM_MOUSEMOVE; mulai lacak keluarnya kursor dari sini.
                self.tracking = false;
                self.track_mouse();
            }
        }
    }

    fn track_mouse(&mut self) {
        if !self.tracking {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
            self.tracking = true;
        }
    }

    /// Slot tile terdekat dari titik (DIP), untuk drag atur ulang.
    fn slot_at(&self, x: f32, y: f32) -> usize {
        let d = |s: &(f32, f32, f32, f32)| (s.0 + s.2 / 2.0 - x).powi(2) + (s.1 + s.3 / 2.0 - y).powi(2);
        self.slots
            .iter()
            .enumerate()
            .min_by(|a, b| d(a.1).total_cmp(&d(b.1)))
            .map_or(0, |(i, _)| i)
    }

    fn effects(&mut self, fx: Effects) {
        if fx.save {
            let _ = self.store.save(&self.core.data);
        }
        match fx.sound {
            Some(Sound::Chime) => sysmon::play_sound(true),
            Some(Sound::Soft) => sysmon::play_sound(false),
            Some(Sound::Adzan) => sysmon::play_adzan(),
            None => {}
        }
        if let Some(text) = fx.clipboard {
            clip::set_text(self.hwnd, &text);
        }
        if let Some(pin) = fx.launch {
            self.pending_launch = Some(pin);
        }
        if fx.fetch_lyrics {
            self.request_lyrics();
        }
        if let Some(name) = fx.reveal {
            if let Some(dir) = watch::downloads_dir() {
                let args = util::wide(&format!("/select,\"{}\"", dir.join(name).display()));
                unsafe {
                    ShellExecuteW(None, w!("open"), w!("explorer.exe"), PCWSTR(args.as_ptr()), None, SW_SHOWNORMAL);
                }
            }
        }
    }

    fn perform(&mut self, a: Action) {
        match a {
            Action::Cmd(c) => self.command(c),
            Action::OpenSettings => self.open_settings(),
            Action::Media(m) => {
                self.media.command(m);
                // baris lirik yang diklik langsung disorot
                if let media::MediaCmd::Seek(ms) = m {
                    if let Some(mi) = self.media_info.as_mut() {
                        mi.position_ms = ms;
                        mi.updated_ms = util::now().ms;
                    }
                }
                // tampilkan perubahan play/pause seketika
                if m == media::MediaCmd::PlayPause {
                    if let Some(mi) = self.media_info.as_mut() {
                        let now = util::now();
                        mi.position_ms = mi.position_now(now.ms);
                        mi.updated_ms = now.ms;
                        mi.playing = !mi.playing;
                    }
                }
                self.refresh();
            }
            Action::PinMenu(id) => self.pin_menu(id),
            Action::PeekWater => {
                let now = util::now();
                let fx = self.core.apply(Command::AddWater(250), &now);
                self.effects(fx);
                self.command(Command::DismissPeek);
            }
        }
    }

    /// Jalankan hasil gestur scroll / klik tengah.
    fn gesture(&mut self, a: GestureAction) {
        match a {
            // peek volume datang sendiri dari IAudioEndpointVolumeCallback
            GestureAction::Volume(d) => self.sys.set_volume_step(d),
            GestureAction::ToggleMute => self.sys.toggle_mute(),
            GestureAction::Track { next } => {
                self.media.command(if next { media::MediaCmd::Next } else { media::MediaCmd::Prev });
                self.refresh();
            }
            GestureAction::PlayPause => self.perform(Action::Media(media::MediaCmd::PlayPause)),
            GestureAction::Tab => self.refresh(),
        }
    }

    fn system(&mut self, ev: SystemEvent) {
        let now = util::now();
        let fx = self.core.system(ev, &now);
        self.effects(fx);
        self.refresh();
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<&Hit> {
        self.hits.iter().rev().find(|h| h.contains(x, y))
    }

    fn on_media(&mut self) {
        self.media_info = self.media.snapshot();
        util::log(&format!(
            "media: {:?}",
            self.media_info.as_ref().map(|m| (m.title.as_str(), m.playing))
        ));
        let key = self.media_info.as_ref().map(|m| m.thumb_key.clone()).unwrap_or_default();
        if key != self.cover_key {
            self.cover_key = key;
            self.cover = self
                .media_info
                .as_ref()
                .and_then(|m| m.thumb.clone())
                .and_then(|b| self.gfx.decode_image(&b).ok());
        }
        // Lagu baru → cari liriknya (di thread lirik).
        let key = self.media_info.as_ref().map(track_key);
        let fx = self.core.system(SystemEvent::Track { key }, &util::now());
        self.effects(fx);
        self.refresh();
    }

    /// Klik kanan ikon tray: menu kaca; menu Win32 standar hanya jika popup gagal dibuat.
    fn menu(&mut self) {
        if let Some(mut m) = self.tray_menu.take() {
            m.close();
            return;
        }
        let mut at = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut at);
        }
        let opened = TrayMenu::open(self.notch.compositor(), &self.gfx, self.core.data.settings.solid, at, &self.tray_view());
        match opened {
            Ok(m) => self.tray_menu = Some(m),
            Err(e) => {
                util::log(&format!("tray menu error: {e}"));
                let st = self.menu_state();
                let id = self.tray.show_menu(&st);
                self.menu_command(id, &st);
            }
        }
    }

    fn menu_state(&self) -> MenuState {
        MenuState {
            hover: self.core.data.settings.open_on_hover,
            personality: self.core.data.settings.personality,
            autostart: util::autostart_enabled(),
            solid: self.core.data.settings.solid,
            prayer_city: self.core.data.settings.prayer_city.clone(),
            prayer_located: self.core.data.settings.prayer_location.is_some(),
            prayer_place: self.core.prayer_place(),
            adzan: self.core.data.settings.adzan_sound,
            clipboard: self.core.data.settings.clipboard_history,
            scroll: self.core.data.settings.scroll_gestures,
            lang: self.core.lang,
            language: self.core.data.settings.language,
        }
    }

    fn menu_command(&mut self, id: usize, st: &MenuState) {
        match id {
            tray::ID_OPEN => self.command(Command::Open(self.core.last_widget)),
            tray::ID_SETTINGS => self.open_settings(),
            tray::ID_LANG_AUTO => self.command(Command::SetLanguage(Language::Auto)),
            tray::ID_LANG_ID => self.command(Command::SetLanguage(Language::Id)),
            tray::ID_LANG_EN => self.command(Command::SetLanguage(Language::En)),
            tray::ID_ADD_TODO => self.command(Command::StartAddTodo),
            tray::ID_LAUNCHER => self.command(Command::Open(Widget::Launcher)),
            tray::ID_HOVER => self.command(Command::SetOpenOnHover(!st.hover)),
            tray::ID_PERSONALITY => self.command(Command::SetPersonality(!st.personality)),
            tray::ID_SCROLL => self.command(Command::SetScrollGestures(!st.scroll)),
            tray::ID_SOLID => self.command(Command::SetToggle(Toggle::Solid, !st.solid)),
            tray::ID_AUTOSTART => {
                util::set_autostart(!st.autostart);
                self.redraw_settings();
            }
            tray::ID_ADZAN => self.command(Command::SetAdzanSound(!st.adzan)),
            tray::ID_CLIPBOARD => self.command(Command::SetClipboardHistory(!st.clipboard)),
            tray::ID_PRAYER_OFF => self.command(Command::SetPrayerCity(String::new())),
            tray::ID_PRAYER_LOCATE => watch::locate(self.hwnd),
            id if (tray::ID_CITY..tray::ID_CITY + prayer::CITIES.len()).contains(&id) => {
                self.command(Command::SetPrayerCity(prayer::CITIES[id - tray::ID_CITY].name.to_string()));
            }
            tray::ID_DATA => util::open_folder(&util::data_dir()),
            tray::ID_EXIT => unsafe {
                let _ = PostMessageW(self.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            _ => {}
        }
    }

    fn shutdown(&mut self) {
        if let Some(mut w) = self.settings_win.take() {
            w.close();
        }
        if let Some(mut m) = self.tray_menu.take() {
            m.close();
        }
        let _ = self.store.save(&self.core.data);
        self.tray.remove();
        self.sys.stop();
        self.watchers.stop();
        clip::listen(self.hwnd, false);
        self.edit.hide();
        unsafe {
            if self.drop_target.take().is_some() {
                let _ = RevokeDragDrop(self.hwnd);
            }
            let _ = UnregisterHotKey(self.hwnd, HK_TOGGLE);
            let _ = UnregisterHotKey(self.hwnd, HK_ADD);
        }
    }

    // ---------------------------------------------------------------- messages

    fn on_message(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if hwnd == self.host {
            return self.on_host_message(msg, wp, lp);
        }
        if self.settings_win.as_ref().map_or(false, |w| w.hwnd == hwnd) {
            return self.on_settings_message(msg, wp, lp);
        }
        if self.tray_menu.as_ref().map_or(false, |m| m.hwnd == hwnd) {
            return self.on_tray_menu_message(msg, wp, lp);
        }
        let s = self.scale();
        let mouse = || {
            let x = (lp.0 & 0xffff) as i16 as f32 / s;
            let y = ((lp.0 >> 16) & 0xffff) as i16 as f32 / s;
            (x, y)
        };
        match msg {
            WM_MOUSEMOVE => {
                // Drag atur ulang tile launcher (mouse capture biasa, tanpa OLE).
                if let Some((id, px, py)) = self.press {
                    let (x, y) = mouse();
                    if self.reorder.is_some() || (x - px).hypot(y - py) > 6.0 {
                        let to = self.slot_at(x, y);
                        if self.reorder != Some((id, to)) {
                            self.reorder = Some((id, to));
                            self.refresh();
                        }
                        return Some(LRESULT(0));
                    }
                }
                self.track_mouse();
                unsafe {
                    let _ = KillTimer(self.hwnd, T_HOVER_CLOSE);
                }
                // Kursor yang baru dipakai scroll tidak membuka notch (1,5 s sejak scroll terakhir).
                if self.core.view == View::Idle && self.core.data.settings.open_on_hover && !self.core.gestures.hover_blocked(util::now().ms) {
                    unsafe {
                        SetTimer(self.hwnd, T_HOVER_OPEN, 140, None);
                    }
                }
                let (x, y) = mouse();
                if self.core.eye_sleeping {
                    self.system(SystemEvent::UserActivity);
                }
                if self.core.view == View::Idle && self.core.data.settings.personality {
                    // Lirik ke arah kursor: maks ±2 DIP horizontal, ±1 DIP vertikal.
                    let q = |v: f32| (v * 4.0).round() / 4.0;
                    let dx = q(((x - WIN_W / 2.0) / (IDLE_W_SMALL / 2.0) * 2.0).clamp(-2.0, 2.0));
                    let dy = q(((y - IDLE_H / 2.0) / (IDLE_H / 2.0)).clamp(-1.0, 1.0));
                    let _ = self.notch.eyes_look(dx, dy);
                }
                let h = self.hit_at(x, y).map(|h| h.action.clone());
                if h != self.hover {
                    self.hover = h;
                    unsafe {
                        let _ = SetCursor(LoadCursorW(None, if self.hover.is_some() { IDC_HAND } else { IDC_ARROW }).unwrap_or_default());
                    }
                    self.refresh();
                }
                Some(LRESULT(0))
            }
            WM_SETCURSOR => {
                unsafe {
                    let _ = SetCursor(LoadCursorW(None, if self.hover.is_some() { IDC_HAND } else { IDC_ARROW }).unwrap_or_default());
                }
                Some(LRESULT(1))
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                let _ = self.notch.eyes_look(0.0, 0.0);
                unsafe {
                    let _ = KillTimer(self.hwnd, T_HOVER_OPEN);
                }
                if self.hover.take().is_some() {
                    self.refresh();
                }
                if self.core.is_open() && self.core.data.settings.open_on_hover && !self.core.adding_todo {
                    unsafe {
                        SetTimer(self.hwnd, T_HOVER_CLOSE, 450, None);
                    }
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN => {
                let (x, y) = mouse();
                if let Some(id) = self.hit_at(x, y).and_then(|h| h.drag) {
                    self.press = Some((id, x, y));
                    unsafe {
                        SetCapture(self.hwnd);
                    }
                }
                Some(LRESULT(0))
            }
            WM_CAPTURECHANGED => {
                self.press = None;
                if self.reorder.take().is_some() {
                    self.refresh();
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                if self.press.take().is_some() {
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    if let Some((id, to)) = self.reorder.take() {
                        self.command(Command::MovePin { id, to });
                        return Some(LRESULT(0));
                    }
                }
                let (x, y) = mouse();
                unsafe {
                    let _ = KillTimer(self.hwnd, T_HOVER_OPEN);
                }
                if let Some(a) = self.hit_at(x, y).map(|h| h.action.clone()) {
                    self.perform(a);
                } else {
                    match self.core.view {
                        View::Idle => self.command(Command::Open(self.core.last_widget)),
                        View::Peek => self.command(Command::ActivatePeek),
                        View::Expanded(_) => {}
                    }
                }
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                unsafe {
                    let _ = KillTimer(self.hwnd, T_HOVER_OPEN);
                }
                let delta = ((wp.0 >> 16) & 0xffff) as i16 as i32;
                let shift = wp.0 & MK_SHIFT != 0;
                // Koordinat roda adalah koordinat layar.
                let mut pt = POINT { x: (lp.0 & 0xffff) as i16 as i32, y: ((lp.0 >> 16) & 0xffff) as i16 as i32 };
                unsafe {
                    let _ = ScreenToClient(self.hwnd, &mut pt);
                }
                let (x, y) = (pt.x as f32 / s, pt.y as f32 / s);
                let zone = self.scroll_zones.iter().rev().find(|z| x >= z.0 && x < z.0 + z.2 && y >= z.1 && y < z.1 + z.3).map(|z| z.4);
                if let Some(zone) = zone {
                    let now = util::now();
                    let a = self.core.scroll(zone, delta, msg == WM_MOUSEHWHEEL, shift, &now);
                    util::log(&format!("wheel h={} delta={} shift={} {:?} -> {:?}", msg == WM_MOUSEHWHEEL, delta, shift, zone, a));
                    if let Some(a) = a {
                        self.gesture(a);
                    }
                }
                Some(LRESULT(0))
            }
            WM_MBUTTONUP => {
                if matches!(self.core.view, View::Idle | View::Peek) {
                    if let Some(a) = self.core.middle_click() {
                        self.gesture(a);
                    }
                }
                Some(LRESULT(0))
            }
            WM_RBUTTONUP => {
                let (x, y) = mouse();
                if let Some(a) = self.hit_at(x, y).and_then(|h| h.alt.clone()) {
                    self.perform(a);
                }
                Some(LRESULT(0))
            }
            WM_TIMER => {
                let id = wp.0;
                match id {
                    T_CORE => {
                        let now = util::now();
                        self.core.set_last_input(util::last_input_ms(&now));
                        let fx = self.core.tick(&now);
                        self.effects(fx);
                        self.refresh();
                    }
                    T_TICK | T_LYRIC => self.refresh(),
                    T_HOVER_OPEN => {
                        unsafe {
                            let _ = KillTimer(self.hwnd, T_HOVER_OPEN);
                        }
                        if self.core.view == View::Idle && !self.core.gestures.hover_blocked(util::now().ms) {
                            self.command(Command::Open(self.core.last_widget));
                        }
                    }
                    T_HOVER_CLOSE => {
                        unsafe {
                            let _ = KillTimer(self.hwnd, T_HOVER_CLOSE);
                        }
                        if self.core.is_open() && !self.core.adding_todo && !self.edit.has_focus() {
                            self.command(Command::Close);
                        }
                    }
                    T_REGION => {
                        unsafe {
                            let _ = KillTimer(self.hwnd, T_REGION);
                        }
                        let (w, h, _) = self.notch.current();
                        self.set_region(w, h);
                    }
                    T_EYE_DEMO => {
                        self.eye_demo = self.eye_demo.map(|i| (i + 1) % EyeMood::ALL.len());
                        util::log(&format!("eye demo: {:?}", self.eye_demo.map(|i| EyeMood::ALL[i])));
                        self.refresh();
                    }
                    _ => {}
                }
                Some(LRESULT(0))
            }
            WM_HOTKEY => {
                match wp.0 as i32 {
                    HK_TOGGLE => self.command(Command::Toggle),
                    HK_ADD => self.command(Command::StartAddTodo),
                    _ => {}
                }
                Some(LRESULT(0))
            }
            WM_APP_TOGGLE => {
                self.command(Command::Toggle);
                Some(LRESULT(0))
            }
            WM_APP_SETTINGS => {
                self.open_settings_at(Section::ALL.get(wp.0).copied());
                Some(LRESULT(0))
            }
            WM_APP_MEDIA => {
                self.on_media();
                Some(LRESULT(0))
            }
            WM_APP_VOLUME => {
                self.system(SystemEvent::Volume { percent: wp.0 as u32, muted: lp.0 != 0 });
                Some(LRESULT(0))
            }
            WM_APP_NET => {
                self.system(SystemEvent::Network { connected: SysMon::network_connected() });
                Some(LRESULT(0))
            }
            WM_POWERBROADCAST => {
                let (on_ac, percent) = SysMon::power_status();
                self.system(SystemEvent::Power { on_ac, percent });
                Some(LRESULT(1))
            }
            WM_INPUT => {
                let ev = self.sys.on_raw_input(lp);
                if ev.activity && self.core.eye_sleeping {
                    self.system(SystemEvent::UserActivity);
                }
                if let Some(on) = ev.caps {
                    self.system(SystemEvent::CapsLock { on });
                }
                None
            }
            WM_CLIPBOARDUPDATE => {
                if self.core.data.settings.clipboard_history {
                    if let Some(text) = clip::read_text(self.hwnd) {
                        self.system(SystemEvent::Clipboard { text });
                    }
                }
                Some(LRESULT(0))
            }
            WM_APP_PRIVACY => {
                let (camera, mic) = watch::privacy_scan();
                if camera != self.core.camera || mic != self.core.mic {
                    self.system(SystemEvent::Privacy { camera, mic });
                }
                Some(LRESULT(0))
            }
            WM_APP_DOWNLOAD => {
                // Dikirim watch.rs sebagai Box<String>; diambil alih di sini.
                let name = unsafe { *Box::from_raw(lp.0 as *mut String) };
                self.system(SystemEvent::DownloadDone { name });
                Some(LRESULT(0))
            }
            WM_APP_LYRICS => {
                // Box<lyrics::Reply> dari thread lirik.
                let r = unsafe { *Box::from_raw(lp.0 as *mut lyrics::Reply) };
                self.system(SystemEvent::Lyrics { key: r.key, result: r.result });
                Some(LRESULT(0))
            }
            WM_APP_DROP => {
                // Box<DropEvent> dari launcher::DropTarget saat App sedang dipinjam.
                let ev = unsafe { *Box::from_raw(lp.0 as *mut DropEvent) };
                self.on_drop(ev);
                Some(LRESULT(0))
            }
            WM_APP_LOCATION => {
                // Box<GeoPoint> dari watch::locate, atau 0 jika gagal.
                let point = (lp.0 != 0).then(|| unsafe { *Box::from_raw(lp.0 as *mut GeoPoint) });
                util::log(&format!("location: {:?}", point));
                self.system(SystemEvent::Location(point));
                Some(LRESULT(0))
            }
            WM_COPYDATA => {
                let cds = unsafe { &*(lp.0 as *const COPYDATASTRUCT) };
                if cds.dwData != COPYDATA_PROGRESS || cds.lpData.is_null() {
                    return Some(LRESULT(0));
                }
                let bytes = unsafe { std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize) };
                let msg = String::from_utf8_lossy(bytes);
                let (state, label) = msg.split_once('\n').unwrap_or((&msg, ""));
                if let Some(state) = ProgressState::parse(state) {
                    let label = if label.trim().is_empty() { self.core.lang.t("Proses", "Process").to_string() } else { label.trim().to_string() };
                    self.system(SystemEvent::Progress { label, state });
                }
                Some(LRESULT(1))
            }
            WM_SETTINGCHANGE => {
                // Bahasa tampilan Windows bisa berubah (berlaku untuk Language::Auto).
                if self.apply_language() {
                    self.refresh();
                    self.redraw_settings();
                }
                // "Animation effects" bisa diubah kapan saja dari Settings Windows.
                let motion = util::animations_enabled();
                if motion != self.eye_motion {
                    self.eye_motion = motion;
                    self.refresh();
                }
                None
            }
            WM_APP_APPBAR => {
                if wp.0 as u32 == ABN_FULLSCREENAPP {
                    unsafe {
                        let _ = ShowWindow(self.hwnd, if lp.0 != 0 { SW_HIDE } else { SW_SHOWNOACTIVATE });
                    }
                    if lp.0 != 0 {
                        self.edit.hide();
                    }
                }
                Some(LRESULT(0))
            }
            WM_APP_EDIT_SUBMIT => {
                let text = self.edit.text();
                if text.trim().is_empty() {
                    self.command(Command::CancelAddTodo);
                } else {
                    self.edit.clear();
                    self.command(Command::SubmitTodo(text));
                }
                Some(LRESULT(0))
            }
            WM_APP_EDIT_CANCEL => {
                self.edit.clear();
                self.command(Command::CancelAddTodo);
                Some(LRESULT(0))
            }
            WM_APP_EDIT_BLUR => {
                if self.core.adding_todo && self.edit.visible {
                    let text = self.edit.text();
                    self.edit.clear();
                    if !text.trim().is_empty() {
                        let now = util::now();
                        let fx = self.core.apply(Command::SubmitTodo(text), &now);
                        self.effects(fx);
                    }
                    self.command(Command::CancelAddTodo);
                }
                Some(LRESULT(0))
            }
            WM_DPICHANGED => {
                let dpi = (wp.0 >> 16) & 0xffff;
                let _ = self.notch.set_scale(dpi as f32 / 96.0);
                self.place();
                self.edit_rect = None;
                self.refresh();
                Some(LRESULT(0))
            }
            WM_DISPLAYCHANGE => {
                self.place();
                self.edit_rect = None;
                self.refresh();
                Some(LRESULT(0))
            }
            WM_CLOSE => {
                self.shutdown();
                unsafe {
                    let _ = DestroyWindow(self.host);
                    let _ = DestroyWindow(self.hwnd);
                    PostQuitMessage(0);
                }
                Some(LRESULT(0))
            }
            _ => None,
        }
    }

    fn on_host_message(&mut self, msg: u32, _wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if msg == WM_APP_TRAY {
            match (lp.0 & 0xffff) as u32 {
                WM_LBUTTONUP => self.command(Command::Toggle),
                WM_RBUTTONUP | WM_CONTEXTMENU => self.menu(),
                _ => {}
            }
            return Some(LRESULT(0));
        }
        if msg == self.taskbar_created && msg != 0 {
            self.tray.add();
            return Some(LRESULT(0));
        }
        None
    }
}

/// Kunci lagu untuk lirik: judul + artis.
fn track_key(m: &MediaInfo) -> String {
    format!("{}\u{1}{}", m.title, m.artist)
}

/// Jendela Pengaturan memakai bingkai sendiri: area klien = seluruh jendela. Ditangani di
/// sini tanpa meminjam App, karena WM_NCCALCSIZE juga datang saat jendela sedang dibuat.
unsafe extern "system" fn settings_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCALCSIZE && wp.0 != 0 {
        return LRESULT(0);
    }
    wndproc(hwnd, msg, wp, lp)
}

/// Dipanggil dari callback IDropTarget (UI thread). Jika App sedang dipinjam (callback
/// terjadi di dalam penanganan pesan lain), event dikirim ulang lewat WM_APP_DROP.
pub fn drop_event(hwnd: HWND, ev: DropEvent) {
    let mut ev = Some(ev);
    APP.with(|a| {
        if let Ok(mut g) = a.try_borrow_mut() {
            if let (Some(app), Some(e)) = (g.as_mut(), ev.take()) {
                app.on_drop(e);
            }
        }
    });
    if let Some(e) = ev {
        unsafe {
            let _ = PostMessageW(hwnd, WM_APP_DROP, WPARAM(0), LPARAM(Box::into_raw(Box::new(e)) as isize));
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_MOUSEACTIVATE {
        return LRESULT(MA_NOACTIVATE as isize);
    }
    let r = APP.with(|a| match a.try_borrow_mut() {
        Ok(mut g) => g.as_mut().and_then(|app| app.on_message(hwnd, msg, wp, lp)),
        Err(_) => None,
    });
    match r {
        Some(l) => l,
        None => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Pesan galat sebelum App ada: ikut bahasa Windows.
fn tr<'a>(id: &'a str, en: &'a str) -> &'a str {
    if util::windows_is_indonesian() {
        id
    } else {
        en
    }
}

fn fatal(msg: &str) {
    let w = util::wide(msg);
    unsafe {
        MessageBoxW(None, PCWSTR(w.as_ptr()), w!("Topline"), MB_ICONERROR | MB_OK);
    }
}

/// `topline progress <start|0-100|done|fail> [label...]`: kirim ke Topline yang sedang
/// berjalan lalu keluar. Tidak pernah memulai Topline baru.
fn send_progress(args: &[String]) {
    let state = args.first().map(String::as_str).unwrap_or("start");
    let label = args.get(1..).map(|r| r.join(" ")).unwrap_or_default();
    let payload = format!("{}\n{}", state, label);
    unsafe {
        if let Ok(h) = FindWindowW(CLASS_NOTCH, None) {
            let cds = COPYDATASTRUCT { dwData: COPYDATA_PROGRESS, cbData: payload.len() as u32, lpData: payload.as_ptr() as *mut _ };
            SendMessageW(h, WM_COPYDATA, WPARAM(0), LPARAM(&cds as *const _ as isize));
        }
    }
}

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("progress") {
        return send_progress(&args[2..]);
    }
    // `topline settings [bagian]`: buka Pengaturan (di instance yang berjalan, atau setelah mulai).
    let settings_arg = (args.get(1).map(String::as_str) == Some("settings"))
        .then(|| args.get(2).and_then(|s| Section::from_arg(s)).unwrap_or(Section::General));
    let settings_msg = settings_arg.map(|s| Section::ALL.iter().position(|x| *x == s).unwrap_or(0));
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // Satu instance saja: instance kedua membuka/menutup notch yang sudah jalan.
        let _mutex = CreateMutexW(None, TRUE, w!("Local\\Topline.SingleInstance"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(h) = FindWindowW(CLASS_NOTCH, None) {
                let _ = match settings_msg {
                    Some(i) => {
                        // izinkan jendela Pengaturan tampil di depan
                        let mut pid = 0u32;
                        GetWindowThreadProcessId(h, Some(&mut pid));
                        let _ = AllowSetForegroundWindow(pid);
                        PostMessageW(h, WM_APP_SETTINGS, WPARAM(i), LPARAM(0))
                    }
                    None => PostMessageW(h, WM_APP_TOGGLE, WPARAM(0), LPARAM(0)),
                };
            }
            return;
        }

        // OLE (tetap STA) untuk drag & drop; kompatibel dengan DispatcherQueue.
        let _ = OleInitialize(None);
        let opts = DispatcherQueueOptions {
            dwSize: size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_STA,
        };
        let _dq = match CreateDispatcherQueueController(opts) {
            Ok(c) => c,
            Err(e) => return fatal(&format!("{}: {e}", tr("Gagal menyiapkan compositor Windows", "Couldn't set up the Windows compositor"))),
        };

        let hinst: HINSTANCE = match GetModuleHandleW(None) {
            Ok(h) => h.into(),
            Err(_) => return,
        };
        for class in [CLASS_NOTCH, CLASS_HOST] {
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: hinst,
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        }

        let ex = WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE;
        let hwnd = match CreateWindowExW(ex, CLASS_NOTCH, w!("Topline"), WS_POPUP, 0, 0, 640, 540, None, None, hinst, None) {
            Ok(h) => h,
            Err(e) => return fatal(&format!("{}: {e}", tr("Gagal membuat jendela", "Couldn't create the window"))),
        };
        let big_icon = util::icon_from_ico(embedded::APP_ICON, GetSystemMetrics(SM_CXICON));
        let small_icon = util::icon_from_ico(embedded::APP_ICON, GetSystemMetrics(SM_CXSMICON));
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(settings_wndproc),
            hInstance: hinst,
            lpszClassName: setwin::CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: big_icon,
            hIconSm: small_icon,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: traymenu::CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let host = CreateWindowExW(WS_EX_TOOLWINDOW, CLASS_HOST, w!("Topline tray"), WS_POPUP, 0, 0, 0, 0, None, None, hinst, None)
            .unwrap_or_default();

        // Kaca: izinkan HostBackdropBrush, dan jangan bulatkan sudut jendela besar.
        let on: BOOL = TRUE;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_HOSTBACKDROPBRUSH, &on as *const _ as *const _, size_of::<BOOL>() as u32);
        let corner = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const _ as *const _, size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32);

        match App::new(hwnd, host) {
            Ok(app) => {
                APP.with(|a| *a.borrow_mut() = Some(app));
                if let Some(i) = settings_msg {
                    let _ = PostMessageW(hwnd, WM_APP_SETTINGS, WPARAM(i), LPARAM(0));
                }
            }
            Err(e) => return fatal(&format!("{}: {e}", tr("Topline gagal dimulai", "Topline failed to start"))),
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        APP.with(|a| *a.borrow_mut() = None);
    }
}
