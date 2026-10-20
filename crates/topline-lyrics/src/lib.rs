//! Lirik untuk Topline, tanpa dependensi Windows dan tanpa jaringan:
//! parser `.lrc`, pemilihan baris aktif dari posisi lagu, pembersihan judul
//! (YouTube/browser) menjadi kueri pencarian, dan pemilihan hasil LRCLIB terbaik.

/// Satu baris lirik. `ms` = waktu mulai (0 untuk lirik tidak tersinkron).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub ms: i64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    /// Punya stempel waktu per baris.
    pub synced: bool,
    /// Lagu instrumental (tanpa lirik).
    pub instrumental: bool,
}

/// `[mm:ss]`, `[mm:ss.xx]`, `[mm:ss.xxx]`, atau `[mm:ss:xx]` → milidetik.
fn parse_stamp(s: &str) -> Option<i64> {
    let (m, rest) = s.split_once(':')?;
    let m: i64 = m.trim().parse().ok()?;
    let (sec, frac) = match rest.find(['.', ':']) {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    let sec: i64 = sec.trim().parse().ok()?;
    if !(0..60).contains(&sec) || m < 0 {
        return None;
    }
    let frac_ms = if frac.is_empty() {
        0
    } else {
        let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).take(3).collect();
        if digits.is_empty() {
            return None;
        }
        let v: i64 = digits.parse().ok()?;
        v * 10i64.pow(3 - digits.len() as u32)
    };
    Some(m * 60_000 + sec * 1000 + frac_ms)
}

impl Lyrics {
    /// Parse `.lrc`. Baris dengan beberapa stempel (`[00:12][00:40]teks`) diulang, tag
    /// metadata (`[ar:..]`) diabaikan, `[offset:+/-ms]` diterapkan. Tanpa stempel sama sekali
    /// → dianggap lirik biasa (tidak tersinkron).
    pub fn parse_lrc(src: &str) -> Lyrics {
        let mut offset = 0i64;
        let mut lines = Vec::new();
        for raw in src.lines() {
            let mut rest = raw.trim();
            let mut stamps = Vec::new();
            while let Some(stripped) = rest.strip_prefix('[') {
                let Some(end) = stripped.find(']') else { break };
                let tag = &stripped[..end];
                if let Some(ms) = parse_stamp(tag) {
                    stamps.push(ms);
                } else if let Some(v) = tag.strip_prefix("offset:") {
                    offset = v.trim().parse().unwrap_or(0);
                }
                rest = stripped[end + 1..].trim_start();
            }
            let text = rest.trim().to_string();
            for ms in stamps {
                lines.push(Line { ms, text: text.clone() });
            }
        }
        if lines.is_empty() {
            return Lyrics::plain(src);
        }
        // offset positif = lirik muncul lebih awal
        for l in &mut lines {
            l.ms = (l.ms - offset).max(0);
        }
        lines.sort_by_key(|l| l.ms);
        Lyrics { lines, synced: true, instrumental: false }
    }

    /// Lirik biasa tanpa waktu; baris kosong di awal/akhir dibuang.
    pub fn plain(src: &str) -> Lyrics {
        let mut lines: Vec<Line> = src.lines().map(|t| Line { ms: 0, text: t.trim().to_string() }).collect();
        while lines.first().map_or(false, |l| l.text.is_empty()) {
            lines.remove(0);
        }
        while lines.last().map_or(false, |l| l.text.is_empty()) {
            lines.pop();
        }
        Lyrics { lines, synced: false, instrumental: false }
    }

    pub fn instrumental() -> Lyrics {
        Lyrics { lines: Vec::new(), synced: false, instrumental: true }
    }

    /// Indeks baris yang sedang dinyanyikan pada posisi `pos_ms` (None sebelum baris pertama).
    pub fn active(&self, pos_ms: i64) -> Option<usize> {
        if !self.synced {
            return None;
        }
        let n = self.lines.partition_point(|l| l.ms <= pos_ms);
        n.checked_sub(1)
    }

