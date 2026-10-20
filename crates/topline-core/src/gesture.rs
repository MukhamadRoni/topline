//! Gestur scroll di notch: akumulasi delta roda mouse/touchpad, ambang, dan cooldown.
//! Murni (tanpa Windows); lapisan aplikasi hanya meneruskan WM_MOUSEWHEEL/WM_MOUSEHWHEEL.

/// Satu takik roda mouse (WHEEL_DELTA).
pub const WHEEL_DELTA: i32 = 120;
/// Perubahan volume per takik.
pub const VOLUME_STEP_PERCENT: i32 = 2;
/// Jeda setelah ganti lagu, agar satu gesekan touchpad tidak melompati banyak lagu.
pub const TRACK_COOLDOWN_MS: i64 = 450;
/// Jeda setelah pindah tab.
pub const TAB_COOLDOWN_MS: i64 = 250;
/// Hover tidak membuka notch selama ini sejak scroll terakhir.
pub const HOVER_HOLD_MS: i64 = 1500;
/// Sisa delta kecil dibuang jika tidak ada scroll selama ini.
const IDLE_RESET_MS: i64 = 1000;

/// Penjumlah delta dengan ambang [`WHEEL_DELTA`]. Tanpa cooldown, setiap kelipatan ambang
/// menjadi satu langkah. Dengan cooldown, paling banyak satu langkah, lalu semua delta
/// dibuang sampai tidak ada scroll selama `cooldown_ms` (gesekan yang masih berlanjut,
/// termasuk inersia touchpad, memperpanjang jeda).
#[derive(Clone, Debug, Default)]
pub struct ScrollAccumulator {
    acc: i32,
    cooldown_ms: i64,
    last_ms: Option<i64>,
    cooling: bool,
}

impl ScrollAccumulator {
    pub fn new(cooldown_ms: i64) -> Self {
        Self { cooldown_ms, ..Default::default() }
    }

    /// Tambah delta; kembalikan jumlah langkah bertanda (positif = naik/kanan).
    pub fn feed(&mut self, delta: i32, now_ms: i64) -> i32 {
        let gap = self.last_ms.map_or(i64::MAX, |t| now_ms - t);
        self.last_ms = Some(now_ms);
        if self.cooling {
            if gap < self.cooldown_ms {
                return 0;
            }
            self.cooling = false;
        }
        // Jeda lama atau arah berbalik: buang sisa delta lama.
        if gap > IDLE_RESET_MS || (self.acc > 0 && delta < 0) || (self.acc < 0 && delta > 0) {
            self.acc = 0;
        }
        self.acc += delta;
        let steps = self.acc / WHEEL_DELTA;
        if steps == 0 {
            return 0;
        }
        if self.cooldown_ms > 0 {
            self.acc = 0;
            self.cooling = true;
            steps.signum()
        } else {
            self.acc -= steps * WHEEL_DELTA;
            steps
        }
    }
}

/// Bagian notch di bawah kursor saat scroll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollZone {
    /// Notch idle atau peek.
    Notch,
    /// Bar tab saat notch terbuka.
    Tabs,
    /// Isi widget Musik.
    Music,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAxis {
    Vertical,
    Horizontal,
}

impl ScrollAxis {
    /// WM_MOUSEHWHEEL, atau Shift + roda vertikal, dihitung horizontal.
    pub fn of(horizontal_msg: bool, shift: bool) -> Self {
        if horizontal_msg || shift {
            ScrollAxis::Horizontal
        } else {
            ScrollAxis::Vertical
        }
    }
}

/// Aksi yang harus dijalankan lapisan aplikasi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureAction {
    /// Ubah volume master sebanyak ini (persen, bertanda).
    Volume(i32),
    Track { next: bool },
    /// Widget sudah dipindah di core; cukup gambar ulang.
    Tab,
    PlayPause,
    ToggleMute,
}

/// Akumulator untuk tiap jenis gestur dan waktu scroll terakhir.
#[derive(Clone, Debug)]
pub struct ScrollGestures {
    volume: ScrollAccumulator,
    track: ScrollAccumulator,
    tab: ScrollAccumulator,
    last_ms: Option<i64>,
}

/// Hasil mentah [`ScrollGestures::feed`] sebelum dicocokkan dengan state core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollStep {
    Volume(i32),
    Track { next: bool },
    /// +1 = tab berikutnya, -1 = sebelumnya.
    Tab(i32),
}

impl Default for ScrollGestures {
    fn default() -> Self {
        Self {
            volume: ScrollAccumulator::new(0),
            track: ScrollAccumulator::new(TRACK_COOLDOWN_MS),
            tab: ScrollAccumulator::new(TAB_COOLDOWN_MS),
            last_ms: None,
        }
    }
}

