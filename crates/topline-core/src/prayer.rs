//! Jadwal sholat offline: perhitungan astronomis posisi matahari (algoritme
//! praytimes.org) dengan parameter Kementerian Agama RI:
//! Subuh -20°, Isya -18°, Ashar madzhab Syafi'i (bayangan = 1× tinggi benda),
//! Maghrib saat matahari terbenam, ihtiyath +2 menit (Terbit -2 menit),
//! Imsak = Subuh - 10 menit.

/// Kota bawaan: (nama, wilayah, lintang, bujur, zona waktu jam).
#[derive(Clone, Copy, Debug)]
pub struct City {
    pub name: &'static str,
    pub region: &'static str,
    pub lat: f64,
    pub lon: f64,
    pub tz: f64,
}

const fn c(name: &'static str, region: &'static str, lat: f64, lon: f64, tz: f64) -> City {
    City { name, region, lat, lon, tz }
}

pub const REGIONS: [&str; 6] = ["Sumatra", "Jawa", "Bali & Nusa Tenggara", "Kalimantan", "Sulawesi", "Maluku & Papua"];

pub const CITIES: &[City] = &[
    c("Banda Aceh", "Sumatra", 5.5483, 95.3238, 7.0),
    c("Medan", "Sumatra", 3.5952, 98.6722, 7.0),
    c("Padang", "Sumatra", -0.9471, 100.4172, 7.0),
    c("Pekanbaru", "Sumatra", 0.5071, 101.4478, 7.0),
    c("Batam", "Sumatra", 1.1301, 104.0529, 7.0),
    c("Jambi", "Sumatra", -1.6101, 103.6131, 7.0),
    c("Palembang", "Sumatra", -2.9761, 104.7754, 7.0),
    c("Bengkulu", "Sumatra", -3.7928, 102.2608, 7.0),
    c("Pangkalpinang", "Sumatra", -2.1316, 106.1169, 7.0),
    c("Bandar Lampung", "Sumatra", -5.3971, 105.2668, 7.0),
    c("Serang", "Jawa", -6.1200, 106.1503, 7.0),
    c("Tangerang", "Jawa", -6.1783, 106.6319, 7.0),
    c("Jakarta", "Jawa", -6.2088, 106.8456, 7.0),
    c("Bekasi", "Jawa", -6.2383, 106.9756, 7.0),
    c("Depok", "Jawa", -6.4025, 106.7942, 7.0),
    c("Bogor", "Jawa", -6.5971, 106.8060, 7.0),
    c("Bandung", "Jawa", -6.9175, 107.6191, 7.0),
    c("Cirebon", "Jawa", -6.7320, 108.5523, 7.0),
    c("Semarang", "Jawa", -6.9667, 110.4167, 7.0),
    c("Yogyakarta", "Jawa", -7.7956, 110.3695, 7.0),
    c("Surakarta", "Jawa", -7.5755, 110.8243, 7.0),
    c("Surabaya", "Jawa", -7.2575, 112.7521, 7.0),
    c("Malang", "Jawa", -7.9666, 112.6326, 7.0),
    c("Denpasar", "Bali & Nusa Tenggara", -8.6705, 115.2126, 8.0),
    c("Mataram", "Bali & Nusa Tenggara", -8.5833, 116.1167, 8.0),
    c("Kupang", "Bali & Nusa Tenggara", -10.1772, 123.6070, 8.0),
    c("Pontianak", "Kalimantan", -0.0263, 109.3425, 7.0),
    c("Palangka Raya", "Kalimantan", -2.2161, 113.9135, 7.0),
    c("Banjarmasin", "Kalimantan", -3.3194, 114.5908, 8.0),
    c("Balikpapan", "Kalimantan", -1.2379, 116.8529, 8.0),
    c("Samarinda", "Kalimantan", -0.5022, 117.1536, 8.0),
    c("Tanjung Selor", "Kalimantan", 2.8375, 117.3653, 8.0),
    c("Makassar", "Sulawesi", -5.1477, 119.4327, 8.0),
    c("Mamuju", "Sulawesi", -2.6748, 118.8885, 8.0),
    c("Palu", "Sulawesi", -0.8917, 119.8707, 8.0),
    c("Kendari", "Sulawesi", -3.9985, 122.5129, 8.0),
    c("Gorontalo", "Sulawesi", 0.5435, 123.0568, 8.0),
    c("Manado", "Sulawesi", 1.4748, 124.8421, 8.0),
    c("Ternate", "Maluku & Papua", 0.7893, 127.3842, 9.0),
    c("Ambon", "Maluku & Papua", -3.6954, 128.1814, 9.0),
    c("Sorong", "Maluku & Papua", -0.8762, 131.2558, 9.0),
    c("Manokwari", "Maluku & Papua", -0.8615, 134.0620, 9.0),
    c("Jayapura", "Maluku & Papua", -2.5337, 140.7181, 9.0),
    c("Merauke", "Maluku & Papua", -8.4932, 140.4018, 9.0),
];

