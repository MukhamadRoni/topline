//! Model jendela Pengaturan: daftar opsi on/off dan angka beserta batas, langkah, dan
//! format tampilannya. Perubahan diterapkan lewat `Command::SetToggle` / `Command::StepNumber`
//! agar efek sampingnya (mis. durasi Pomodoro) diurus core.

use crate::i18n::Lang;
use crate::model::Settings;

/// Opsi on/off di [`Settings`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Toggle {
    OpenOnHover,
    Personality,
    SystemPeeks,
    Solid,
    ScrollGestures,
    ClipboardHistory,
    AdzanSound,
    LyricsOnline,
    LyricsCaption,
}

impl Toggle {
    pub fn get(self, s: &Settings) -> bool {
        match self {
            Toggle::OpenOnHover => s.open_on_hover,
            Toggle::Personality => s.personality,
            Toggle::SystemPeeks => s.system_peeks,
            Toggle::Solid => s.solid,
            Toggle::ScrollGestures => s.scroll_gestures,
            Toggle::ClipboardHistory => s.clipboard_history,
            Toggle::AdzanSound => s.adzan_sound,
            Toggle::LyricsOnline => s.lyrics_online,
            Toggle::LyricsCaption => s.lyrics_caption,
        }
    }

    pub(crate) fn set(self, s: &mut Settings, v: bool) {
        let f = match self {
            Toggle::OpenOnHover => &mut s.open_on_hover,
            Toggle::Personality => &mut s.personality,
            Toggle::SystemPeeks => &mut s.system_peeks,
            Toggle::Solid => &mut s.solid,
            Toggle::ScrollGestures => &mut s.scroll_gestures,
            Toggle::ClipboardHistory => &mut s.clipboard_history,
            Toggle::AdzanSound => &mut s.adzan_sound,
            Toggle::LyricsOnline => &mut s.lyrics_online,
            Toggle::LyricsCaption => &mut s.lyrics_caption,
        };
        *f = v;
    }
}

/// Opsi angka di [`Settings`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Number {
    EyeSleepMinutes,
    FocusMin,
    BreakMin,
    SessionsTarget,
    WaterGoalMl,
    WaterIntervalMin,
    WaterStartHour,
    WaterEndHour,
}

impl Number {
    /// (minimum, maksimum, langkah)
    pub fn range(self) -> (u32, u32, u32) {
        match self {
            Number::EyeSleepMinutes => (1, 60, 1),
            Number::FocusMin => (5, 120, 5),
            Number::BreakMin => (1, 30, 1),
            Number::SessionsTarget => (1, 12, 1),
            Number::WaterGoalMl => (500, 5000, 250),
            Number::WaterIntervalMin => (15, 240, 15),
            Number::WaterStartHour => (0, 22, 1),
            Number::WaterEndHour => (1, 23, 1),
        }
    }

    pub fn get(self, s: &Settings) -> u32 {
        match self {
            Number::EyeSleepMinutes => s.eye_sleep_minutes,
            Number::FocusMin => s.focus_min,
            Number::BreakMin => s.break_min,
            Number::SessionsTarget => s.sessions_target,
            Number::WaterGoalMl => s.water_goal_ml,
            Number::WaterIntervalMin => s.water_interval_min,
            Number::WaterStartHour => s.water_start_hour,
            Number::WaterEndHour => s.water_end_hour,
        }
    }

    fn slot(self, s: &mut Settings) -> &mut u32 {
        match self {
            Number::EyeSleepMinutes => &mut s.eye_sleep_minutes,
            Number::FocusMin => &mut s.focus_min,
            Number::BreakMin => &mut s.break_min,
            Number::SessionsTarget => &mut s.sessions_target,
            Number::WaterGoalMl => &mut s.water_goal_ml,
            Number::WaterIntervalMin => &mut s.water_interval_min,
            Number::WaterStartHour => &mut s.water_start_hour,
            Number::WaterEndHour => &mut s.water_end_hour,
        }
    }

