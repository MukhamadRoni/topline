//! Data yang disimpan ke disk dan jam lokal yang diberikan dari luar.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Waktu lokal saat ini. Diisi oleh aplikasi (dari `GetLocalTime`), sehingga
/// core tidak pernah membaca jam sistem sendiri dan mudah diuji.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    /// Milidetik monotonic/epoch, hanya dipakai untuk selisih waktu.
    pub ms: i64,
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 = Minggu ... 6 = Sabtu
    pub weekday: u32,
}

impl Clock {
    pub fn date_key(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
    pub fn minute_of_day(&self) -> u32 {
        self.hour * 60 + self.minute
    }
    pub fn hhmm(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
    /// Milidetik sampai pergantian menit berikutnya.
    pub fn ms_to_next_minute(&self) -> i64 {
        ((60 - self.second as i64) * 1000).max(1000)
    }
    /// Contoh: "Sel · 20 Okt"
    pub fn short_date_id(&self) -> String {
        const DAYS: [&str; 7] = ["Min", "Sen", "Sel", "Rab", "Kam", "Jum", "Sab"];
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des",
        ];
        format!(
            "{} · {} {}",
            DAYS[(self.weekday % 7) as usize],
            self.day,
            MONTHS[((self.month.max(1) - 1) % 12) as usize]
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Settings {
    /// Tampilkan sepasang mata (bukan jam) saat idle.
    pub personality: bool,
    /// Buka notch saat kursor diarahkan (hover).
    pub open_on_hover: bool,
    pub water_goal_ml: u32,
    pub water_interval_min: u32,
    pub water_start_hour: u32,
    pub water_end_hour: u32,
    pub focus_min: u32,
    pub break_min: u32,
    pub sessions_target: u32,
    /// Peek volume, Caps Lock, dsb.
    pub system_peeks: bool,
    /// Matikan efek kaca (latar navy pekat).
    pub solid: bool,
    /// Mata Personality tertidur setelah sekian menit tanpa input.
    pub eye_sleep_minutes: u32,
    /// Kota untuk jadwal sholat (nama dari `prayer::CITIES`); kosong = nonaktif.
    /// Saat `prayer_location` terisi, ini nama kota terdekat (untuk label).
    pub prayer_city: String,
    /// Koordinat dari "Lokasi saat ini"; jika ada, dipakai menggantikan koordinat kota.
    pub prayer_location: Option<GeoPoint>,
    /// Bunyikan suara saat waktu sholat tiba.
    pub adzan_sound: bool,
    /// Simpan riwayat teks yang disalin (hanya di memori).
    pub clipboard_history: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            personality: false,
            open_on_hover: true,
            water_goal_ml: 2000,
            water_interval_min: 60,
            water_start_hour: 8,
            water_end_hour: 20,
            focus_min: 25,
            break_min: 5,
            sessions_target: 4,
            system_peeks: true,
            solid: false,
            eye_sleep_minutes: 5,
            prayer_city: String::new(),
            prayer_location: None,
            adzan_sound: true,
            clipboard_history: true,
        }
    }
}

/// Titik lokasi: lintang, bujur, zona waktu (jam dari UTC).
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
    pub tz: f64,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Todo {
    pub id: u64,
    pub text: String,
    /// Jam jatuh tempo (jam, menit), opsional.
    pub time: Option<(u32, u32)>,
    /// Tanggal tugas dibuat, "YYYY-MM-DD".
    pub date: String,
    pub done: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub notified: bool,
}

impl Todo {
    pub fn time_label(&self) -> String {
        match self.time {
            Some((h, m)) => format!("{:02}:{:02}", h, m),
            None => String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct WaterDay {
    pub date: String,
    pub total_ml: u32,
    /// (menit dalam hari, ml)
    pub entries: Vec<(u32, u32)>,
    /// Menit dalam hari saat pengingat terakhir ditampilkan.
    pub last_reminder_min: Option<u32>,
}

impl WaterDay {
    /// Total per jam untuk grafik, jam `from..=to`.
    pub fn hourly(&self, from: u32, to: u32) -> Vec<u32> {
        let mut v = vec![0u32; (to.saturating_sub(from) + 1) as usize];
        for (min, ml) in &self.entries {
            let h = min / 60;
            if h >= from && h <= to {
                v[(h - from) as usize] += ml;
            }
        }
        v
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
pub struct Persisted {
    pub version: u32,
    pub settings: Settings,
    pub todos: Vec<Todo>,
    pub water: WaterDay,
    pub next_id: u64,
}

impl Default for Persisted {
    fn default() -> Self {
        Self {
            version: 1,
            settings: Settings::default(),
            todos: Vec::new(),
            water: WaterDay::default(),
            next_id: 1,
        }
    }
}
