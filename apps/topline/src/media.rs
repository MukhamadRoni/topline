//! Pemutar media dari semua aplikasi lewat
//! `GlobalSystemMediaTransportControlsSessionManager` (Spotify, browser, Media Player, ...).
//!
//! Berjalan di thread sendiri (MTA). Tidak ada polling: thread tidur sampai Windows
//! mengirim event perubahan, lalu mengirim snapshot ke UI lewat `PostMessage`.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::Foundation::TypedEventHandler;
use windows::Media::Control::*;
use windows::Storage::Streams::DataReader;
use windows::Win32::Foundation::*;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::WM_APP_MEDIA;

#[derive(Clone, Debug, Default)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub app: String,
    pub playing: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    /// Epoch ms saat posisi terakhir dilaporkan.
    pub updated_ms: i64,
    pub thumb: Option<Arc<Vec<u8>>>,
    pub thumb_key: String,
}

impl MediaInfo {
    pub fn position_now(&self, now_epoch_ms: i64) -> i64 {
        let mut p = self.position_ms;
        if self.playing && self.updated_ms > 0 {
            p += (now_epoch_ms - self.updated_ms).max(0);
        }
        if self.duration_ms > 0 {
            p = p.min(self.duration_ms);
        }
        p.max(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaCmd {
    PlayPause,
    Next,
    Prev,
}

#[derive(Clone)]
pub struct MediaHandle {
    pub info: Arc<Mutex<Option<MediaInfo>>>,
    session: Arc<Mutex<Option<GlobalSystemMediaTransportControlsSession>>>,
    poke: Sender<()>,
}

impl MediaHandle {
    pub fn snapshot(&self) -> Option<MediaInfo> {
        self.info.lock().ok().and_then(|g| g.clone())
    }

    pub fn command(&self, cmd: MediaCmd) {
        let s = self.session.lock().ok().and_then(|g| g.clone());
        if let Some(s) = s {
            // Fire-and-forget: hasilnya datang lagi lewat event PlaybackInfoChanged.
            let _ = match cmd {
                MediaCmd::PlayPause => s.TryTogglePlayPauseAsync().map(|_| ()),
                MediaCmd::Next => s.TrySkipNextAsync().map(|_| ()),
                MediaCmd::Prev => s.TrySkipPreviousAsync().map(|_| ()),
            };
        }
        let _ = self.poke.send(());
    }
}

fn friendly_app(aumid: &str) -> String {
    let l = aumid.to_lowercase();
    let known = [
        ("spotify", "Spotify"),
        ("chrome", "Chrome"),
        ("msedge", "Edge"),
        ("firefox", "Firefox"),
        ("zunemusic", "Media Player"),
        ("zunevideo", "Film & TV"),
        ("vlc", "VLC"),
        ("brave", "Brave"),
        ("opera", "Opera"),
        ("youtube", "YouTube Music"),
        ("applemusic", "Apple Music"),
        ("tidal", "TIDAL"),
        ("deezer", "Deezer"),
    ];
    for (k, v) in known {
        if l.contains(k) {
            return v.to_string();
        }
    }
    let base = aumid.rsplit(['\\', '!']).next().unwrap_or(aumid);
    base.trim_end_matches(".exe").to_string()
}

fn filetime_to_unix_ms(ut: i64) -> i64 {
    (ut - 116_444_736_000_000_000) / 10_000
}

fn read_thumb(props: &GlobalSystemMediaTransportControlsSessionMediaProperties) -> Option<Vec<u8>> {
    let r = props.Thumbnail().ok()?;
    let stream = r.OpenReadAsync().ok()?.get().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 8 * 1024 * 1024 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.get().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    Some(buf)
}

pub fn start(hwnd: HWND) -> MediaHandle {
    let info = Arc::new(Mutex::new(None));
    let session = Arc::new(Mutex::new(None));
    let (tx, rx) = channel::<()>();
    let handle = MediaHandle { info: info.clone(), session: session.clone(), poke: tx.clone() };
    let hwnd_raw = hwnd.0 as isize;

    std::thread::Builder::new()
        .name("topline-media".into())
        .spawn(move || {
            unsafe {
                let _ = RoInitialize(RO_INIT_MULTITHREADED);
            }
            let post = move || unsafe {
                let _ = PostMessageW(HWND(hwnd_raw as *mut _), WM_APP_MEDIA, WPARAM(0), LPARAM(0));
            };
            let mgr = match GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.get()) {
                Ok(m) => m,
                Err(_) => return,
            };
            {
                let tx = tx.clone();
                let _ = mgr.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = tx.send(());
                    Ok(())
                }));
            }
            {
                let tx = tx.clone();
                let _ = mgr.SessionsChanged(&TypedEventHandler::new(move |_, _| {
                    let _ = tx.send(());
                    Ok(())
                }));
            }

            let mut current: Option<GlobalSystemMediaTransportControlsSession> = None;
            let mut tokens: Vec<(u8, windows::Foundation::EventRegistrationToken)> = Vec::new();
            let mut last_thumb_key = String::new();
            let mut last_thumb: Option<Arc<Vec<u8>>> = None;
            let _ = tx.send(());

            while rx.recv().is_ok() {
                // gabungkan rentetan event
                std::thread::sleep(Duration::from_millis(60));
                while rx.try_recv().is_ok() {}

                let s = mgr.GetCurrentSession().ok();
                let changed = match (&current, &s) {
                    (Some(a), Some(b)) => a != b,
                    (None, None) => false,
                    _ => true,
                };
                if changed {
                    if let Some(old) = &current {
                        for (kind, t) in tokens.drain(..) {
                            let _ = match kind {
                                0 => old.RemoveMediaPropertiesChanged(t),
                                1 => old.RemovePlaybackInfoChanged(t),
                                _ => old.RemoveTimelinePropertiesChanged(t),
                            };
                        }
                    }
                    if let Some(ns) = &s {
                        let t1 = tx.clone();
                        if let Ok(t) = ns.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                            let _ = t1.send(());
                            Ok(())
                        })) {
                            tokens.push((0, t));
                        }
                        let t2 = tx.clone();
                        if let Ok(t) = ns.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                            let _ = t2.send(());
                            Ok(())
                        })) {
                            tokens.push((1, t));
                        }
                        let t3 = tx.clone();
                        if let Ok(t) = ns.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
                            let _ = t3.send(());
                            Ok(())
                        })) {
                            tokens.push((2, t));
                        }
                    }
                    current = s.clone();
                    if let Ok(mut g) = session.lock() {
                        *g = s.clone();
                    }
                }

                let snapshot = s.as_ref().and_then(|s| {
                    let props = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
                    let title = props.Title().map(|h| h.to_string()).unwrap_or_default();
                    let artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
                    if title.is_empty() && artist.is_empty() {
                        return None;
                    }
                    let app = s.SourceAppUserModelId().map(|h| friendly_app(&h.to_string())).unwrap_or_default();
                    let playing = s
                        .GetPlaybackInfo()
                        .and_then(|p| p.PlaybackStatus())
                        .map(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
                        .unwrap_or(false);
                    let (mut position_ms, mut duration_ms, mut updated_ms) = (0, 0, 0);
                    if let Ok(tl) = s.GetTimelineProperties() {
                        position_ms = tl.Position().map(|t| t.Duration / 10_000).unwrap_or(0);
                        let start = tl.StartTime().map(|t| t.Duration / 10_000).unwrap_or(0);
                        duration_ms = tl.EndTime().map(|t| t.Duration / 10_000).unwrap_or(0) - start;
                        position_ms -= start;
                        updated_ms = tl.LastUpdatedTime().map(|d| filetime_to_unix_ms(d.UniversalTime)).unwrap_or(0);
                    }
                    let key = format!("{}\u{1}{}", title, artist);
                    if key != last_thumb_key {
                        last_thumb = read_thumb(&props).map(Arc::new);
                        // Thumbnail kadang baru tersedia sesaat kemudian; coba lagi nanti.
                        if last_thumb.is_some() {
                            last_thumb_key = key.clone();
                        }
                    }
                    Some(MediaInfo {
                        title,
                        artist,
                        app,
                        playing,
                        position_ms,
                        duration_ms,
                        updated_ms,
                        thumb: last_thumb.clone(),
                        thumb_key: if last_thumb.is_some() { key } else { String::new() },
                    })
                });
                if let Ok(mut g) = info.lock() {
                    *g = snapshot;
                }
                post();
            }
        })
        .ok();
    handle
}
