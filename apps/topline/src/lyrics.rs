//! Pencari lirik di thread sendiri. Urutan: file `.lrc` pengguna di folder lirik → cache →
//! LRCLIB (HTTPS lewat WinHTTP; yang dikirim hanya judul, artis, dan durasi). Thread tidur di
//! `recv()` sampai ada lagu baru, jadi tidak ada beban saat idle. Hasil dikirim ke UI lewat
//! `PostMessageW(WM_APP_LYRICS)` dengan LPARAM = `Box<Reply>`.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::time::{Duration, SystemTime};

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Networking::WinHttp::*;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use topline_core::{Lyrics, LyricsResult, LyricsSource};
use topline_lyrics::{clean_artist, clean_title, pick_best, queries, Candidate};

use crate::util;
use crate::WM_APP_LYRICS;

const HOST: &str = "lrclib.net";
/// LRCLIB meminta User-Agent yang menyebut nama dan versi aplikasi.
const USER_AGENT: &str = concat!("Topline/", env!("CARGO_PKG_VERSION"), " (notch Windows 11)");
/// Hasil "tidak ditemukan" dicoba lagi setelah sekian lama.
const NOT_FOUND_TTL: Duration = Duration::from_secs(3 * 24 * 3600);

pub struct Query {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: i64,
    pub online: bool,
}

pub struct Reply {
    pub key: String,
    pub result: LyricsResult,
}

pub struct LyricsHandle {
    tx: Sender<Query>,
}

impl LyricsHandle {
    pub fn request(&self, q: Query) {
        let _ = self.tx.send(q);
    }
}

/// Folder lirik pengguna: taruh `Artis - Judul.lrc` (atau `Judul.lrc`) di sini.
pub fn dir() -> PathBuf {
    util::data_dir().join("lyrics")
}

fn cache_dir() -> PathBuf {
    dir().join("cache")
}

/// Jumlah lagu di cache lirik.
pub fn cache_count() -> usize {
    std::fs::read_dir(cache_dir()).map(|d| d.flatten().count()).unwrap_or(0)
}

pub fn clear_cache() {
    let _ = std::fs::remove_dir_all(cache_dir());
}

pub fn start(hwnd: HWND) -> LyricsHandle {
    let (tx, rx) = channel::<Query>();
    let raw = hwnd.0 as isize;
    std::thread::Builder::new()
        .name("topline-lyrics".into())
        .spawn(move || {
            while let Ok(mut q) = rx.recv() {
                // Lagu dilewati cepat-cepat: cukup cari yang terakhir.
                while let Ok(newer) = rx.try_recv() {
                    q = newer;
                }
                let result = find(&q);
                util::log(&format!("lyrics {:?}: {}", q.title, describe(&result)));
                let reply = Box::new(Reply { key: q.key, result });
                unsafe {
                    let _ = PostMessageW(HWND(raw as *mut _), WM_APP_LYRICS, WPARAM(0), LPARAM(Box::into_raw(reply) as isize));
                }
            }
        })
        .ok();
    LyricsHandle { tx }
}

fn describe(r: &LyricsResult) -> String {
    match r {
        LyricsResult::Found(l, s) => format!("{:?} {} baris, sinkron={}", s, l.lines.len(), l.synced),
        other => format!("{:?}", other),
    }
}

// ------------------------------------------------------------------ pencarian

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Nama file yang aman: huruf kecil, tanpa karakter terlarang.
fn file_key(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
        .collect::<String>()
        .to_lowercase()
        .trim()
        .to_string()
}

fn find(q: &Query) -> LyricsResult {
    if let Some(l) = find_local(q) {
        return LyricsResult::Found(l, LyricsSource::Local);
    }
    let base = cache_dir().join(format!("{:016x}", fnv(&q.key.to_lowercase())));
    if let Some(r) = read_cache(&base) {
        return r;
    }
    if !q.online {
        return LyricsResult::Offline;
    }
    match fetch_online(q) {
        Ok(Some(c)) => {
            let lyrics = c.lyrics();
            write_cache(&base, &c);
            LyricsResult::Found(lyrics, LyricsSource::Online)
        }
        Ok(None) => {
            write_none(&base);
            LyricsResult::NotFound
        }
        Err(()) => LyricsResult::Failed,
    }
}

/// `Artis - Judul.lrc` atau `Judul.lrc` di folder lirik (tanpa membedakan huruf besar/kecil).
fn find_local(q: &Query) -> Option<Lyrics> {
    let mut wanted: Vec<String> = queries(&q.title, &q.artist)
        .into_iter()
        .flat_map(|(t, a)| [format!("{} - {}", a, t), t])
        .chain([format!("{} - {}", clean_artist(&q.artist), clean_title(&q.title)), q.title.clone()])
        .map(|s| file_key(&s))
        .collect();
    wanted.dedup();
    for e in std::fs::read_dir(dir()).ok()?.flatten() {
        let p = e.path();
        let is_lrc = p.extension().map_or(false, |x| x.eq_ignore_ascii_case("lrc") || x.eq_ignore_ascii_case("txt"));
        let stem = p.file_stem().map(|s| file_key(&s.to_string_lossy())).unwrap_or_default();
        if is_lrc && wanted.contains(&stem) {
            let text = std::fs::read_to_string(&p).ok()?;
            return Some(Lyrics::parse_lrc(&text));
        }
    }
    None
}