pub fn city(name: &str) -> Option<&'static City> {
    CITIES.iter().find(|c| c.name == name)
}

/// Kota bawaan terdekat dari sebuah titik (jarak sudut di bola).
pub fn nearest(lat: f64, lon: f64) -> &'static City {
    let dist = |c: &City| {
        let x = dcos(lat) * dcos(c.lat) * dcos(lon - c.lon) + dsin(lat) * dsin(c.lat);
        -x // cos jarak sudut terbesar = terdekat
    };
    CITIES.iter().min_by(|a, b| dist(a).total_cmp(&dist(b))).unwrap_or(&CITIES[0])
}

/// Urutan waktu dalam [`PrayerTimes::min`].
pub const NAMES: [&str; 7] = ["Imsak", "Subuh", "Terbit", "Dzuhur", "Ashar", "Maghrib", "Isya"];
const NAMES_EN: [&str; 7] = ["Imsak", "Fajr", "Sunrise", "Dhuhr", "Asr", "Maghrib", "Isha"];

/// Nama waktu sholat ke-`i` dalam bahasa UI.
pub fn name(i: usize, lang: crate::Lang) -> &'static str {
    match lang {
        crate::Lang::Id => NAMES[i],
        crate::Lang::En => NAMES_EN[i],
    }
}
/// Indeks waktu yang memunculkan pengingat (bukan Imsak/Terbit).
pub const NOTIFY: [usize; 5] = [1, 3, 4, 5, 6];

/// Jadwal satu hari, menit sejak 00:00 waktu lokal kota.
#[derive(Clone, Debug, PartialEq)]
pub struct PrayerTimes {
    pub date: String,
    pub min: [u32; 7],
}

impl PrayerTimes {
    pub fn label(&self, i: usize) -> String {
        format!("{:02}:{:02}", self.min[i] / 60, self.min[i] % 60)
    }
}

fn dsin(d: f64) -> f64 {
    d.to_radians().sin()
}
fn dcos(d: f64) -> f64 {
    d.to_radians().cos()
}
fn fix(a: f64, b: f64) -> f64 {
    let a = a - b * (a / b).floor();
    if a < 0.0 {
        a + b
    } else {
        a
    }
}

