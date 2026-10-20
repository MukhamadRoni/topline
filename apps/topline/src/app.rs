//! Perakitan aplikasi: jendela, message loop, timer, dan penghubung antara
//! event Windows <-> core <-> tampilan.

use std::cell::RefCell;
use std::mem::size_of;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
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

const ABN_FULLSCREENAPP: u32 = 2;
const WM_MOUSELEAVE: u32 = 0x02A3;

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
            hover: None,
            tracking: false,
            last_view: View::Idle,
            region: (0.0, 0.0),
            edit_rect: None,
            taskbar_created,
            origin: (0, 0),
            eye_motion: util::animations_enabled(),
            eye_demo: util::eye_demo_enabled().then_some(0),
        };
        util::log(&format!(
            "start: scale={} backdrop={} geist={} view={:?}",
            scale, app.notch.has_backdrop, app.gfx.geist, app.core.view
        ));
        let _ = app.notch.set_solid(app.core.data.settings.solid);
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
        self.core.system(SystemEvent::Media { playing }, &now);
        let view_changed = self.core.view != self.last_view;
        if view_changed {
            self.hover = None;
        }
        let ctx = view::Ctx {
            core: &self.core,
            now: &now,
            media: self.media_info.as_ref(),
            cover: self.cover.as_ref(),
            hover: self.hover.as_ref(),
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
        }
    }

    // ---------------------------------------------------------------- actions

    fn command(&mut self, cmd: Command) {
        let now = util::now();
        let fx = self.core.apply(cmd, &now);
        self.effects(fx);
        self.refresh();
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
            Action::Media(m) => {
                self.media.command(m);
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
            Action::PeekWater => {
                let now = util::now();
                let fx = self.core.apply(Command::AddWater(250), &now);
                self.effects(fx);
                self.command(Command::DismissPeek);
            }
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
        self.refresh();
    }

    fn menu(&mut self) {
        let st = MenuState {
            hover: self.core.data.settings.open_on_hover,
            personality: self.core.data.settings.personality,
            autostart: util::autostart_enabled(),
            solid: self.core.data.settings.solid,
            prayer_city: self.core.data.settings.prayer_city.clone(),
            prayer_located: self.core.data.settings.prayer_location.is_some(),
            prayer_place: self.core.prayer_place(),
            adzan: self.core.data.settings.adzan_sound,
            clipboard: self.core.data.settings.clipboard_history,
        };
        match self.tray.show_menu(&st) {
            tray::ID_OPEN => self.command(Command::Open(self.core.last_widget)),
            tray::ID_ADD_TODO => self.command(Command::StartAddTodo),
            tray::ID_HOVER => self.command(Command::SetOpenOnHover(!st.hover)),
            tray::ID_PERSONALITY => self.command(Command::SetPersonality(!st.personality)),
            tray::ID_SOLID => {
                self.core.data.settings.solid = !st.solid;
                let _ = self.notch.set_solid(!st.solid);
                let _ = self.store.save(&self.core.data);
            }
            tray::ID_AUTOSTART => util::set_autostart(!st.autostart),
            tray::ID_ADZAN => self.command(Command::SetAdzanSound(!st.adzan)),
            tray::ID_CLIPBOARD => {
                clip::listen(self.hwnd, !st.clipboard);
                self.command(Command::SetClipboardHistory(!st.clipboard));
            }
            tray::ID_PRAYER_OFF => self.command(Command::SetPrayerCity(String::new())),
            tray::ID_PRAYER_LOCATE => watch::locate(self.hwnd),
            id if (tray::ID_CITY..tray::ID_CITY + prayer::CITIES.len()).contains(&id) => {
                self.command(Command::SetPrayerCity(prayer::CITIES[id - tray::ID_CITY].name.to_string()));
            }
            tray::ID_DATA => unsafe {
                let dir = util::wide(&util::data_dir().display().to_string());
                let _ = std::fs::create_dir_all(util::data_dir());
                ShellExecuteW(None, w!("open"), PCWSTR(dir.as_ptr()), None, None, SW_SHOWNORMAL);
            },
            tray::ID_EXIT => unsafe {
                let _ = PostMessageW(self.hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            _ => {}
        }
    }

    fn shutdown(&mut self) {
        let _ = self.store.save(&self.core.data);
        self.tray.remove();
        self.sys.stop();
        self.watchers.stop();
        clip::listen(self.hwnd, false);
        self.edit.hide();
        unsafe {
            let _ = UnregisterHotKey(self.hwnd, HK_TOGGLE);
            let _ = UnregisterHotKey(self.hwnd, HK_ADD);
        }
    }

    // ---------------------------------------------------------------- messages

    fn on_message(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if hwnd == self.host {
            return self.on_host_message(msg, wp, lp);
        }
        let s = self.scale();
        let mouse = || {
            let x = (lp.0 & 0xffff) as i16 as f32 / s;
            let y = ((lp.0 >> 16) & 0xffff) as i16 as f32 / s;
            (x, y)
        };
        match msg {
            WM_MOUSEMOVE => {
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
                unsafe {
                    let _ = KillTimer(self.hwnd, T_HOVER_CLOSE);
                }
                if self.core.view == View::Idle && self.core.data.settings.open_on_hover {
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
            WM_LBUTTONUP => {
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
                    T_TICK => self.refresh(),
                    T_HOVER_OPEN => {
                        unsafe {
                            let _ = KillTimer(self.hwnd, T_HOVER_OPEN);
                        }
                        if self.core.view == View::Idle {
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
                    let label = if label.trim().is_empty() { "Proses".to_string() } else { label.trim().to_string() };
                    self.system(SystemEvent::Progress { label, state });
                }
                Some(LRESULT(1))
            }
            WM_SETTINGCHANGE => {
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
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        // Satu instance saja: instance kedua membuka/menutup notch yang sudah jalan.
        let _mutex = CreateMutexW(None, TRUE, w!("Local\\Topline.SingleInstance"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(h) = FindWindowW(CLASS_NOTCH, None) {
                let _ = PostMessageW(h, WM_APP_TOGGLE, WPARAM(0), LPARAM(0));
            }
            return;
        }

        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let opts = DispatcherQueueOptions {
            dwSize: size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_STA,
        };
        let _dq = match CreateDispatcherQueueController(opts) {
            Ok(c) => c,
            Err(e) => return fatal(&format!("Gagal menyiapkan compositor Windows: {e}")),
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
            Err(e) => return fatal(&format!("Gagal membuat jendela: {e}")),
        };
        let host = CreateWindowExW(WS_EX_TOOLWINDOW, CLASS_HOST, w!("Topline tray"), WS_POPUP, 0, 0, 0, 0, None, None, hinst, None)
            .unwrap_or_default();

        // Kaca: izinkan HostBackdropBrush, dan jangan bulatkan sudut jendela besar.
        let on: BOOL = TRUE;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_HOSTBACKDROPBRUSH, &on as *const _ as *const _, size_of::<BOOL>() as u32);
        let corner = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &corner as *const _ as *const _, size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32);

        match App::new(hwnd, host) {
            Ok(app) => APP.with(|a| *a.borrow_mut() = Some(app)),
            Err(e) => return fatal(&format!("Topline gagal dimulai: {e}")),
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        APP.with(|a| *a.borrow_mut() = None);
    }
}
