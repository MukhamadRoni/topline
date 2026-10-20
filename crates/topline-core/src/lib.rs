//! Topline core: seluruh logika produk tanpa satu pun panggilan Windows.
//!
//! Lapisan UI mengirim [`Command`] dan event sistem ([`SystemEvent`]) ke [`Core`],
//! memanggil [`Core::tick`] saat deadline dari [`Core::next_deadline_ms`] tiba,
//! lalu menggambar ulang berdasarkan state publik di [`Core`].

pub mod gesture;
pub mod model;
pub mod parse;
pub mod prayer;

pub use gesture::{GestureAction, ScrollGestures, ScrollStep, ScrollZone};
pub use model::*;
pub use parse::parse_task;
pub use prayer::PrayerTimes;

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Widget {
    Music,
    Focus,
    Water,
    Todo,
    Launcher,
    Prayer,
    Clipboard,
}

impl Widget {
    pub const ALL: [Widget; 7] = [
        Widget::Music,
        Widget::Focus,
        Widget::Water,
        Widget::Todo,
        Widget::Launcher,
        Widget::Prayer,
        Widget::Clipboard,
    ];
}

/// Status proses yang dilaporkan lewat `topline progress ...`.
#[derive(Clone, Debug, PartialEq)]
pub enum ProgressState {
    /// Berjalan, dengan persentase jika diketahui.
    Running(Option<u32>),
    Done,
    Failed,
}

impl ProgressState {
    /// Argumen CLI: "start", angka 0–100, "done"/"selesai", "fail"/"gagal".
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().trim_end_matches('%').to_ascii_lowercase().as_str() {
            "start" | "mulai" => Some(Self::Running(None)),
            "done" | "ok" | "selesai" => Some(Self::Done),
            "fail" | "error" | "gagal" => Some(Self::Failed),
            n => n.parse::<u32>().ok().map(|p| Self::Running(Some(p.min(100)))),
        }
    }
}

/// Satu entri riwayat clipboard.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub id: u64,
    pub text: String,
    pub pinned: bool,
}

