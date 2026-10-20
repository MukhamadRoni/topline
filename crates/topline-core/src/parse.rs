//! Memisahkan jam dari teks tugas, misalnya "Rapat tim 16:30" -> ("Rapat tim", 16:30).

/// Mengembalikan (teks tanpa jam, jam opsional). Format yang dikenali:
/// `16:30`, `16.30`, `9:05`, `pukul 7.15`, `jam 14:00`.
pub fn parse_task(input: &str) -> (String, Option<(u32, u32)>) {
    let trimmed = input.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n {
        if chars[i].is_ascii_digit() && (i == 0 || !chars[i - 1].is_ascii_alphanumeric()) {
            // 1-2 digit jam
            let mut j = i;
            while j < n && j - i < 2 && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j < n && (chars[j] == ':' || chars[j] == '.') {
                let sep = j;
                if chars.get(sep + 1).map_or(false, |c| c.is_ascii_digit())
                    && chars.get(sep + 2).map_or(false, |c| c.is_ascii_digit())
                    && !chars.get(sep + 3).map_or(false, |c| c.is_ascii_digit())
                {
                    let h: u32 = chars[i..sep].iter().collect::<String>().parse().unwrap_or(99);
                    let m: u32 = chars[sep + 1..sep + 3]
                        .iter()
                        .collect::<String>()
                        .parse()
                        .unwrap_or(99);
                    if h < 24 && m < 60 {
                        let before: String = chars[..i].iter().collect();
                        let after: String = chars[sep + 3..].iter().collect();
                        let mut text = format!("{} {}", strip_time_word(before.trim_end()), after.trim_start());
                        text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                        if text.is_empty() {
                            text = trimmed.to_string();
                        }
                        return (text, Some((h, m)));
                    }
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    (trimmed.split_whitespace().collect::<Vec<_>>().join(" "), None)
}

/// Menghapus kata "jam"/"pukul"/"at" di akhir teks sebelum angka jam.
fn strip_time_word(s: &str) -> String {
    let lower = s.to_lowercase();
    for w in ["pukul", "jam", "at", "@"] {
        if lower.ends_with(w) {
            let cut = s.len() - w.len();
            let prev_ok = cut == 0 || s[..cut].ends_with(' ');
            if prev_ok {
                return s[..cut].trim_end().to_string();
            }
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colon() {
        assert_eq!(parse_task("Rapat tim 16:30"), ("Rapat tim".into(), Some((16, 30))));
    }
    #[test]
    fn dot_and_word() {
        assert_eq!(parse_task("Minum obat pukul 7.15 pagi"), ("Minum obat pagi".into(), Some((7, 15))));
        assert_eq!(parse_task("Call jam 09:05"), ("Call".into(), Some((9, 5))));
    }
    #[test]
    fn no_time() {
        assert_eq!(parse_task("  Beli   kopi "), ("Beli kopi".into(), None));
        assert_eq!(parse_task("Versi 2.5"), ("Versi 2.5".into(), None));
        assert_eq!(parse_task("Rilis v0.30"), ("Rilis v0.30".into(), None));
    }
    #[test]
    fn invalid_time_ignored() {
        assert_eq!(parse_task("Skor 25:99"), ("Skor 25:99".into(), None));
    }
    #[test]
    fn only_time() {
        assert_eq!(parse_task("16:30"), ("16:30".into(), Some((16, 30))));
    }
}