impl ScrollGestures {
    /// `delta` dari HIWORD(wParam): roda vertikal positif = ke atas, horizontal positif = ke kanan.
    pub fn feed(&mut self, zone: ScrollZone, delta: i32, horizontal_msg: bool, shift: bool, now_ms: i64) -> Option<ScrollStep> {
        self.last_ms = Some(now_ms);
        let axis = ScrollAxis::of(horizontal_msg, shift);
        // Shift + roda ke atas = ke kiri, seperti di aplikasi lain.
        let h = if horizontal_msg { delta } else { -delta };
        let nz = |s: i32| (s != 0).then_some(s);
        match (zone, axis) {
            (ScrollZone::Tabs, ScrollAxis::Vertical) => nz(self.tab.feed(-delta, now_ms)).map(ScrollStep::Tab),
            (ScrollZone::Tabs, ScrollAxis::Horizontal) => nz(self.tab.feed(h, now_ms)).map(ScrollStep::Tab),
            (_, ScrollAxis::Vertical) => nz(self.volume.feed(delta, now_ms)).map(|s| ScrollStep::Volume(s * VOLUME_STEP_PERCENT)),
            (_, ScrollAxis::Horizontal) => nz(self.track.feed(h, now_ms)).map(|s| ScrollStep::Track { next: s > 0 }),
        }
    }

    /// Hover-open ditahan selama [`HOVER_HOLD_MS`] sejak scroll terakhir.
    pub fn hover_blocked(&self, now_ms: i64) -> bool {
        self.last_ms.map_or(false, |t| now_ms - t < HOVER_HOLD_MS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_deltas_accumulate() {
        let mut a = ScrollAccumulator::new(0);
        assert_eq!(a.feed(40, 0), 0);
        assert_eq!(a.feed(40, 10), 0);
        assert_eq!(a.feed(40, 20), 1);
        assert_eq!(a.feed(100, 30), 0, "sisa 0 + 100 belum mencapai ambang");
        assert_eq!(a.feed(260, 40), 3, "beberapa takik sekaligus");
    }

    #[test]
    fn direction_positive_and_negative() {
        let mut a = ScrollAccumulator::new(0);
        assert_eq!(a.feed(120, 0), 1);
        assert_eq!(a.feed(-120, 10), -1);
        assert_eq!(a.feed(-240, 20), -2);
        // berbalik arah membuang sisa arah lama
        assert_eq!(a.feed(-100, 30), 0);
        assert_eq!(a.feed(100, 40), 0);
        assert_eq!(a.feed(20, 50), 1);
    }

    #[test]
    fn stale_remainder_dropped() {
        let mut a = ScrollAccumulator::new(0);
        assert_eq!(a.feed(100, 0), 0);
        assert_eq!(a.feed(100, 5000), 0, "sisa lama dibuang");
    }

    #[test]
    fn cooldown_blocks_double_step() {
        let mut a = ScrollAccumulator::new(TRACK_COOLDOWN_MS);
        assert_eq!(a.feed(480, 0), 1, "dengan cooldown paling banyak satu langkah");
        // satu gesekan touchpad: event terus berdatangan, termasuk inersia
        for t in (10..1200).step_by(10) {
            assert_eq!(a.feed(60, t), 0, "t={t}");
        }
        // setelah tenang lebih dari cooldown, langkah berikutnya boleh
        assert_eq!(a.feed(120, 1190 + TRACK_COOLDOWN_MS + 1), 1);
        assert_eq!(a.feed(-120, 1190 + TRACK_COOLDOWN_MS + 50), 0);
    }

    #[test]
    fn shift_vertical_counts_as_horizontal() {
        assert_eq!(ScrollAxis::of(false, true), ScrollAxis::Horizontal);
        assert_eq!(ScrollAxis::of(true, false), ScrollAxis::Horizontal);
        assert_eq!(ScrollAxis::of(false, false), ScrollAxis::Vertical);
        let mut g = ScrollGestures::default();
        // Shift + roda ke bawah = ke kanan = lagu berikutnya
        assert_eq!(g.feed(ScrollZone::Notch, -120, false, true, 0), Some(ScrollStep::Track { next: true }));
        let mut g = ScrollGestures::default();
        assert_eq!(g.feed(ScrollZone::Notch, 120, false, true, 0), Some(ScrollStep::Track { next: false }));
        let mut g = ScrollGestures::default();
        assert_eq!(g.feed(ScrollZone::Notch, 120, true, false, 0), Some(ScrollStep::Track { next: true }));
    }

    #[test]
    fn zones_route_to_actions() {
        let mut g = ScrollGestures::default();
        assert_eq!(g.feed(ScrollZone::Notch, 120, false, false, 0), Some(ScrollStep::Volume(2)));
        assert_eq!(g.feed(ScrollZone::Music, -240, false, false, 10), Some(ScrollStep::Volume(-4)));
        assert_eq!(g.feed(ScrollZone::Tabs, -120, false, false, 20), Some(ScrollStep::Tab(1)), "roda ke bawah = tab berikutnya");
        assert_eq!(g.feed(ScrollZone::Tabs, -120, false, false, 100), None, "cooldown tab");
        assert_eq!(g.feed(ScrollZone::Tabs, 120, false, false, 100 + TAB_COOLDOWN_MS + 1), Some(ScrollStep::Tab(-1)));
    }

    #[test]
    fn hover_held_after_scroll() {
        let mut g = ScrollGestures::default();
        assert!(!g.hover_blocked(0));
        g.feed(ScrollZone::Notch, 10, false, false, 1000);
        assert!(g.hover_blocked(1000 + HOVER_HOLD_MS - 1));
        assert!(!g.hover_blocked(1000 + HOVER_HOLD_MS));
    }
}