/// Batas entri tak-disematkan yang disimpan, dan panjang teks per entri.
pub const MAX_CLIPS: usize = 20;
const MAX_CLIP_CHARS: usize = 4000;
pub const MAX_CLIPS_VISIBLE: usize = 6;
const CLIP_COPIED_MS: i64 = 1200;
const TRACK_PEEK_MS: i64 = 1600;
/// Jumlah sematan launcher maksimum (grid 4 x 2).
pub const MAX_PINS: usize = 8;
const PIN_NOTICE_MS: i64 = 3500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Idle,
    Peek,
    Expanded(Widget),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusPhase {
    Focus,
    Break,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FocusState {
    pub phase: FocusPhase,
    pub running: bool,
    /// Sisa waktu saat dijeda (ms).
    pub remaining_ms: i64,
    /// Saat berjalan: kapan fase berakhir (Clock::ms).
    pub ends_at_ms: Option<i64>,
    pub sessions_done: u32,
}

impl FocusState {
    pub fn remaining(&self, now_ms: i64) -> i64 {
        match self.ends_at_ms {
            Some(end) if self.running => (end - now_ms).max(0),
            _ => self.remaining_ms.max(0),
        }
    }
    pub fn label(&self, now_ms: i64) -> String {
        let s = (self.remaining(now_ms) + 999) / 1000;
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PeekKind {
    Volume { percent: u32, muted: bool },
    Charging { percent: u32 },
    Unplugged { percent: u32 },
    BatteryLow { percent: u32 },
    NetworkLost,
    NetworkBack,
    CapsLock { on: bool },
    FocusDone,
    BreakDone,
    WaterReminder,
    TodoDue { text: String, time: String },
    /// Waktu sholat tiba.
    Prayer { name: String, time: String },
    /// Kamera atau mikrofon mulai dipakai oleh `app`.
    CamMic { camera: bool, app: String },
    /// Hasil "Lokasi saat ini": nama tempat, atau None jika gagal.
    Location { place: Option<String> },
    Progress { label: String, percent: Option<u32> },
    ProgressDone { label: String, ok: bool },
    DownloadDone { name: String },
    /// Ganti lagu lewat gestur scroll; judul kosong sampai snapshot media baru tiba.
    Track { title: String, next: bool },
    /// Sematan launcher gagal dibuka.
    LaunchFailed { name: String },
}

impl PeekKind {
    /// Prioritas lebih tinggi menggantikan peek yang sedang tampil.
    pub fn priority(&self) -> u8 {
        match self {
            PeekKind::Volume { .. } | PeekKind::CapsLock { .. } | PeekKind::Track { .. } => 1,
            PeekKind::Charging { .. } | PeekKind::Unplugged { .. } | PeekKind::NetworkBack | PeekKind::Progress { .. } => 2,
            PeekKind::WaterReminder | PeekKind::DownloadDone { .. } | PeekKind::LaunchFailed { .. } => 3,
            PeekKind::NetworkLost
            | PeekKind::BatteryLow { .. }
            | PeekKind::CamMic { .. }
            | PeekKind::ProgressDone { .. }
            | PeekKind::Location { .. } => 4,
            PeekKind::FocusDone | PeekKind::BreakDone | PeekKind::TodoDue { .. } | PeekKind::Prayer { .. } => 5,
        }
    }
    pub fn duration_ms(&self) -> i64 {
        match self {
            PeekKind::Volume { .. } | PeekKind::CapsLock { .. } => 1600,
            PeekKind::Track { .. } => TRACK_PEEK_MS,
            PeekKind::WaterReminder | PeekKind::TodoDue { .. } | PeekKind::FocusDone | PeekKind::BreakDone => 6000,
            PeekKind::Prayer { .. } => 15_000,
            // tanpa kabar baru selama 30 detik dianggap sudah berhenti
            PeekKind::Progress { .. } => 30_000,
            PeekKind::ProgressDone { .. } | PeekKind::DownloadDone { .. } | PeekKind::Location { .. } => 5000,
            _ => 3000,
        }
    }
    fn same_kind(&self, other: &PeekKind) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
    /// Widget yang dibuka saat peek diklik.
    pub fn widget(&self) -> Option<Widget> {
        match self {
            PeekKind::FocusDone | PeekKind::BreakDone => Some(Widget::Focus),
            PeekKind::WaterReminder => Some(Widget::Water),
            PeekKind::TodoDue { .. } => Some(Widget::Todo),
            PeekKind::Prayer { .. } => Some(Widget::Prayer),
            _ => None,
        }
    }
}

/// Ekspresi mata Personality, urut dari prioritas tertinggi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EyeMood {
    /// Sementara: tugas selesai, target air tercapai, sesi fokus selesai.
    Happy,
    /// Sementara: pengguna kembali setelah mata tidur.
    Greeting,
    Sleeping,
    LowBattery,
    Focus,
    Music,
    Normal,
}

impl EyeMood {
    pub const ALL: [EyeMood; 7] = [
        EyeMood::Happy,
        EyeMood::Greeting,
        EyeMood::Sleeping,
        EyeMood::LowBattery,
        EyeMood::Focus,
        EyeMood::Music,
        EyeMood::Normal,
    ];
}

pub const EYE_HAPPY_MS: i64 = 2500;
pub const EYE_GREET_MS: i64 = 1200;
/// Jam 23:00–05:00: mata tertidur setelah 1 menit tanpa input.
const EYE_NIGHT_IDLE_MS: i64 = 60_000;

fn is_night(hour: u32) -> bool {
    hour >= 23 || hour < 5
}

#[derive(Clone, Debug, PartialEq)]
pub struct Peek {
    pub kind: PeekKind,
    pub until_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Buka/tutup (hotkey, klik, tray).
    Toggle,
    Open(Widget),
    Close,
    Switch(Widget),
    FocusStartPause,
    FocusReset,
    FocusSkip,
    AddWater(u32),
    UndoWater,
    ToggleTodo(u64),
    DeleteTodo(u64),
    StartAddTodo,
    SubmitTodo(String),
    CancelAddTodo,
    SetPersonality(bool),
    SetOpenOnHover(bool),
    /// Peek diklik.
    ActivatePeek,
    /// Tutup peek tanpa membuka widget.
    DismissPeek,
    /// Nama kota dari `prayer::CITIES`; string kosong = nonaktif.
    SetPrayerCity(String),
    SetAdzanSound(bool),
    SetClipboardHistory(bool),
    SetScrollGestures(bool),
    /// Drag dari Explorer/Start masuk ke notch: buka launcher dengan zona jatuh.
    DragEnter,
    /// Drag keluar tanpa drop: tutup lagi jika notch dibuka oleh drag.
    DragLeave,
    /// Drop: sematkan (maks. [`MAX_PINS`], duplikat diabaikan).
    AddPins(Vec<PinInput>),
    RemovePin(u64),
    /// Pindahkan sematan ke indeks `to` (atur ulang dengan drag di dalam launcher).
    MovePin { id: u64, to: usize },
    /// Buka target: core menutup notch dan mengembalikan [`Effects::launch`].
    LaunchPin(u64),
    /// Tanya konfirmasi lepas sematan untuk target yang hilang (None = batal).
    AskRemovePin(Option<u64>),
    /// Salin ulang entri riwayat clipboard.
    CopyClip(u64),
    PinClip(u64),
    DeleteClip(u64),
    /// Hapus semua entri yang tidak disematkan.
    ClearClips,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SystemEvent {
    Volume { percent: u32, muted: bool },
    Power { on_ac: bool, percent: Option<u32> },
    Network { connected: bool },
    CapsLock { on: bool },
    /// Status pemutaran media (ekspresi mata) dan judul lagu (None = tidak ada sesi media).
    Media { playing: bool, title: Option<String> },
    /// Ada input pengguna (Raw Input saat mata tidur, atau mouse di atas notch).
    UserActivity,
    /// Teks baru di clipboard (sudah disaring dari format rahasia).
    Clipboard { text: String },
    /// Aplikasi yang sedang memakai kamera/mikrofon (None = tidak dipakai).
    Privacy { camera: Option<String>, mic: Option<String> },
    Progress { label: String, state: ProgressState },
    /// File selesai diunduh ke folder Downloads.
    DownloadDone { name: String },
    /// Hasil layanan lokasi Windows untuk jadwal sholat (None = gagal / ditolak).
    Location(Option<GeoPoint>),
    /// ShellExecuteEx gagal membuka sematan.
    LaunchFailed { name: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    Chime,
    Soft,
    Adzan,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Effects {
    pub save: bool,
    pub redraw: bool,
    pub sound: Option<Sound>,
    /// Teks yang harus ditulis ke clipboard.
    pub clipboard: Option<String>,
    /// Nama file di folder Downloads yang harus ditampilkan di Explorer.
    pub reveal: Option<String>,
    /// Sematan yang harus dibuka (ShellExecuteEx di lapisan aplikasi).
    pub launch: Option<Pin>,
}

impl Effects {
    fn merge(&mut self, o: Effects) {
        self.save |= o.save;
        self.redraw |= o.redraw;
        if o.sound.is_some() {
            self.sound = o.sound;
        }
        if o.clipboard.is_some() {
            self.clipboard = o.clipboard;
        }
        if o.reveal.is_some() {
            self.reveal = o.reveal;
        }
        if o.launch.is_some() {
            self.launch = o.launch;
        }
    }
}

pub struct Core {
    pub data: Persisted,
    pub view: View,
    pub last_widget: Widget,
    pub focus: FocusState,
    pub peek: Option<Peek>,
    queue: VecDeque<PeekKind>,
    pub adding_todo: bool,
    last_power: Option<(bool, Option<u32>)>,
    last_network: Option<bool>,
    battery_low_warned: bool,
    media_playing: bool,
    /// Judul lagu dari snapshot media terakhir (None = tidak ada sesi media).
    media_title: Option<String>,
    /// Judul saat gestur ganti lagu dilakukan; peek Track menunggu judul yang berbeda.
    track_from: Option<String>,
    pub gestures: ScrollGestures,
    /// Drag OLE sedang di atas notch (zona jatuh disorot).
    pub dragging: bool,
    /// Notch dibuka oleh drag (ditutup lagi jika drag dibatalkan).
    drag_opened: bool,
    /// Sematan yang sedang ditanya "Lepas sematan?".
    pub pin_confirm: Option<u64>,
    /// Pesan singkat di launcher (mis. batas sematan) dan batas waktunya.
    pub pin_notice: Option<(String, i64)>,
    /// Input pengguna terakhir (Clock::ms), dari GetLastInputInfo.
    last_input_ms: Option<i64>,
    pub eye_sleeping: bool,
    eye_happy_until: Option<i64>,
    eye_greet_until: Option<i64>,
    /// Jadwal sholat hari ini untuk kota terpilih.
    pub prayer: Option<PrayerTimes>,
    prayer_city_key: String,
    /// Waktu sholat terakhir (menit) yang sudah diingatkan / dilewati hari ini.
    prayer_done_min: u32,
    /// Riwayat clipboard, terbaru di depan. Tidak disimpan ke disk.
    pub clips: Vec<Clip>,
    clip_next_id: u64,
    /// Entri yang baru disalin ulang (untuk tanda "Disalin") dan batas waktunya.
    pub clip_copied: Option<(u64, i64)>,
    /// Aplikasi yang sedang memakai kamera / mikrofon.
    pub camera: Option<String>,
    pub mic: Option<String>,
    privacy_seen: bool,
}

pub const MAX_TODOS_VISIBLE: usize = 6;

impl Core {
    pub fn new(mut data: Persisted, now: &Clock) -> Self {
        if data.next_id == 0 {
            data.next_id = 1;
        }
        let focus_ms = data.settings.focus_min as i64 * 60_000;
        let mut c = Self {
            data,
            view: View::Idle,
            last_widget: Widget::Music,
            focus: FocusState {
                phase: FocusPhase::Focus,
                running: false,
                remaining_ms: focus_ms,
                ends_at_ms: None,
                sessions_done: 0,
            },
            peek: None,
            queue: VecDeque::new(),
            adding_todo: false,
            last_power: None,
            last_network: None,
            battery_low_warned: false,
            media_playing: false,
            media_title: None,
            track_from: None,
            gestures: ScrollGestures::default(),
            dragging: false,
            drag_opened: false,
            pin_confirm: None,
            pin_notice: None,
            last_input_ms: None,
            eye_sleeping: false,
            eye_happy_until: None,
            eye_greet_until: None,
            prayer: None,
            prayer_city_key: String::new(),
            prayer_done_min: 0,
            clips: Vec::new(),
            clip_next_id: 1,
            clip_copied: None,
            camera: None,
            mic: None,
            privacy_seen: false,
        };
        c.rollover(now);
        c
    }

    pub fn is_open(&self) -> bool {
        matches!(self.view, View::Expanded(_))
    }

    pub fn widget(&self) -> Option<Widget> {
        match self.view {
            View::Expanded(w) => Some(w),
            _ => None,
        }
    }

    /// Tugas untuk hari ini (dan tugas lama yang belum selesai), maksimum yang terlihat.
    pub fn visible_todos(&self, now: &Clock) -> Vec<&Todo> {
        let today = now.date_key();
        let mut v: Vec<&Todo> = self
            .data
            .todos
            .iter()
            .filter(|t| t.date == today || !t.done)
            .collect();
        if v.len() > MAX_TODOS_VISIBLE {
            let cut = v.len() - MAX_TODOS_VISIBLE;
            v.drain(0..cut);
        }
        v
    }

    pub fn water_percent(&self) -> u32 {
        let goal = self.data.settings.water_goal_ml.max(1);
        ((self.data.water.total_ml as u64 * 100 / goal as u64) as u32).min(100)
    }

    pub fn next_water_reminder(&self) -> Option<u32> {
        let s = &self.data.settings;
        if self.data.water.total_ml >= s.water_goal_ml {
            return None;
        }
        let start = s.water_start_hour * 60;
        let end = s.water_end_hour * 60;
        let base = self
            .data
            .water
            .entries
            .last()
            .map(|e| e.0)
            .into_iter()
            .chain(self.data.water.last_reminder_min)
            .max()
            .unwrap_or(start.saturating_sub(s.water_interval_min));
        let next = (base + s.water_interval_min).max(start);
        if next > end {
            None
        } else {
            Some(next)
        }
    }

    /// Tab yang tampil di notch terbuka. Sholat hanya jika kota dipilih,
    /// Clipboard hanya jika riwayat aktif.
    pub fn tabs(&self) -> Vec<Widget> {
        let s = &self.data.settings;
        Widget::ALL
            .into_iter()
            .filter(|w| match w {
                Widget::Prayer => !s.prayer_city.is_empty(),
                Widget::Clipboard => s.clipboard_history,
                _ => true,
            })
            .collect()
    }

    // ---------------------------------------------------------------- sholat

    /// Hitung ulang jadwal jika kota atau tanggal berubah. Waktu yang sudah lewat
    /// saat dihitung tidak diingatkan.
    fn refresh_prayer(&mut self, now: &Clock) {
        let s = &self.data.settings;
        let (c, key) = match (s.prayer_location, prayer::city(&s.prayer_city)) {
            (Some(g), _) => {
                let n = prayer::nearest(g.lat, g.lon);
                let c = prayer::City { lat: g.lat, lon: g.lon, tz: g.tz, ..*n };
                (c, format!("@{:.4},{:.4},{}", g.lat, g.lon, g.tz))
            }
            (None, Some(c)) => (*c, c.name.to_string()),
            (None, None) => {
                self.prayer = None;
                self.prayer_city_key.clear();
                return;
            }
        };
        let today = now.date_key();
        if self.prayer.as_ref().map_or(true, |p| p.date != today) || self.prayer_city_key != key {
            self.prayer = Some(prayer::compute(&c, now.year, now.month, now.day));
            self.prayer_city_key = key;
            self.prayer_done_min = now.minute_of_day();
        }
    }

    /// Nama tempat jadwal sholat untuk label: "Surabaya" atau "sekitar Surabaya" (lokasi).
    pub fn prayer_place(&self) -> String {
        let s = &self.data.settings;
        match s.prayer_location {
            Some(_) => format!("sekitar {}", s.prayer_city),
            None => s.prayer_city.clone(),
        }
    }

    /// Indeks (dalam `prayer::NAMES`) waktu sholat berikutnya hari ini.
    pub fn next_prayer(&self, now: &Clock) -> Option<usize> {
        let p = self.prayer.as_ref()?;
        prayer::NOTIFY.into_iter().find(|&i| p.min[i] > now.minute_of_day())
    }

    // ---------------------------------------------------------------- clipboard

    /// Entri yang ditampilkan: yang disematkan dulu, lalu terbaru.
    pub fn visible_clips(&self) -> Vec<&Clip> {
        let mut v: Vec<&Clip> = self.clips.iter().filter(|c| c.pinned).collect();
        v.extend(self.clips.iter().filter(|c| !c.pinned));
        v.truncate(MAX_CLIPS_VISIBLE);
        v
    }

    fn add_clip(&mut self, text: String) {
        if !self.data.settings.clipboard_history || text.trim().is_empty() {
            return;
        }
        let text: String = text.chars().take(MAX_CLIP_CHARS).collect();
        let entry = match self.clips.iter().position(|c| c.text == text) {
            Some(i) => self.clips.remove(i),
            None => {
                let id = self.clip_next_id;
                self.clip_next_id += 1;
                Clip { id, text, pinned: false }
            }
        };
        self.clips.insert(0, entry);
        let mut unpinned = 0;
        self.clips.retain(|c| {
            unpinned += !c.pinned as usize;
            c.pinned || unpinned <= MAX_CLIPS
        });
    }

    // ---------------------------------------------------------------- mata

    /// Ekspresi mata saat ini menurut prioritas.
    pub fn eye_mood(&self, now: &Clock) -> EyeMood {
        let active = |t: Option<i64>| t.map_or(false, |t| now.ms < t);
        if active(self.eye_happy_until) {
            EyeMood::Happy
        } else if active(self.eye_greet_until) {
            EyeMood::Greeting
        } else if self.eye_sleeping {
            EyeMood::Sleeping
        } else if matches!(self.last_power, Some((false, Some(p))) if p <= 20) {
            EyeMood::LowBattery
        } else if self.focus.running && self.focus.phase == FocusPhase::Focus {
            EyeMood::Focus
        } else if self.media_playing {
            EyeMood::Music
        } else {
            EyeMood::Normal
        }
    }

    /// Catat waktu input terakhir dari sistem (GetLastInputInfo). Tidak membangunkan
    /// mata; itu tugas [`SystemEvent::UserActivity`].
    pub fn set_last_input(&mut self, ms: i64) {
        self.last_input_ms = Some(self.last_input_ms.map_or(ms, |t| t.max(ms)));
    }

    /// Kapan mata harus tertidur, jika sedang relevan (Personality, idle, belum tidur).
    fn eye_sleep_at(&self, now: &Clock) -> Option<i64> {
        if !self.data.settings.personality || self.eye_sleeping || self.view != View::Idle {
            return None;
        }
        let last = self.last_input_ms?;
        let normal = last + self.data.settings.eye_sleep_minutes.max(1) as i64 * 60_000;
        let night_start = if is_night(now.hour) {
            now.ms
        } else {
            now.ms + ((23 * 60 - now.minute_of_day() as i64) * 60 - now.second as i64) * 1000
        };
        Some(normal.min((last + EYE_NIGHT_IDLE_MS).max(night_start)))
    }

    fn note_activity(&mut self, now: &Clock) {
        self.set_last_input(now.ms);
        if self.eye_sleeping {
            self.eye_sleeping = false;
            self.eye_greet_until = Some(now.ms + EYE_GREET_MS);
        }
    }

    fn eye_happy(&mut self, now: &Clock) {
        self.eye_happy_until = Some(now.ms + EYE_HAPPY_MS);
    }

    // ---------------------------------------------------------------- commands

    pub fn apply(&mut self, cmd: Command, now: &Clock) -> Effects {
        let mut fx = Effects { redraw: true, ..Default::default() };
        // Semua command berasal dari pengguna: membuka notch juga membangunkan mata.
        self.note_activity(now);
        match cmd {
            Command::Toggle => {
                if self.is_open() {
                    self.close();
                } else {
                    self.open(self.last_widget);
                }
            }
            Command::Open(w) => self.open(w),
            Command::Close => self.close(),
            Command::Switch(w) => {
                self.adding_todo = false;
                self.open(w)
            }
            Command::FocusStartPause => {
                if self.focus.running {
                    self.focus.remaining_ms = self.focus.remaining(now.ms);
                    self.focus.running = false;
                    self.focus.ends_at_ms = None;
                } else {
                    if self.focus.remaining_ms <= 0 {
                        self.focus.remaining_ms = self.phase_len(self.focus.phase);
                    }
                    self.focus.running = true;
                    self.focus.ends_at_ms = Some(now.ms + self.focus.remaining_ms);
                }
            }
            Command::FocusReset => {
                self.focus.running = false;
                self.focus.ends_at_ms = None;
                self.focus.phase = FocusPhase::Focus;
                self.focus.remaining_ms = self.phase_len(FocusPhase::Focus);
            }
            Command::FocusSkip => {
                let next = match self.focus.phase {
                    FocusPhase::Focus => FocusPhase::Break,
                    FocusPhase::Break => FocusPhase::Focus,
                };
                self.focus.phase = next;
                self.focus.running = false;
                self.focus.ends_at_ms = None;
                self.focus.remaining_ms = self.phase_len(next);
            }
            Command::AddWater(ml) => {
                self.rollover(now);
                let goal = self.data.settings.water_goal_ml;
                let before = self.data.water.total_ml;
                self.data.water.total_ml += ml;
                self.data.water.entries.push((now.minute_of_day(), ml));
                if before < goal && self.data.water.total_ml >= goal {
                    self.eye_happy(now);
                }
                fx.save = true;
            }
            Command::UndoWater => {
                if let Some((_, ml)) = self.data.water.entries.pop() {
                    self.data.water.total_ml = self.data.water.total_ml.saturating_sub(ml);
                    fx.save = true;
                }
            }
            Command::ToggleTodo(id) => {
                if let Some(t) = self.data.todos.iter_mut().find(|t| t.id == id) {
                    t.done = !t.done;
                    fx.save = true;
                    if t.done {
                        self.eye_happy(now);
                    }
                }
            }
            Command::DeleteTodo(id) => {
                self.data.todos.retain(|t| t.id != id);
                fx.save = true;
            }
            Command::StartAddTodo => {
                self.open(Widget::Todo);
                self.adding_todo = true;
            }
            Command::SubmitTodo(text) => {
                let (t, time) = parse_task(&text);
                if !t.is_empty() {
                    let id = self.data.next_id;
                    self.data.next_id += 1;
                    let already_past = time.map_or(false, |(h, m)| h * 60 + m < now.minute_of_day());
                    self.data.todos.push(Todo {
                        id,
                        text: t,
                        time,
                        date: now.date_key(),
                        done: false,
                        notified: already_past,
                    });
                    fx.save = true;
                    // Hapus tugas selesai yang lama agar daftar tetap ringkas.
                    let today = now.date_key();
                    self.data.todos.retain(|t| !(t.done && t.date != today));
                }
                // tetap dalam mode menambah untuk input berturut-turut
            }
            Command::CancelAddTodo => self.adding_todo = false,
            Command::SetPersonality(on) => {
                self.data.settings.personality = on;
                fx.save = true;
            }
            Command::SetOpenOnHover(on) => {
                self.data.settings.open_on_hover = on;
                fx.save = true;
            }
            Command::DismissPeek => {
                self.peek = None;
                if self.view == View::Peek {
                    self.view = View::Idle;
                }
            }
            Command::ActivatePeek => {
                let peek = self.peek.take();
                if let Some(PeekKind::DownloadDone { name }) = peek.as_ref().map(|p| &p.kind) {
                    fx.reveal = Some(name.clone());
                }
                match peek.and_then(|p| p.kind.widget()) {
                    Some(w) => self.open(w),
                    None => self.view = View::Idle,
                }
            }
            Command::SetPrayerCity(city) => {
                self.data.settings.prayer_city = if prayer::city(&city).is_some() { city } else { String::new() };
                self.data.settings.prayer_location = None;
                self.refresh_prayer(now);
                if self.widget() == Some(Widget::Prayer) && self.prayer.is_none() {
                    self.open(Widget::Music);
                }
                fx.save = true;
            }
            Command::SetAdzanSound(on) => {
                self.data.settings.adzan_sound = on;
                fx.save = true;
            }
            Command::SetClipboardHistory(on) => {
                self.data.settings.clipboard_history = on;
                if !on {
                    self.clips.clear();
                    self.clip_copied = None;
                    if self.widget() == Some(Widget::Clipboard) {
                        self.open(Widget::Music);
                    }
                }
                fx.save = true;
            }
            Command::CopyClip(id) => {
                if let Some(text) = self.clips.iter().find(|c| c.id == id).map(|c| c.text.clone()) {
                    fx.clipboard = Some(text.clone());
                    self.clip_copied = Some((id, now.ms + CLIP_COPIED_MS));
                    self.add_clip(text);
                }
            }
            Command::PinClip(id) => {
                if let Some(c) = self.clips.iter_mut().find(|c| c.id == id) {
                    c.pinned = !c.pinned;
                }
            }
            Command::SetScrollGestures(on) => {
                self.data.settings.scroll_gestures = on;
                fx.save = true;
            }
            Command::DragEnter => {
                self.dragging = true;
                if !self.is_open() {
                    self.drag_opened = true;
                }
                self.open(Widget::Launcher);
            }
            Command::DragLeave => {
                self.dragging = false;
                if std::mem::take(&mut self.drag_opened) {
                    self.close();
                }
            }
            Command::AddPins(items) => {
                self.dragging = false;
                self.drag_opened = false;
                let mut full = false;
                for it in items {
                    let target = it.target.trim().to_string();
                    let key = target.to_lowercase();
                    if target.is_empty() || self.data.pins.iter().any(|p| p.target.to_lowercase() == key) {
                        continue;
                    }
                    if self.data.pins.len() >= MAX_PINS {
                        full = true;
                        break;
                    }
                    let id = self.data.next_id;
                    self.data.next_id += 1;
                    let name = if it.name.trim().is_empty() { target.clone() } else { it.name.trim().to_string() };
                    self.data.pins.push(Pin { id, kind: it.kind, target, name });
                    fx.save = true;
                }
                if full {
                    self.pin_notice = Some(("Maksimal 8. Lepas satu sematan dulu.".into(), now.ms + PIN_NOTICE_MS));
                }
                self.open(Widget::Launcher);
            }
            Command::RemovePin(id) => {
                self.data.pins.retain(|p| p.id != id);
                self.pin_confirm = None;
                fx.save = true;
            }
            Command::MovePin { id, to } => {
                if let Some(i) = self.data.pins.iter().position(|p| p.id == id) {
                    let p = self.data.pins.remove(i);
                    let to = to.min(self.data.pins.len());
                    self.data.pins.insert(to, p);
                    fx.save = i != to;
                }
            }
            Command::LaunchPin(id) => {
                if let Some(p) = self.data.pins.iter().find(|p| p.id == id) {
                    fx.launch = Some(p.clone());
                    self.close();
                }
            }
            Command::AskRemovePin(id) => self.pin_confirm = id,
            Command::DeleteClip(id) => self.clips.retain(|c| c.id != id),
            Command::ClearClips => self.clips.retain(|c| c.pinned),
        }
        fx.merge(self.tick(now));
        fx
    }

    fn phase_len(&self, p: FocusPhase) -> i64 {
        let s = &self.data.settings;
        match p {
            FocusPhase::Focus => s.focus_min as i64 * 60_000,
            FocusPhase::Break => s.break_min as i64 * 60_000,
        }
    }

    fn open(&mut self, w: Widget) {
        let w = if self.tabs().contains(&w) { w } else { Widget::Music };
        if w != Widget::Todo {
            self.adding_todo = false;
        }
        if w != Widget::Launcher {
            self.pin_confirm = None;
        }
        self.view = View::Expanded(w);
        self.last_widget = w;
    }

    fn close(&mut self) {
        self.adding_todo = false;
        self.pin_confirm = None;
        self.view = if self.peek.is_some() { View::Peek } else { View::Idle };
    }

    // ---------------------------------------------------------------- gestur

    /// Scroll di notch. Pindah tab dan peek ganti lagu ditangani di sini; volume dan
    /// perintah media dijalankan lapisan aplikasi dari aksi yang dikembalikan.
    pub fn scroll(&mut self, zone: ScrollZone, delta: i32, horizontal: bool, shift: bool, now: &Clock) -> Option<GestureAction> {
        if !self.data.settings.scroll_gestures {
            return None;
        }
        self.note_activity(now);
        match self.gestures.feed(zone, delta, horizontal, shift, now.ms)? {
            ScrollStep::Volume(d) => Some(GestureAction::Volume(d)),
            ScrollStep::Track { next } => {
                self.media_title.as_ref()?;
                self.track_from = self.media_title.clone();
                self.push_peek(PeekKind::Track { title: String::new(), next }, now);
                Some(GestureAction::Track { next })
            }
            ScrollStep::Tab(dir) => {
                let tabs = self.tabs();
                let cur = tabs.iter().position(|w| Some(*w) == self.widget())?;
                let to = (cur as i32 + dir).clamp(0, tabs.len() as i32 - 1) as usize;
                if to == cur {
                    return None;
                }
                self.adding_todo = false;
                self.open(tabs[to]);
                Some(GestureAction::Tab)
            }
        }
    }

    /// Klik tengah di notch: play/pause jika ada sesi media, selain itu mute.
    pub fn middle_click(&self) -> Option<GestureAction> {
        if !self.data.settings.scroll_gestures {
            return None;
        }
        Some(if self.media_title.is_some() { GestureAction::PlayPause } else { GestureAction::ToggleMute })
    }

    // ---------------------------------------------------------------- system events

    pub fn system(&mut self, ev: SystemEvent, now: &Clock) -> Effects {
        let peeks_on = self.data.settings.system_peeks;
        match ev {
            SystemEvent::Volume { percent, muted } => {
                if peeks_on {
                    self.push_peek(PeekKind::Volume { percent, muted }, now);
                }
            }
            SystemEvent::CapsLock { on } => {
                if peeks_on {
                    self.push_peek(PeekKind::CapsLock { on }, now);
                }
            }
            SystemEvent::Power { on_ac, percent } => {
                let prev = self.last_power.replace((on_ac, percent));
                if let Some((was_ac, _)) = prev {
                    let p = percent.unwrap_or(0);
                    if on_ac && !was_ac {
                        self.battery_low_warned = false;
                        self.push_peek(PeekKind::Charging { percent: p }, now);
                    } else if !on_ac && was_ac {
                        self.push_peek(PeekKind::Unplugged { percent: p }, now);
                    }
                }
                if let (false, Some(p)) = (on_ac, percent) {
                    if p <= 20 && !self.battery_low_warned {
                        self.battery_low_warned = true;
                        self.push_peek(PeekKind::BatteryLow { percent: p }, now);
                    }
                }
            }
            SystemEvent::Network { connected } => {
                let prev = self.last_network.replace(connected);
                if let Some(was) = prev {
                    if was && !connected {
                        self.push_peek(PeekKind::NetworkLost, now);
                    } else if !was && connected {
                        self.push_peek(PeekKind::NetworkBack, now);
                    }
                }
            }
            SystemEvent::Media { playing, title } => {
                self.media_playing = playing;
                // Peek Track menunggu judul lagu baru (berbeda dari saat gestur).
                if let Some(Peek { kind: PeekKind::Track { title: t, .. }, until_ms }) = &mut self.peek {
                    if let Some(new) = title.as_ref().filter(|n| t.is_empty() && !n.is_empty() && Some(*n) != self.track_from.as_ref()) {
                        *t = new.clone();
                        *until_ms = now.ms + TRACK_PEEK_MS;
                    }
                }
                self.media_title = title;
            }
            SystemEvent::UserActivity => self.note_activity(now),
            SystemEvent::Clipboard { text } => self.add_clip(text),
            SystemEvent::Privacy { camera, mic } => {
                let first = !std::mem::replace(&mut self.privacy_seen, true);
                let started_cam = camera.is_some() && self.camera.is_none();
                let started_mic = mic.is_some() && self.mic.is_none();
                if !first && peeks_on {
                    if let (true, Some(app)) = (started_cam, &camera) {
                        self.push_peek(PeekKind::CamMic { camera: true, app: app.clone() }, now);
                    } else if let (true, Some(app)) = (started_mic, &mic) {
                        self.push_peek(PeekKind::CamMic { camera: false, app: app.clone() }, now);
                    }
                }
                self.camera = camera;
                self.mic = mic;
            }
            SystemEvent::Progress { label, state } => match state {
                ProgressState::Running(percent) => {
                    self.push_peek(PeekKind::Progress { label, percent: percent.map(|p| p.min(100)) }, now);
                }
                ProgressState::Done | ProgressState::Failed => {
                    if matches!(self.peek.as_ref().map(|p| &p.kind), Some(PeekKind::Progress { .. })) {
                        self.peek = None;
                    }
                    let ok = state == ProgressState::Done;
                    self.push_peek(PeekKind::ProgressDone { label, ok }, now);
                    return Effects { redraw: true, sound: Some(Sound::Soft), ..Default::default() };
                }
            },
            SystemEvent::DownloadDone { name } => self.push_peek(PeekKind::DownloadDone { name }, now),
            SystemEvent::LaunchFailed { name } => self.push_peek(PeekKind::LaunchFailed { name }, now),
            SystemEvent::Location(point) => {
                let Some(g) = point else {
                    self.push_peek(PeekKind::Location { place: None }, now);
                    return Effects { redraw: true, ..Default::default() };
                };
                self.data.settings.prayer_location = Some(g);
                self.data.settings.prayer_city = prayer::nearest(g.lat, g.lon).name.to_string();
                self.refresh_prayer(now);
                self.push_peek(PeekKind::Location { place: Some(self.prayer_place()) }, now);
                return Effects { redraw: true, save: true, ..Default::default() };
            }
        }
        Effects { redraw: true, ..Default::default() }
    }

    pub fn push_peek(&mut self, kind: PeekKind, now: &Clock) {
        if self.is_open() {
            // Saat notch terbuka, hanya simpan peek penting untuk nanti.
            if kind.priority() >= 4 {
                self.queue.push_back(kind);
            }
            return;
        }
        let until = now.ms + kind.duration_ms();
        match &mut self.peek {
            Some(cur) if cur.kind.same_kind(&kind) => {
                cur.kind = kind;
                cur.until_ms = until;
            }
            Some(cur) if cur.kind.priority() > kind.priority() => {
                if kind.priority() >= 3 {
                    self.queue.push_back(kind);
                }
            }
            _ => {
                self.peek = Some(Peek { kind, until_ms: until });
                self.view = View::Peek;
            }
        }
    }

    // ---------------------------------------------------------------- time

    fn rollover(&mut self, now: &Clock) {
        let today = now.date_key();
        if self.data.water.date != today {
            self.data.water = WaterDay { date: today, ..Default::default() };
        }
        self.refresh_prayer(now);
    }

    /// Dipanggil saat deadline tiba (atau kapan saja). Idempoten.
    pub fn tick(&mut self, now: &Clock) -> Effects {
        let mut fx = Effects::default();
        let before = self.data.water.date.clone();
        self.rollover(now);
        if before != self.data.water.date {
            fx.save = true;
            fx.redraw = true;
        }

        // Pomodoro
        if self.focus.running && self.focus.remaining(now.ms) == 0 {
            let finished = self.focus.phase;
            fx.sound = Some(Sound::Chime);
            fx.redraw = true;
            match finished {
                FocusPhase::Focus => {
                    self.focus.sessions_done += 1;
                    self.focus.phase = FocusPhase::Break;
                    let len = self.phase_len(FocusPhase::Break);
                    self.focus.remaining_ms = len;
                    self.focus.ends_at_ms = Some(now.ms + len);
                    self.focus.running = true;
                    self.eye_happy(now);
                    self.push_peek(PeekKind::FocusDone, now);
                }
                FocusPhase::Break => {
                    self.focus.phase = FocusPhase::Focus;
                    self.focus.running = false;
                    self.focus.ends_at_ms = None;
                    self.focus.remaining_ms = self.phase_len(FocusPhase::Focus);
                    if self.focus.sessions_done >= self.data.settings.sessions_target {
                        self.focus.sessions_done = 0;
                    }
                    self.push_peek(PeekKind::BreakDone, now);
                }
            }
        }

        // Pengingat minum
        if let Some(due) = self.next_water_reminder() {
            if now.minute_of_day() >= due {
                self.data.water.last_reminder_min = Some(now.minute_of_day());
                fx.save = true;
                fx.redraw = true;
                fx.sound.get_or_insert(Sound::Soft);
                self.push_peek(PeekKind::WaterReminder, now);
            }
        }

        // To-do jatuh tempo
        let today = now.date_key();
        let mut due_items = Vec::new();
        for t in self.data.todos.iter_mut() {
            if !t.done && !t.notified && t.date == today {
                if let Some((h, m)) = t.time {
                    if now.minute_of_day() >= h * 60 + m {
                        t.notified = true;
                        due_items.push(PeekKind::TodoDue { text: t.text.clone(), time: format!("{:02}:{:02}", h, m) });
                    }
                }
            }
        }
        if !due_items.is_empty() {
            fx.save = true;
            fx.redraw = true;
            fx.sound = Some(Sound::Chime);
            for k in due_items {
                self.push_peek(k, now);
            }
        }

        // Peek kedaluwarsa -> berikutnya dari antrean
        if let Some(p) = &self.peek {
            if now.ms >= p.until_ms {
                self.peek = None;
                fx.redraw = true;
                if self.view == View::Peek {
                    self.view = View::Idle;
                }
            }
        }
        if self.peek.is_none() && !self.is_open() {
            if let Some(k) = self.queue.pop_front() {
                self.push_peek(k, now);
                fx.redraw = true;
            }
        }

        // Waktu sholat: hanya yang terakhir lewat yang diingatkan (mis. setelah sleep).
        let due = self.prayer.as_ref().and_then(|p| {
            prayer::NOTIFY
                .into_iter()
                .filter(|&i| p.min[i] > self.prayer_done_min && p.min[i] <= now.minute_of_day())
                .last()
                .map(|i| (p.min[i], PeekKind::Prayer { name: prayer::NAMES[i].to_string(), time: p.label(i) }))
        });
        if let Some((min, kind)) = due {
            self.prayer_done_min = min;
            fx.redraw = true;
            if self.data.settings.adzan_sound {
                fx.sound = Some(Sound::Adzan);
            }
            self.push_peek(kind, now);
        }

        if self.clip_copied.map_or(false, |(_, t)| now.ms >= t) {
            self.clip_copied = None;
            fx.redraw = true;
        }
        if self.pin_notice.as_ref().map_or(false, |(_, t)| now.ms >= *t) {
            self.pin_notice = None;
            fx.redraw = true;
        }

        // Mata: ekspresi sementara kedaluwarsa, lalu cek waktu tidur.
        for t in [&mut self.eye_happy_until, &mut self.eye_greet_until] {
            if t.map_or(false, |t| now.ms >= t) {
                *t = None;
                fx.redraw = true;
            }
        }
        if self.eye_sleep_at(now).map_or(false, |at| now.ms >= at) {
            self.eye_sleeping = true;
            fx.redraw = true;
        }
        fx
    }

    /// Kapan `tick` perlu dipanggil lagi (Clock::ms). Tidak termasuk
    /// penggambaran ulang per detik/menit yang diatur oleh lapisan UI.
    pub fn next_deadline_ms(&self, now: &Clock) -> Option<i64> {
        let mut best: Option<i64> = None;
        let mut consider = |t: i64| {
            best = Some(best.map_or(t, |b: i64| b.min(t)));
        };
        if let Some(p) = &self.peek {
            consider(p.until_ms);
        }
        if self.focus.running {
            if let Some(end) = self.focus.ends_at_ms {
                consider(end);
            }
        }
        let to_minute = |min: u32| now.ms + (min as i64 - now.minute_of_day() as i64) * 60_000 - now.second as i64 * 1000;
        if let Some(m) = self.next_water_reminder() {
            consider(to_minute(m).max(now.ms + 1000));
        }
        let today = now.date_key();
        for t in &self.data.todos {
            if !t.done && !t.notified && t.date == today {
                if let Some((h, m)) = t.time {
                    consider(to_minute(h * 60 + m).max(now.ms + 1000));
                }
            }
        }
        if let Some(p) = &self.prayer {
            if let Some(m) = prayer::NOTIFY.into_iter().map(|i| p.min[i]).find(|&m| m > self.prayer_done_min) {
                consider(to_minute(m).max(now.ms + 1000));
            }
        }
        if let Some((_, t)) = self.clip_copied {
            consider(t);
        }
        if let Some((_, t)) = &self.pin_notice {
            consider(*t);
        }
        // mata (hanya saat Personality aktif)
        if self.data.settings.personality {
            for t in [self.eye_happy_until, self.eye_greet_until, self.eye_sleep_at(now)].into_iter().flatten() {
                consider(t);
            }
        }
        // pergantian hari
        let to_midnight = (24 * 60 - now.minute_of_day()) as i64 * 60_000 - now.second as i64 * 1000;
        consider(now.ms + to_midnight.max(1000));
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(h: u32, m: u32, s: u32) -> Clock {
        let ms = ((h * 3600 + m * 60 + s) as i64) * 1000;
        Clock { ms, year: 2026, month: 10, day: 20, hour: h, minute: m, second: s, weekday: 2 }
    }

    #[test]
    fn toggle_opens_last_widget() {
        let now = clock(9, 0, 0);
        let mut c = Core::new(Persisted::default(), &now);
        c.apply(Command::Open(Widget::Water), &now);
        c.apply(Command::Close, &now);
        assert_eq!(c.view, View::Idle);
        c.apply(Command::Toggle, &now);
        assert_eq!(c.view, View::Expanded(Widget::Water));
    }

    #[test]
    fn pomodoro_runs_into_break_and_back() {
        let now = clock(9, 0, 0);
        let mut c = Core::new(Persisted::default(), &now);
        c.apply(Command::FocusStartPause, &now);
        assert!(c.focus.running);
        assert_eq!(c.focus.label(now.ms), "25:00");
        let end = clock(9, 25, 0);
        let fx = c.tick(&end);
        assert_eq!(fx.sound, Some(Sound::Chime));
        assert_eq!(c.focus.phase, FocusPhase::Break);
        assert_eq!(c.focus.sessions_done, 1);
        assert!(matches!(c.peek.as_ref().unwrap().kind, PeekKind::FocusDone));
        let end2 = clock(9, 30, 0);
        c.tick(&end2);
        assert_eq!(c.focus.phase, FocusPhase::Focus);
        assert!(!c.focus.running);
    }

    #[test]
    fn pause_keeps_remaining() {
        let t0 = clock(9, 0, 0);
        let mut c = Core::new(Persisted::default(), &t0);
        c.apply(Command::FocusStartPause, &t0);
        let t1 = clock(9, 10, 0);
        c.apply(Command::FocusStartPause, &t1);
        assert_eq!(c.focus.label(clock(10, 0, 0).ms), "15:00");
    }

    #[test]
    fn water_reminder_every_interval_and_stops_at_goal() {
        let t0 = clock(7, 0, 0);
        let mut c = Core::new(Persisted::default(), &t0);
        assert_eq!(c.next_water_reminder(), Some(8 * 60));
        c.tick(&clock(8, 0, 0));
        assert!(matches!(c.peek.as_ref().unwrap().kind, PeekKind::WaterReminder));
        assert_eq!(c.next_water_reminder(), Some(9 * 60));
        c.apply(Command::AddWater(250), &clock(8, 30, 0));
        assert_eq!(c.next_water_reminder(), Some(9 * 60 + 30));
        c.apply(Command::AddWater(2000), &clock(8, 31, 0));
        assert_eq!(c.next_water_reminder(), None);
        assert_eq!(c.water_percent(), 100);
    }

    #[test]
    fn water_resets_next_day() {
        let t0 = clock(9, 0, 0);
        let mut c = Core::new(Persisted::default(), &t0);
        c.apply(Command::AddWater(500), &t0);
        let mut tomorrow = clock(9, 0, 0);
        tomorrow.day = 21;
        c.tick(&tomorrow);
        assert_eq!(c.data.water.total_ml, 0);
    }

    #[test]
    fn todo_submit_parses_time_and_notifies() {
        let t0 = clock(16, 0, 0);
        let mut c = Core::new(Persisted::default(), &t0);
        c.apply(Command::StartAddTodo, &t0);
        assert!(c.adding_todo);
        c.apply(Command::SubmitTodo("Rapat tim 16:30".into()), &t0);
        assert!(c.adding_todo, "tetap mode tambah untuk input berturut-turut");
        assert_eq!(c.data.todos[0].text, "Rapat tim");
        assert_eq!(c.data.todos[0].time, Some((16, 30)));
        c.apply(Command::Close, &t0);
        c.tick(&clock(16, 30, 0));
        assert!(matches!(c.peek.as_ref().unwrap().kind, PeekKind::TodoDue { .. }));
        assert!(c.data.todos[0].notified);
    }

    #[test]
    fn past_time_todo_not_notified() {
        let t0 = clock(17, 0, 0);
        let mut c = Core::new(Persisted::default(), &t0);
        c.data.water.total_ml = 5000;
        c.apply(Command::SubmitTodo("Lapor 9:00".into()), &t0);
        c.tick(&t0);
        assert!(c.peek.is_none());
    }

    #[test]
    fn peeks_expire_and_queue() {
        let t0 = clock(10, 0, 0);
        let mut c = Core::new(Persisted::default(), &t0);
        c.data.water.total_ml = 5000; // tanpa pengingat air
        c.system(SystemEvent::Network { connected: true }, &t0);
        c.system(SystemEvent::Network { connected: false }, &t0);
        assert!(matches!(c.peek.as_ref().unwrap().kind, PeekKind::NetworkLost));
        c.system(SystemEvent::Volume { percent: 40, muted: false }, &t0);
        assert!(matches!(c.peek.as_ref().unwrap().kind, PeekKind::NetworkLost), "volume tidak menggantikan peek penting");
        let mut later = t0;
        later.ms += 4000;
        c.tick(&later);
        assert!(c.peek.is_none());
        assert_eq!(c.view, View::Idle);
    }

    #[test]
    fn charging_peek_only_on_change() {
        let t0 = clock(10, 0, 0);
        let mut c = Core::new(Persisted::default(), &t0);
        c.data.water.total_ml = 5000;
        c.system(SystemEvent::Power { on_ac: false, percent: Some(70) }, &t0);
        assert!(c.peek.is_none());
        c.system(SystemEvent::Power { on_ac: true, percent: Some(70) }, &t0);
        assert!(matches!(c.peek.as_ref().unwrap().kind, PeekKind::Charging { percent: 70 }));
    }

    #[test]
    fn deadline_is_soonest() {
        let t0 = clock(9, 0, 0);
        let mut c = Core::new(Persisted::default(), &t0);
        c.data.water.total_ml = 5000;
        c.apply(Command::FocusStartPause, &t0);
        assert_eq!(c.next_deadline_ms(&t0), Some(t0.ms + 25 * 60_000));
    }

    /// Core dengan Personality aktif, tanpa pengingat air.
    fn eyes_core(now: &Clock) -> Core {
        let mut c = Core::new(Persisted::default(), now);
        c.data.settings.personality = true;
        c.data.water.total_ml = 5000;
        c
    }

    #[test]
    fn eye_mood_priority() {
        let t0 = clock(10, 0, 0);
        let mut c = eyes_core(&t0);
        assert_eq!(c.eye_mood(&t0), EyeMood::Normal);
        c.system(SystemEvent::Media { playing: true, title: Some("Lagu".into()) }, &t0);
        assert_eq!(c.eye_mood(&t0), EyeMood::Music);
        c.apply(Command::FocusStartPause, &t0);
        assert_eq!(c.eye_mood(&t0), EyeMood::Focus, "fokus mengalahkan musik");
        c.system(SystemEvent::Power { on_ac: false, percent: Some(15) }, &t0);
        assert_eq!(c.eye_mood(&t0), EyeMood::LowBattery, "baterai lemah mengalahkan fokus");
        c.eye_sleeping = true;
        assert_eq!(c.eye_mood(&t0), EyeMood::Sleeping);
        c.eye_greet_until = Some(t0.ms + 1);
        assert_eq!(c.eye_mood(&t0), EyeMood::Greeting);
        c.eye_happy_until = Some(t0.ms + 1);
        assert_eq!(c.eye_mood(&t0), EyeMood::Happy);
    }

    #[test]
    fn low_battery_mood_only_when_discharging() {
        let t0 = clock(10, 0, 0);
        let mut c = eyes_core(&t0);
        c.system(SystemEvent::Power { on_ac: true, percent: Some(10) }, &t0);
        assert_eq!(c.eye_mood(&t0), EyeMood::Normal);
        c.system(SystemEvent::Power { on_ac: false, percent: Some(21) }, &t0);
        assert_eq!(c.eye_mood(&t0), EyeMood::Normal);
        c.system(SystemEvent::Power { on_ac: false, percent: Some(20) }, &t0);
        assert_eq!(c.eye_mood(&t0), EyeMood::LowBattery);
    }

    #[test]
    fn happy_triggers_and_expires_on_time() {
        let t0 = clock(10, 0, 0);
        let mut c = eyes_core(&t0);
        c.apply(Command::SubmitTodo("Kirim laporan".into()), &t0);
        let id = c.data.todos[0].id;
        c.apply(Command::ToggleTodo(id), &t0);
        assert_eq!(c.eye_mood(&t0), EyeMood::Happy);
        assert_eq!(c.next_deadline_ms(&t0), Some(t0.ms + EYE_HAPPY_MS));
        let mut t1 = t0;
        t1.ms += EYE_HAPPY_MS - 1;
        c.tick(&t1);
        assert_eq!(c.eye_mood(&t1), EyeMood::Happy);
        t1.ms += 1;
        let fx = c.tick(&t1);
        assert!(fx.redraw);
        assert_eq!(c.eye_mood(&t1), EyeMood::Normal);
        // membatalkan centang tidak membuat senang
        c.apply(Command::ToggleTodo(id), &t1);
        assert_eq!(c.eye_mood(&t1), EyeMood::Normal);
    }

    #[test]
    fn happy_on_water_goal_and_focus_done() {
        let t0 = clock(10, 0, 0);
        let mut c = eyes_core(&t0);
        c.data.water.total_ml = 1800;
        c.apply(Command::AddWater(250), &t0);
        assert_eq!(c.eye_mood(&t0), EyeMood::Happy);
        c.eye_happy_until = None;
        c.apply(Command::AddWater(250), &t0);
        assert_eq!(c.eye_mood(&t0), EyeMood::Normal, "sudah di atas target: tidak senang lagi");
        c.apply(Command::FocusStartPause, &t0);
        let end = clock(10, 25, 0);
        c.tick(&end);
        assert_eq!(c.eye_mood(&end), EyeMood::Happy);
    }

    #[test]
    fn sleep_deadline_rescheduled_by_new_input() {
        let t0 = clock(10, 0, 0);
        let mut c = eyes_core(&t0);
        c.set_last_input(t0.ms);
        assert_eq!(c.next_deadline_ms(&t0), Some(t0.ms + 5 * 60_000));
        // Saat deadline tiba ternyata ada input di 10:02 -> jadwal ulang, belum tidur.
        let t5 = clock(10, 5, 0);
        c.set_last_input(clock(10, 2, 0).ms);
        c.tick(&t5);
        assert!(!c.eye_sleeping);
        assert_eq!(c.next_deadline_ms(&t5), Some(clock(10, 7, 0).ms));
        let t7 = clock(10, 7, 0);
        assert!(c.tick(&t7).redraw);
        assert!(c.eye_sleeping);
        assert_eq!(c.eye_mood(&t7), EyeMood::Sleeping);
        assert_eq!(c.next_deadline_ms(&t7), Some(clock(0, 0, 0).ms + 24 * 3600_000), "tidur: hanya pergantian hari");
    }

    #[test]
    fn sleep_follows_setting_and_needs_personality_idle() {
        let t0 = clock(10, 0, 0);
        let mut c = eyes_core(&t0);
        c.data.settings.eye_sleep_minutes = 2;
        c.set_last_input(t0.ms);
        assert_eq!(c.next_deadline_ms(&t0), Some(clock(10, 2, 0).ms));
        c.apply(Command::Open(Widget::Music), &t0);
        c.tick(&clock(10, 30, 0));
        assert!(!c.eye_sleeping, "notch terbuka: mata tidak tidur");
        c.apply(Command::Close, &t0);
        c.data.settings.personality = false;
        c.tick(&clock(10, 30, 0));
        assert!(!c.eye_sleeping, "tanpa Personality tidak ada mata");
        assert_eq!(c.next_deadline_ms(&t0), Some(clock(0, 0, 0).ms + 24 * 3600_000));
    }

    #[test]
    fn night_sleeps_after_one_minute() {
        let t0 = clock(23, 30, 0);
        let mut c = eyes_core(&t0);
        c.set_last_input(t0.ms);
        assert_eq!(c.next_deadline_ms(&t0), Some(clock(23, 31, 0).ms));
        let early = clock(4, 0, 0);
        let mut c = eyes_core(&early);
        c.set_last_input(early.ms);
        assert_eq!(c.next_deadline_ms(&early), Some(clock(4, 1, 0).ms));
        // Sebelum jam malam: tidur tepat saat 23:00 tiba, bukan 5 menit setelah input.
        let t1 = clock(22, 58, 0);
        let mut c = eyes_core(&t1);
        c.set_last_input(t1.ms);
        assert_eq!(c.next_deadline_ms(&t1), Some(clock(23, 0, 0).ms));
        let t2 = clock(23, 0, 0);
        c.tick(&t2);
        assert!(c.eye_sleeping);
        // Pagi (05:00 ke atas) kembali ke 5 menit.
        let t3 = clock(5, 0, 0);
        let mut c = eyes_core(&t3);
        c.set_last_input(t3.ms);
        assert_eq!(c.next_deadline_ms(&t3), Some(clock(5, 5, 0).ms));
    }

    #[test]
    fn sleeping_wakes_into_greeting() {
        let t0 = clock(10, 0, 0);
        let mut c = eyes_core(&t0);
        c.set_last_input(t0.ms);
        let t5 = clock(10, 5, 0);
        c.tick(&t5);
        assert!(c.eye_sleeping);
        let t6 = clock(10, 6, 0);
        c.system(SystemEvent::UserActivity, &t6);
        assert!(!c.eye_sleeping);
        assert_eq!(c.eye_mood(&t6), EyeMood::Greeting);
        assert_eq!(c.next_deadline_ms(&t6), Some(t6.ms + EYE_GREET_MS));
        let mut t7 = t6;
        t7.ms += EYE_GREET_MS;
        c.tick(&t7);
        assert_eq!(c.eye_mood(&t7), EyeMood::Normal);
        assert_eq!(c.next_deadline_ms(&t7), Some(t6.ms + 5 * 60_000), "jadwal tidur dari input terakhir");
        // Aktivitas saat tidak tidur tidak memunculkan sapaan.
        c.system(SystemEvent::UserActivity, &t7);
        assert_eq!(c.eye_mood(&t7), EyeMood::Normal);
    }

    #[test]
    fn opening_notch_wakes_eyes() {
        let t0 = clock(10, 0, 0);
        let mut c = eyes_core(&t0);
        c.eye_sleeping = true;
        c.apply(Command::Toggle, &t0);
        assert!(!c.eye_sleeping);
        assert!(c.is_open());
    }

    /// Core tanpa pengingat air agar peek lain mudah diuji.
    fn quiet_core(now: &Clock) -> Core {
        let mut c = Core::new(Persisted::default(), now);
        c.data.water.total_ml = 5000;
        c
    }

    #[test]
    fn prayer_off_by_default_and_tab_follows_setting() {
        let t0 = clock(9, 0, 0);
        let mut c = quiet_core(&t0);
        assert!(c.prayer.is_none());
        assert!(!c.tabs().contains(&Widget::Prayer));
        c.apply(Command::Open(Widget::Prayer), &t0);
        assert_eq!(c.view, View::Expanded(Widget::Music), "tab nonaktif tidak bisa dibuka");
        c.apply(Command::SetPrayerCity("Jakarta".into()), &t0);
        assert!(c.tabs().contains(&Widget::Prayer));
        assert_eq!(c.prayer.as_ref().unwrap().date, "2026-10-20");
        c.apply(Command::SetPrayerCity("Kota Fiktif".into()), &t0);
        assert!(c.prayer.is_none());
        assert_eq!(c.data.settings.prayer_city, "");
    }

    #[test]
    fn prayer_reminder_at_time_with_adzan() {
        let t0 = clock(9, 0, 0);
        let mut c = quiet_core(&t0);
        c.apply(Command::SetPrayerCity("Jakarta".into()), &t0);
        c.apply(Command::Close, &t0);
        let dz = c.prayer.as_ref().unwrap().min[3];
        assert_eq!(c.next_prayer(&t0), Some(3));
        let at = clock(dz / 60, dz % 60, 0);
        assert_eq!(c.next_deadline_ms(&t0), Some(at.ms));
        let fx = c.tick(&at);
        assert_eq!(fx.sound, Some(Sound::Adzan));
        assert!(matches!(&c.peek.as_ref().unwrap().kind, PeekKind::Prayer { name, .. } if name == "Dzuhur"));
        assert_eq!(c.next_prayer(&at), Some(4));
        // tidak diulang
        c.peek = None;
        assert_eq!(c.tick(&at).sound, None);
        // tanpa suara jika dimatikan
        c.apply(Command::SetAdzanSound(false), &at);
        let asr = c.prayer.as_ref().unwrap().min[4];
        assert_eq!(c.tick(&clock(asr / 60, asr % 60, 0)).sound, None);
        assert!(matches!(&c.peek.as_ref().unwrap().kind, PeekKind::Prayer { name, .. } if name == "Ashar"));
    }

    #[test]
    fn prayer_skips_times_already_past_and_only_latest_after_gap() {
        let t0 = clock(13, 0, 0);
        let mut c = quiet_core(&t0);
        c.apply(Command::SetPrayerCity("Jakarta".into()), &t0);
        c.apply(Command::Close, &t0);
        c.tick(&t0);
        assert!(c.peek.is_none(), "Subuh/Dzuhur sudah lewat saat kota dipilih");
        // komputer tidur sampai 19:30: hanya Isya yang diingatkan
        c.tick(&clock(19, 30, 0));
        assert!(matches!(&c.peek.as_ref().unwrap().kind, PeekKind::Prayer { name, .. } if name == "Isya"));
        // hari berikutnya dihitung ulang
        let mut tomorrow = clock(0, 0, 5);
        tomorrow.day = 21;
        c.tick(&tomorrow);
        assert_eq!(c.prayer.as_ref().unwrap().date, "2026-10-21");
        assert_eq!(c.next_prayer(&tomorrow), Some(1));
    }

    #[test]
    fn clipboard_history_dedupes_caps_and_pins() {
        let t0 = clock(9, 0, 0);
        let mut c = quiet_core(&t0);
        c.system(SystemEvent::Clipboard { text: "satu".into() }, &t0);
        c.system(SystemEvent::Clipboard { text: "dua".into() }, &t0);
        c.system(SystemEvent::Clipboard { text: "   ".into() }, &t0);
        c.system(SystemEvent::Clipboard { text: "satu".into() }, &t0);
        let texts: Vec<_> = c.clips.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["satu", "dua"]);
        let dua = c.clips[1].id;
        c.apply(Command::PinClip(dua), &t0);
        for i in 0..30 {
            c.system(SystemEvent::Clipboard { text: format!("teks {i}") }, &t0);
        }
        assert_eq!(c.clips.iter().filter(|c| !c.pinned).count(), MAX_CLIPS);
        assert!(c.clips.iter().any(|c| c.id == dua), "yang disematkan tidak terbuang");
        assert_eq!(c.visible_clips()[0].id, dua, "disematkan tampil paling atas");
        assert_eq!(c.visible_clips().len(), MAX_CLIPS_VISIBLE);
        c.apply(Command::ClearClips, &t0);
        assert_eq!(c.clips.len(), 1);
        c.apply(Command::DeleteClip(dua), &t0);
        assert!(c.clips.is_empty());
    }

    #[test]
    fn copy_clip_writes_clipboard_and_shows_copied() {
        let t0 = clock(9, 0, 0);
        let mut c = quiet_core(&t0);
        c.system(SystemEvent::Clipboard { text: "alamat".into() }, &t0);
        c.system(SystemEvent::Clipboard { text: "nomor".into() }, &t0);
        let id = c.clips[1].id;
        let fx = c.apply(Command::CopyClip(id), &t0);
        assert_eq!(fx.clipboard.as_deref(), Some("alamat"));
        assert_eq!(c.clips[0].id, id, "pindah ke atas");
        assert_eq!(c.clip_copied.map(|x| x.0), Some(id));
        let mut t1 = t0;
        t1.ms += CLIP_COPIED_MS;
        c.tick(&t1);
        assert!(c.clip_copied.is_none());
    }

    #[test]
    fn clipboard_disabled_ignores_and_clears() {
        let t0 = clock(9, 0, 0);
        let mut c = quiet_core(&t0);
        c.system(SystemEvent::Clipboard { text: "rahasia".into() }, &t0);
        c.apply(Command::Open(Widget::Clipboard), &t0);
        c.apply(Command::SetClipboardHistory(false), &t0);
        assert!(c.clips.is_empty());
        assert_eq!(c.view, View::Expanded(Widget::Music));
        assert!(!c.tabs().contains(&Widget::Clipboard));
        c.system(SystemEvent::Clipboard { text: "lagi".into() }, &t0);
        assert!(c.clips.is_empty());
    }

    #[test]
    fn camera_peek_only_on_start_after_initial_state() {
        let t0 = clock(9, 0, 0);
        let mut c = quiet_core(&t0);
        c.system(SystemEvent::Privacy { camera: None, mic: Some("Discord".into()) }, &t0);
        assert!(c.peek.is_none(), "status awal hanya dicatat");
        assert_eq!(c.mic.as_deref(), Some("Discord"));
        c.system(SystemEvent::Privacy { camera: Some("Zoom".into()), mic: Some("Discord".into()) }, &t0);
        assert!(matches!(&c.peek.as_ref().unwrap().kind, PeekKind::CamMic { camera: true, app } if app == "Zoom"));
        c.peek = None;
        c.system(SystemEvent::Privacy { camera: Some("Zoom".into()), mic: Some("Discord".into()) }, &t0);
        assert!(c.peek.is_none(), "tidak berubah: tanpa peek");
        c.system(SystemEvent::Privacy { camera: None, mic: None }, &t0);
        assert!(c.camera.is_none() && c.mic.is_none());
    }

    #[test]
    fn progress_updates_then_done() {
        let t0 = clock(9, 0, 0);
        let mut c = quiet_core(&t0);
        let run = |p| SystemEvent::Progress { label: "cargo build".into(), state: ProgressState::Running(p) };
        c.system(run(Some(10)), &t0);
        c.system(run(Some(150)), &t0);
        assert!(matches!(&c.peek.as_ref().unwrap().kind, PeekKind::Progress { percent: Some(100), .. }));
        let fx = c.system(SystemEvent::Progress { label: "cargo build".into(), state: ProgressState::Failed }, &t0);
        assert_eq!(fx.sound, Some(Sound::Soft));
        assert!(matches!(&c.peek.as_ref().unwrap().kind, PeekKind::ProgressDone { ok: false, .. }));
        // progress basi hilang sendiri setelah 30 detik tanpa kabar
        let mut c = quiet_core(&t0);
        c.system(run(None), &t0);
        let mut t1 = t0;
        t1.ms += 30_000;
        c.tick(&t1);
        assert!(c.peek.is_none());
    }

    #[test]
    fn current_location_overrides_city_and_city_clears_it() {
        let t0 = clock(9, 0, 0);
        let mut c = quiet_core(&t0);
        let fx = c.system(SystemEvent::Location(Some(GeoPoint { lat: -7.29, lon: 112.73, tz: 7.0 })), &t0);
        assert!(fx.save);
        assert_eq!(c.data.settings.prayer_city, "Surabaya");
        assert_eq!(c.prayer_place(), "sekitar Surabaya");
        assert!(c.tabs().contains(&Widget::Prayer));
        assert!(matches!(&c.peek.as_ref().unwrap().kind, PeekKind::Location { place: Some(p) } if p == "sekitar Surabaya"));
        let at_loc = c.prayer.clone().unwrap();
        let city = prayer::compute(prayer::city("Surabaya").unwrap(), 2026, 10, 20);
        assert!(at_loc.min.iter().zip(city.min).all(|(a, b)| a.abs_diff(b) <= 1), "titik dekat kota: jadwal hampir sama");
        // memilih kota dari menu menghapus lokasi
        c.apply(Command::SetPrayerCity("Jakarta".into()), &t0);
        assert!(c.data.settings.prayer_location.is_none());
        assert_eq!(c.prayer_place(), "Jakarta");
        // gagal: hanya kabar, pengaturan tidak berubah
        c.peek = None;
        let fx = c.system(SystemEvent::Location(None), &t0);
        assert!(!fx.save);
        assert!(matches!(&c.peek.as_ref().unwrap().kind, PeekKind::Location { place: None }));
        assert_eq!(c.data.settings.prayer_city, "Jakarta");
    }

    #[test]
    fn progress_state_parses_cli_words() {
        assert_eq!(ProgressState::parse("start"), Some(ProgressState::Running(None)));
        assert_eq!(ProgressState::parse("42"), Some(ProgressState::Running(Some(42))));
        assert_eq!(ProgressState::parse("75%"), Some(ProgressState::Running(Some(75))));
        assert_eq!(ProgressState::parse("250"), Some(ProgressState::Running(Some(100))));
        assert_eq!(ProgressState::parse("Selesai"), Some(ProgressState::Done));
        assert_eq!(ProgressState::parse("gagal"), Some(ProgressState::Failed));
        assert_eq!(ProgressState::parse("apa"), None);
    }

    #[test]
    fn download_peek_click_reveals_file() {
        let t0 = clock(9, 0, 0);
        let mut c = quiet_core(&t0);
        c.system(SystemEvent::DownloadDone { name: "laporan.pdf".into() }, &t0);
        let fx = c.apply(Command::ActivatePeek, &t0);
        assert_eq!(fx.reveal.as_deref(), Some("laporan.pdf"));
        assert_eq!(c.view, View::Idle);
    }

    #[test]
    fn short_date() {
        assert_eq!(clock(9, 0, 0).short_date_id(), "Sel · 20 Okt");
    }
    #[test]
    fn scroll_volume_and_setting() {
        let t0 = clock(10, 0, 0);
        let mut c = quiet_core(&t0);
        assert_eq!(c.scroll(ScrollZone::Notch, 120, false, false, &t0), Some(GestureAction::Volume(2)));
        assert_eq!(c.scroll(ScrollZone::Notch, -360, false, false, &t0), Some(GestureAction::Volume(-6)));
        c.apply(Command::SetScrollGestures(false), &t0);
        assert_eq!(c.scroll(ScrollZone::Notch, 120, false, false, &t0), None);
        assert_eq!(c.middle_click(), None);
    }

    #[test]
    fn scroll_track_needs_media_and_peek_gets_new_title() {
        let t0 = clock(10, 0, 0);
        let mut c = quiet_core(&t0);
        assert_eq!(c.scroll(ScrollZone::Notch, 120, true, false, &t0), None, "tanpa sesi media");
        assert_eq!(c.middle_click(), Some(GestureAction::ToggleMute));
        c.system(SystemEvent::Media { playing: true, title: Some("Lama".into()) }, &t0);
        assert_eq!(c.middle_click(), Some(GestureAction::PlayPause));
        let mut t1 = t0;
        t1.ms += 1000;
        assert_eq!(c.scroll(ScrollZone::Notch, 120, true, false, &t1), Some(GestureAction::Track { next: true }));
        let peek = |c: &Core| c.peek.as_ref().map(|p| p.kind.clone());
        assert_eq!(peek(&c), Some(PeekKind::Track { title: String::new(), next: true }));
        // snapshot lama datang lagi: judul belum diisi
        c.system(SystemEvent::Media { playing: true, title: Some("Lama".into()) }, &t1);
        assert_eq!(peek(&c), Some(PeekKind::Track { title: String::new(), next: true }));
        c.system(SystemEvent::Media { playing: true, title: Some("Baru".into()) }, &t1);
        assert_eq!(peek(&c), Some(PeekKind::Track { title: "Baru".into(), next: true }));
        assert_eq!(c.peek.as_ref().unwrap().until_ms, t1.ms + 1600);
        // volume (prioritas sama) menggantikan peek lagu
        c.system(SystemEvent::Volume { percent: 30, muted: false }, &t1);
        assert!(matches!(peek(&c), Some(PeekKind::Volume { .. })));
    }

    #[test]
    fn scroll_switches_tabs_with_bounds() {
        let t0 = clock(10, 0, 0);
        let mut c = quiet_core(&t0);
        assert_eq!(c.scroll(ScrollZone::Tabs, -120, false, false, &t0), None, "tab hanya saat notch terbuka");
        c.apply(Command::Open(Widget::Music), &t0);
        let mut t = t0;
        t.ms += 1000;
        assert_eq!(c.scroll(ScrollZone::Tabs, 120, false, false, &t), None, "sudah tab pertama");
        t.ms += 1000;
        assert_eq!(c.scroll(ScrollZone::Tabs, -120, false, false, &t), Some(GestureAction::Tab));
        assert_eq!(c.view, View::Expanded(Widget::Focus));
        t.ms += 100;
        assert_eq!(c.scroll(ScrollZone::Tabs, -120, false, false, &t), None, "cooldown");
        assert_eq!(c.view, View::Expanded(Widget::Focus));
    }
    fn pin(t: &str) -> PinInput {
        PinInput { kind: PinKind::File, target: t.into(), name: String::new() }
    }

    #[test]
    fn pins_limit_and_dedupe() {
        let t0 = clock(10, 0, 0);
        let mut c = quiet_core(&t0);
        let fx = c.apply(Command::AddPins(vec![pin(r"C:\a.txt"), pin(r"c:\A.TXT"), pin(r"C:\b")]), &t0);
        assert!(fx.save);
        assert_eq!(c.data.pins.len(), 2, "duplikat (beda huruf besar/kecil) diabaikan");
        assert_eq!(c.data.pins[0].name, r"C:\a.txt", "nama kosong memakai target");
        assert_eq!(c.view, View::Expanded(Widget::Launcher));
        assert!(c.pin_notice.is_none());
        let more: Vec<PinInput> = (0..10).map(|i| pin(&format!(r"C:\f{i}"))).collect();
        c.apply(Command::AddPins(more), &t0);
        assert_eq!(c.data.pins.len(), MAX_PINS);
        assert_eq!(c.data.pins[7].target, r"C:\f5");
        let (msg, until) = c.pin_notice.clone().unwrap();
        assert_eq!(msg, "Maksimal 8. Lepas satu sematan dulu.");
        assert_eq!(c.next_deadline_ms(&t0), Some(until));
        let mut t1 = t0;
        t1.ms = until;
        assert!(c.tick(&t1).redraw);
        assert!(c.pin_notice.is_none());
        // duplikat saat penuh tidak memunculkan pesan
        c.apply(Command::AddPins(vec![pin(r"C:\B")]), &t1);
        assert!(c.pin_notice.is_none());
    }

    #[test]
    fn pins_move_remove_and_launch() {
        let t0 = clock(10, 0, 0);
        let mut c = quiet_core(&t0);
        c.apply(Command::AddPins(vec![pin("a"), pin("b"), pin("c"), pin("d")]), &t0);
        let ids: Vec<u64> = c.data.pins.iter().map(|p| p.id).collect();
        let order = |c: &Core| c.data.pins.iter().map(|p| p.target.as_str()).collect::<String>();
        c.apply(Command::MovePin { id: ids[0], to: 2 }, &t0);
        assert_eq!(order(&c), "bcad");
        c.apply(Command::MovePin { id: ids[3], to: 0 }, &t0);
        assert_eq!(order(&c), "dbca");
        c.apply(Command::MovePin { id: ids[1], to: 99 }, &t0);
        assert_eq!(order(&c), "dcab", "indeks di luar batas = paling akhir");
        c.apply(Command::AskRemovePin(Some(ids[2])), &t0);
        assert_eq!(c.pin_confirm, Some(ids[2]));
        c.apply(Command::RemovePin(ids[2]), &t0);
        assert_eq!(order(&c), "dab");
        assert_eq!(c.pin_confirm, None);
        let fx = c.apply(Command::LaunchPin(ids[3]), &t0);
        assert_eq!(fx.launch.map(|p| p.target), Some("d".to_string()));
        assert!(!c.is_open(), "notch menutup setelah membuka target");
        c.system(SystemEvent::LaunchFailed { name: "d".into() }, &t0);
        assert_eq!(c.peek.as_ref().map(|p| p.kind.clone()), Some(PeekKind::LaunchFailed { name: "d".into() }));
    }

    #[test]
    fn drag_opens_launcher_and_cancel_restores() {
        let t0 = clock(10, 0, 0);
        let mut c = quiet_core(&t0);
        c.apply(Command::DragEnter, &t0);
        assert!(c.dragging);
        assert_eq!(c.view, View::Expanded(Widget::Launcher));
        c.apply(Command::DragLeave, &t0);
        assert!(!c.dragging);
        assert_eq!(c.view, View::Idle, "dibuka oleh drag: ditutup lagi");
        // notch sudah terbuka: drag batal tidak menutup
        c.apply(Command::Open(Widget::Water), &t0);
        c.apply(Command::DragEnter, &t0);
        assert_eq!(c.view, View::Expanded(Widget::Launcher));
        c.apply(Command::DragLeave, &t0);
        assert!(c.is_open());
        // drop: tetap terbuka
        c.apply(Command::Close, &t0);
        c.apply(Command::DragEnter, &t0);
        c.apply(Command::AddPins(vec![pin("x")]), &t0);
        assert!(!c.dragging);
        assert_eq!(c.view, View::Expanded(Widget::Launcher));
        assert!(c.tabs().contains(&Widget::Launcher));
    }
}