    /// Geser sebanyak `steps` langkah dalam batas; jam mulai minum selalu sebelum jam selesai.
    /// Mengembalikan true jika nilainya berubah.
    pub(crate) fn step(self, s: &mut Settings, steps: i32) -> bool {
        let (lo, mut hi, st) = self.range();
        let mut lo = lo;
        match self {
            Number::WaterStartHour => hi = hi.min(s.water_end_hour.saturating_sub(1)),
            Number::WaterEndHour => lo = lo.max(s.water_start_hour + 1),
            _ => {}
        }
        let cur = self.get(s) as i64;
        // nilai lama di luar kelipatan langkah (data lama) dibulatkan ke kelipatan terdekat
        let base = (cur - lo as i64).div_euclid(st as i64) * st as i64 + lo as i64;
        let next = (base + steps as i64 * st as i64).clamp(lo as i64, hi.max(lo) as i64) as u32;
        let slot = self.slot(s);
        let changed = *slot != next;
        *slot = next;
        changed
    }

    /// Teks nilai untuk UI, mis. "25 mnt" / "25 min", "2,0 L" / "2.0 L", "08:00".
    pub fn format(self, v: u32, lang: Lang) -> String {
        match self {
            Number::EyeSleepMinutes | Number::FocusMin | Number::BreakMin | Number::WaterIntervalMin => {
                format!("{} {}", v, lang.t("mnt", "min"))
            }
            Number::SessionsTarget => format!("{} {}", v, lang.t("sesi", if v == 1 { "session" } else { "sessions" })),
            Number::WaterGoalMl => {
                let t = format!("{:.2}", v as f32 / 1000.0);
                let t = t.trim_end_matches('0');
                format!("{}{} L", lang.decimal(t), if t.ends_with('.') { "0" } else { "" })
            }
            Number::WaterStartHour | Number::WaterEndHour => format!("{:02}:00", v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_step_within_bounds() {
        let mut s = Settings::default();
        assert!(Number::FocusMin.step(&mut s, 1));
        assert_eq!(s.focus_min, 30);
        assert!(Number::FocusMin.step(&mut s, -100));
        assert_eq!(s.focus_min, 5);
        assert!(!Number::FocusMin.step(&mut s, -1), "sudah minimum");
        s.water_goal_ml = 2100; // bukan kelipatan 250 (data lama)
        Number::WaterGoalMl.step(&mut s, 1);
        assert_eq!(s.water_goal_ml, 2250);
        Number::WaterGoalMl.step(&mut s, 100);
        assert_eq!(s.water_goal_ml, 5000);
    }

    #[test]
    fn water_hours_keep_order() {
        let mut s = Settings::default(); // 08..20
        Number::WaterStartHour.step(&mut s, 20);
        assert_eq!(s.water_start_hour, 19, "mulai < selesai");
        Number::WaterEndHour.step(&mut s, -20);
        assert_eq!(s.water_end_hour, 20, "selesai > mulai");
        Number::WaterStartHour.step(&mut s, -30);
        Number::WaterEndHour.step(&mut s, -30);
        assert_eq!((s.water_start_hour, s.water_end_hour), (0, 1));
    }

    #[test]
    fn formats_values() {
        assert_eq!(Number::WaterGoalMl.format(2000, Lang::Id), "2,0 L");
        assert_eq!(Number::WaterGoalMl.format(2250, Lang::Id), "2,25 L");
        assert_eq!(Number::WaterGoalMl.format(2500, Lang::En), "2.5 L");
        assert_eq!(Number::WaterStartHour.format(8, Lang::En), "08:00");
        assert_eq!(Number::FocusMin.format(25, Lang::Id), "25 mnt");
        assert_eq!(Number::FocusMin.format(25, Lang::En), "25 min");
        assert_eq!(Number::SessionsTarget.format(4, Lang::Id), "4 sesi");
        assert_eq!(Number::SessionsTarget.format(1, Lang::En), "1 session");
    }

    #[test]
    fn toggles_roundtrip() {
        let mut s = Settings::default();
        for t in [Toggle::LyricsOnline, Toggle::Solid, Toggle::ScrollGestures] {
            let v = t.get(&s);
            t.set(&mut s, !v);
            assert_eq!(t.get(&s), !v);
        }
    }
}