fn read_cache(base: &std::path::Path) -> Option<LyricsResult> {
    let with = |ext: &str| base.with_extension(ext);
    if let Ok(t) = std::fs::read_to_string(with("lrc")) {
        return Some(LyricsResult::Found(Lyrics::parse_lrc(&t), LyricsSource::Online));
    }
    if let Ok(t) = std::fs::read_to_string(with("txt")) {
        return Some(LyricsResult::Found(Lyrics::plain(&t), LyricsSource::Online));
    }
    if with("inst").exists() {
        return Some(LyricsResult::Found(Lyrics::instrumental(), LyricsSource::Online));
    }
    let fresh = std::fs::metadata(with("none"))
        .and_then(|m| m.modified())
        .map_or(false, |t| SystemTime::now().duration_since(t).map_or(true, |age| age < NOT_FOUND_TTL));
    fresh.then_some(LyricsResult::NotFound)
}

fn write_cache(base: &std::path::Path, c: &Candidate) {
    let _ = std::fs::create_dir_all(cache_dir());
    let _ = match (&c.synced, &c.plain) {
        (Some(s), _) if !s.trim().is_empty() => std::fs::write(base.with_extension("lrc"), s),
        (_, Some(p)) if !p.trim().is_empty() => std::fs::write(base.with_extension("txt"), p),
        _ => std::fs::write(base.with_extension("inst"), ""),
    };
}

fn write_none(base: &std::path::Path) {
    let _ = std::fs::create_dir_all(cache_dir());
    let _ = std::fs::write(base.with_extension("none"), "");
}

/// Coba beberapa kueri (judul YouTube "Artis - Judul" dipecah). Err hanya jika semua
/// permintaan gagal di jaringan.
fn fetch_online(q: &Query) -> std::result::Result<Option<Candidate>, ()> {
    let mut any_ok = false;
    let qs = queries(&q.title, &q.artist);
    let mut paths: Vec<String> = qs
        .iter()
        .map(|(t, a)| {
            if a.is_empty() {
                format!("/api/search?track_name={}", encode(t))
            } else {
                format!("/api/search?track_name={}&artist_name={}", encode(t), encode(a))
            }
        })
        .collect();
    if let Some((t, a)) = qs.first() {
        paths.push(format!("/api/search?q={}", encode(&format!("{} {}", a, t).trim().to_string())));
    }
    for path in paths {
        let Ok(body) = http_get(&path) else { continue };
        any_ok = true;
        let cands = parse_candidates(&body);
        if let Some(i) = pick_best(&cands, q.duration_ms) {
            return Ok(Some(cands[i].clone()));
        }
    }
    if any_ok {
        Ok(None)
    } else {
        Err(())
    }
}

fn parse_candidates(body: &str) -> Vec<Candidate> {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(body) else { return Vec::new() };
    items
        .iter()
        .map(|v| Candidate {
            synced: v.get("syncedLyrics").and_then(|s| s.as_str()).map(String::from),
            plain: v.get("plainLyrics").and_then(|s| s.as_str()).map(String::from),
            duration: v.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0),
            instrumental: v.get("instrumental").and_then(|b| b.as_bool()).unwrap_or(false),
        })
        .collect()
}

/// Percent-encoding untuk query string (UTF-8).
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// Handle WinHTTP yang ditutup otomatis.
struct H(*mut core::ffi::c_void);

impl Drop for H {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

/// GET https://lrclib.net{path}; Ok hanya untuk status 200.
fn http_get(path: &str) -> std::result::Result<String, ()> {
    unsafe {
        let ua = util::wide(USER_AGENT);
        let session = H(WinHttpOpen(PCWSTR(ua.as_ptr()), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0));
        if session.0.is_null() {
            return Err(());
        }
        let _ = WinHttpSetTimeouts(session.0, 8000, 8000, 8000, 10_000);
        let host = util::wide(HOST);
        let conn = H(WinHttpConnect(session.0, PCWSTR(host.as_ptr()), INTERNET_DEFAULT_HTTPS_PORT, 0));
        if conn.0.is_null() {
            return Err(());
        }
        let p = util::wide(path);
        let req = H(WinHttpOpenRequest(conn.0, w!("GET"), PCWSTR(p.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE));
        if req.0.is_null() {
            return Err(());
        }
        WinHttpSendRequest(req.0, None, None, 0, 0, 0).map_err(|_| ())?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(|_| ())?;
        let mut status = 0u32;
        let mut len = 4u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut _),
            &mut len,
            std::ptr::null_mut(),
        )
        .map_err(|_| ())?;
        if status != 200 {
            util::log(&format!("lrclib {} -> HTTP {}", path, status));
            return Err(());
        }
        let mut body = Vec::new();
        loop {
            let mut avail = 0u32;
            WinHttpQueryDataAvailable(req.0, &mut avail).map_err(|_| ())?;
            if avail == 0 {
                break;
            }
            let start = body.len();
            body.resize(start + avail as usize, 0);
            let mut read = 0u32;
            WinHttpReadData(req.0, body[start..].as_mut_ptr() as *mut _, avail, &mut read).map_err(|_| ())?;
            body.truncate(start + read as usize);
            // batas aman: hasil pencarian lirik tidak sebesar ini
            if body.len() > 4 * 1024 * 1024 {
                return Err(());
            }
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }
}