    /// Posisi (ms) saat baris aktif berikutnya berganti, untuk satu timer deadline.
    pub fn next_change(&self, pos_ms: i64) -> Option<i64> {
        if !self.synced {
            return None;
        }
        self.lines.iter().map(|l| l.ms).find(|&ms| ms > pos_ms)
    }

    /// Teks untuk caption: baris aktif, atau "♪" saat jeda/sebelum baris pertama.
    pub fn caption(&self, pos_ms: i64) -> Option<String> {
        if !self.synced || self.lines.is_empty() {
            return None;
        }
        let t = self.active(pos_ms).map(|i| self.lines[i].text.as_str()).unwrap_or("");
        Some(if t.is_empty() { "♪".to_string() } else { t.to_string() })
    }
}

// ------------------------------------------------------------------ kueri pencarian

/// Buang segmen dalam kurung ("(Official Music Video)", "[Lyrics]", "(feat. X)"),
/// akhiran " | ...", dan "feat./ft. ..." di akhir judul.
pub fn clean_title(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' | '[' | '{' | '【' => depth += 1,
            ')' | ']' | '}' | '】' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let mut t = out.split(" | ").next().unwrap_or("").to_string();
    let lower = t.to_lowercase();
    for key in [" feat. ", " feat ", " ft. ", " ft ", " featuring "] {
        if let Some(i) = lower.find(key) {
            // "A feat. B - Judul": hanya buang jika tidak memotong pemisah artis-judul
            if !t[i..].contains(" - ") {
                t.truncate(i);
                break;
            }
        }
    }
    t.replace(['"', '“', '”'], "").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Nama kanal YouTube sering bukan nama artis: "X - Topic", "XVEVO", "X Official".
pub fn clean_artist(s: &str) -> String {
    let mut a = clean_title(s);
    for suffix in [" - Topic", "VEVO", " Official", " official"] {
        if let Some(stripped) = a.strip_suffix(suffix) {
            a = stripped.trim().to_string();
        }
    }
    a
}

/// Pasangan (judul, artis) untuk dicoba berurutan. Judul "Artis - Judul" (umum di YouTube)
/// dipecah dulu; lalu judul apa adanya dengan artis dari pemutar.
pub fn queries(title: &str, artist: &str) -> Vec<(String, String)> {
    let t = clean_title(title);
    let a = clean_artist(artist);
    let mut v = Vec::new();
    if let Some((left, right)) = t.split_once(" - ") {
        let artist = clean_title(&left.replace(" x ", " & "));
        let artist = clean_artist(&artist);
        v.push((right.trim().to_string(), artist));
    }
    if !t.is_empty() {
        v.push((t, a));
    }
    v.retain(|(t, _)| !t.is_empty());
    v.dedup();
    v
}

/// Satu hasil pencarian LRCLIB.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Candidate {
    pub synced: Option<String>,
    pub plain: Option<String>,
    /// Detik.
    pub duration: f64,
    pub instrumental: bool,
}

/// Pilih hasil terbaik: yang punya lirik tersinkron dan durasinya cocok (±15 s jika durasi
/// lagu diketahui). `duration_ms` = 0 jika tidak diketahui (sering terjadi di browser).
pub fn pick_best(cands: &[Candidate], duration_ms: i64) -> Option<usize> {
    let known = duration_ms > 0;
    let mut best: Option<(i32, usize)> = None;
    for (i, c) in cands.iter().enumerate() {
        let has_synced = c.synced.as_ref().map_or(false, |s| !s.trim().is_empty());
        let has_plain = c.plain.as_ref().map_or(false, |s| !s.trim().is_empty());
        if !has_synced && !has_plain && !c.instrumental {
            continue;
        }
        let diff = (c.duration * 1000.0 - duration_ms as f64).abs();
        if known && diff > 15_000.0 {
            continue;
        }
        let mut score = if has_synced { 4 } else if has_plain { 2 } else { 1 };
        if known {
            score += if diff <= 3000.0 { 3 } else { 1 };
        }
        if best.map_or(true, |(s, _)| score > s) {
            best = Some((score, i));
        }
    }
    best.map(|(_, i)| i)
}

impl Candidate {
    pub fn lyrics(&self) -> Lyrics {
        match (&self.synced, &self.plain) {
            (Some(s), _) if !s.trim().is_empty() => Lyrics::parse_lrc(s),
            (_, Some(p)) if !p.trim().is_empty() => Lyrics::plain(p),
            _ => Lyrics::instrumental(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc_with_tags_offset_and_repeats() {
        let src = "[ar:Penyanyi]\n[ti:Judul]\n[00:01.50]Satu\n[00:05.123][00:20]Ulang\n\n[00:10:5]Dua\nsampah tanpa waktu";
        let l = Lyrics::parse_lrc(src);
        assert!(l.synced);
        let got: Vec<(i64, &str)> = l.lines.iter().map(|x| (x.ms, x.text.as_str())).collect();
        assert_eq!(got, vec![(1500, "Satu"), (5123, "Ulang"), (10500, "Dua"), (20000, "Ulang")]);
        let o = Lyrics::parse_lrc("[offset:500]\n[00:02.00]A");
        assert_eq!(o.lines[0].ms, 1500);
    }

    #[test]
    fn without_stamps_is_plain() {
        let l = Lyrics::parse_lrc("\nbaris satu\nbaris dua\n\n");
        assert!(!l.synced);
        assert_eq!(l.lines.len(), 2);
        assert_eq!(l.active(5000), None);
        assert_eq!(l.caption(5000), None);
    }

    #[test]
    fn active_line_and_next_change() {
        let l = Lyrics::parse_lrc("[00:02.00]A\n[00:04.00]\n[00:06.00]B");
        assert_eq!(l.active(0), None);
        assert_eq!(l.caption(0).as_deref(), Some("♪"));
        assert_eq!(l.next_change(0), Some(2000));
        assert_eq!(l.active(2000), Some(0));
        assert_eq!(l.caption(3000).as_deref(), Some("A"));
        assert_eq!(l.caption(4500).as_deref(), Some("♪"), "baris kosong = jeda");
        assert_eq!(l.next_change(4500), Some(6000));
        assert_eq!(l.active(99_000), Some(2));
        assert_eq!(l.next_change(99_000), None);
    }

    #[test]
    fn cleans_youtube_titles() {
        assert_eq!(clean_title("eńau feat. Ari Lesmana - Sesi Potret (Official Music Video)"), "eńau feat. Ari Lesmana - Sesi Potret");
        assert_eq!(clean_title("Judul [Lyrics] | Kanal"), "Judul");
        assert_eq!(clean_title("Lagu feat. Orang Lain"), "Lagu");
        assert_eq!(clean_artist("Hindia - Topic"), "Hindia");
        assert_eq!(clean_artist("TulusVEVO"), "Tulus");
        let q = queries("eńau feat. Ari Lesmana - Sesi Potret (Official Music Video)", "eńau");
        assert_eq!(q[0], ("Sesi Potret".to_string(), "eńau".to_string()));
        assert_eq!(q[1].0, "eńau feat. Ari Lesmana - Sesi Potret");
        let q = queries("Sesi Potret", "eńau");
        assert_eq!(q, vec![("Sesi Potret".to_string(), "eńau".to_string())]);
        assert!(queries("(Official Video)", "x").is_empty());
    }

    #[test]
    fn picks_synced_with_matching_duration() {
        let c = |s: Option<&str>, p: Option<&str>, d: f64| Candidate { synced: s.map(Into::into), plain: p.map(Into::into), duration: d, instrumental: false };
        let cands = vec![
            c(None, Some("teks"), 200.0),
            c(Some("[00:01]a"), None, 260.0),
            c(Some("[00:01]b"), None, 201.0),
            c(None, None, 200.0),
        ];
        assert_eq!(pick_best(&cands, 200_000), Some(2), "tersinkron dan durasi cocok");
        assert_eq!(pick_best(&cands, 0), Some(1), "durasi tak diketahui: tersinkron pertama");
        assert_eq!(pick_best(&cands[..1], 400_000), None, "durasi terlalu jauh");
        assert_eq!(pick_best(&[], 0), None);
        assert_eq!(cands[2].lyrics().lines[0].text, "b");
        assert!(Candidate { instrumental: true, ..Default::default() }.lyrics().instrumental);
    }
}