fn julian(year: i32, month: u32, day: u32) -> f64 {
    let (mut y, mut m) = (year as f64, month as f64);
    if m <= 2.0 {
        y -= 1.0;
        m += 12.0;
    }
    let a = (y / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    (365.25 * (y + 4716.0)).floor() + (30.6001 * (m + 1.0)).floor() + day as f64 + b - 1524.5
}

/// (deklinasi matahari, persamaan waktu dalam jam)
fn sun(jd: f64) -> (f64, f64) {
    let d = jd - 2451545.0;
    let g = fix(357.529 + 0.98560028 * d, 360.0);
    let q = fix(280.459 + 0.98564736 * d, 360.0);
    let l = fix(q + 1.915 * dsin(g) + 0.020 * dsin(2.0 * g), 360.0);
    let e = 23.439 - 0.00000036 * d;
    let ra = fix((dcos(e) * dsin(l)).atan2(dcos(l)).to_degrees() / 15.0, 24.0);
    let decl = (dsin(e) * dsin(l)).asin().to_degrees();
    let eqt = q / 15.0 - ra;
    (decl, eqt)
}

struct Calc {
    jd: f64,
    lat: f64,
}

impl Calc {
    fn mid_day(&self, t: f64) -> f64 {
        fix(12.0 - sun(self.jd + t).1, 24.0)
    }
    /// Waktu saat matahari berada `angle` derajat di bawah ufuk.
    fn angle_time(&self, angle: f64, t: f64, ccw: bool) -> f64 {
        let decl = sun(self.jd + t).0;
        let noon = self.mid_day(t);
        let x = ((-dsin(angle) - dsin(decl) * dsin(self.lat)) / (dcos(decl) * dcos(self.lat))).clamp(-1.0, 1.0);
        let h = x.acos().to_degrees() / 15.0;
        if ccw {
            noon - h
        } else {
            noon + h
        }
    }
    fn asr_time(&self, factor: f64, t: f64) -> f64 {
        let decl = sun(self.jd + t).0;
        let angle = -(1.0 / (factor + dsin(self.lat - decl).abs() / dcos(self.lat - decl))).atan().to_degrees();
        self.angle_time(angle, t, false)
    }
}

/// Hitung jadwal untuk kota pada tanggal tertentu.
pub fn compute(city: &City, year: i32, month: u32, day: u32) -> PrayerTimes {
    let jd = julian(year, month, day) - city.lon / (15.0 * 24.0);
    let calc = Calc { jd, lat: city.lat };
    // Dua iterasi: tebakan awal per waktu, lalu diperhalus.
    let mut t = [5.0, 6.0, 12.0, 13.0, 18.0, 18.0]; // subuh, terbit, dzuhur, ashar, maghrib, isya
    for _ in 0..2 {
        let p = t.map(|h| h / 24.0);
        t = [
            calc.angle_time(20.0, p[0], true),
            calc.angle_time(0.833, p[1], true),
            calc.mid_day(p[2]),
            calc.asr_time(1.0, p[3]),
            calc.angle_time(0.833, p[4], false),
            calc.angle_time(18.0, p[5], false),
        ];
    }
    let adj = city.tz - city.lon / 15.0;
    let to_min = |h: f64, ihtiyath: f64| -> u32 { (fix((h + adj) * 60.0 + ihtiyath, 1440.0)).ceil() as u32 % 1440 };
    let subuh = to_min(t[0], 2.0);
    PrayerTimes {
        date: format!("{:04}-{:02}-{:02}", year, month, day),
        min: [
            subuh.saturating_sub(10),
            subuh,
            to_min(t[1], -2.0),
            to_min(t[2], 2.0),
            to_min(t[3], 2.0),
            to_min(t[4], 2.0),
            to_min(t[5], 2.0),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hm(s: &str) -> u32 {
        let (h, m) = s.split_once(':').unwrap();
        h.parse::<u32>().unwrap() * 60 + m.parse::<u32>().unwrap()
    }

    fn near(got: &PrayerTimes, want: [&str; 7], tol: u32) {
        for i in 0..7 {
            let w = hm(want[i]);
            let d = got.min[i].abs_diff(w);
            assert!(d <= tol, "{} {}: dapat {}, harap {} (selisih {} mnt)", got.date, NAMES[i], got.label(i), want[i], d);
        }
    }

    #[test]
    fn jakarta_matches_kemenag_schedule() {
        // Jadwal Kemenag DKI Jakarta 20-10-2024 (liputan6.com/jadwal-sholat): 04:03, 04:13,
        // -, 11:41, 14:49, 17:49, 18:59. Terbit tidak dicantumkan; nilai di bawah dari
        // perhitungan matahari terbit (sekitar 05:30) dikurangi ihtiyath.
        let t = compute(city("Jakarta").unwrap(), 2024, 10, 20);
        near(&t, ["04:03", "04:13", "05:28", "11:41", "14:49", "17:49", "18:59"], 2);
    }

    #[test]
    fn surabaya_matches_kemenag_schedule() {
        // Kemenag Kota Surabaya 20-10-2024 (liputan6.com/jadwal-sholat): 03:39, 03:49, -,
        // 11:17, 14:24, 17:26, 18:36. Terbit dari perhitungan.
        let t = compute(city("Surabaya").unwrap(), 2024, 10, 20);
        let terbit = t.label(2);
        near(&t, ["03:39", "03:49", &terbit, "11:17", "14:24", "17:26", "18:36"], 2);
    }

    #[test]
    fn nearest_city_picks_closest() {
        assert_eq!(nearest(-7.29, 112.73).name, "Surabaya");
        assert_eq!(nearest(-6.18, 106.63).name, "Tangerang");
        assert_eq!(nearest(-8.65, 115.20).name, "Denpasar");
        assert_eq!(nearest(-2.53, 140.70).name, "Jayapura");
    }

    #[test]
    fn times_are_ordered_everywhere() {
        for c in CITIES {
            for (m, d) in [(1, 15), (4, 1), (6, 21), (10, 20), (12, 21)] {
                let t = compute(c, 2026, m, d);
                for i in 1..7 {
                    assert!(t.min[i] > t.min[i - 1], "{} {}-{}: {:?}", c.name, m, d, t.min);
                }
            }
        }
    }

    #[test]
    fn regions_cover_all_cities() {
        for c in CITIES {
            assert!(REGIONS.contains(&c.region), "{}", c.name);
        }
    }
}
