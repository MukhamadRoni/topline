//! Event sistem tanpa polling: daya/baterai, jaringan, volume, Caps Lock,
//! dan deteksi aplikasi full-screen (AppBar).

use std::mem::size_of;

use windows::core::*;
use windows::Networking::Connectivity::{NetworkInformation, NetworkStatusChangedEventHandler};
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::Endpoints::*;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Power::*;
use windows::Win32::System::SystemServices::{GUID_ACDC_POWER_SOURCE, GUID_BATTERY_PERCENTAGE_REMAINING};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Input::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::{WM_APP_APPBAR, WM_APP_NET, WM_APP_VOLUME};

pub struct SysMon {
    _power: Vec<HPOWERNOTIFY>,
    _net_token: Option<windows::Foundation::EventRegistrationToken>,
    volume: Option<(IAudioEndpointVolume, IAudioEndpointVolumeCallback)>,
    /// Id perangkat output default saat `volume` dibuat.
    volume_dev: String,
    appbar: bool,
    hwnd: HWND,
    pub caps_on: bool,
    caps_down: bool,
    /// Raw Input mouse terdaftar (hanya selama mata tidur).
    mouse: bool,
}

/// Hasil satu WM_INPUT.
pub struct RawEvent {
    /// Ada input keyboard/mouse dari pengguna.
    pub activity: bool,
    /// Status Caps Lock baru, jika berubah.
    pub caps: Option<bool>,
}

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeCallback {
    hwnd: isize,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeCallback_Impl {
    fn OnNotify(&self, pnotify: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        if pnotify.is_null() {
            return Ok(());
        }
        let d = unsafe { &*pnotify };
        let pct = (d.fMasterVolume * 100.0).round().clamp(0.0, 100.0) as usize;
        let muted = d.bMuted.as_bool() as isize;
        unsafe {
            let _ = PostMessageW(HWND(self.hwnd as *mut _), WM_APP_VOLUME, WPARAM(pct), LPARAM(muted));
        }
        Ok(())
    }
}

impl SysMon {
    pub fn start(hwnd: HWND) -> Self {
        let mut power = Vec::new();
        unsafe {
            for g in [&GUID_ACDC_POWER_SOURCE, &GUID_BATTERY_PERCENTAGE_REMAINING] {
                if let Ok(h) = RegisterPowerSettingNotification(HANDLE(hwnd.0), g, DEVICE_NOTIFY_WINDOW_HANDLE) {
                    power.push(h);
                }
            }
        }

        let raw = hwnd.0 as isize;
        let net_token = NetworkInformation::NetworkStatusChanged(&NetworkStatusChangedEventHandler::new(move |_| {
            unsafe {
                let _ = PostMessageW(HWND(raw as *mut _), WM_APP_NET, WPARAM(0), LPARAM(0));
            }
            Ok(())
        }))
        .ok();

        let (volume, volume_dev) = match unsafe { Self::start_volume(hwnd) } {
            Ok((v, cb, id)) => (Some((v, cb)), id),
            Err(_) => (None, String::new()),
        };

        // Caps Lock lewat Raw Input (tanpa hook keyboard global).
        unsafe {
            let dev = RAWINPUTDEVICE {
                usUsagePage: 0x01,
                usUsage: 0x06,
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            };
            let _ = RegisterRawInputDevices(&[dev], size_of::<RAWINPUTDEVICE>() as u32);
        }

        // AppBar: Windows memberi tahu saat ada aplikasi full-screen.
        let appbar = unsafe {
            let mut abd = APPBARDATA {
                cbSize: size_of::<APPBARDATA>() as u32,
                hWnd: hwnd,
                uCallbackMessage: WM_APP_APPBAR,
                ..Default::default()
            };
            SHAppBarMessage(ABM_NEW, &mut abd) != 0
        };

        let caps_on = unsafe { (GetKeyState(VK_CAPITAL.0 as i32) & 1) != 0 };

        Self { _power: power, _net_token: net_token, volume, volume_dev, appbar, hwnd, caps_on, caps_down: false, mouse: false }
    }

    unsafe fn default_device() -> Result<(IMMDevice, String)> {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let p = device.GetId()?;
        let id = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        Ok((device, id))
    }

    unsafe fn start_volume(hwnd: HWND) -> Result<(IAudioEndpointVolume, IAudioEndpointVolumeCallback, String)> {
        let (device, id) = Self::default_device()?;
        let vol: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
        let cb: IAudioEndpointVolumeCallback = VolumeCallback { hwnd: hwnd.0 as isize }.into();
        vol.RegisterControlChangeNotify(&cb)?;
        Ok((vol, cb, id))
    }

    /// Lepas objek volume lama lalu buat ulang untuk perangkat default saat ini.
    fn restart_volume(&mut self) {
        unsafe {
            if let Some((vol, cb)) = self.volume.take() {
                let _ = vol.UnregisterControlChangeNotify(&cb);
            }
            if let Ok((v, cb, id)) = Self::start_volume(self.hwnd) {
                self.volume = Some((v, cb));
                self.volume_dev = id;
            }
        }
    }

    /// Jalankan `f` pada volume perangkat default. Perangkat default bisa berganti
    /// (headset dicolok): objek dibuat ulang jika id berubah, atau sekali jika panggilan gagal.
    /// Hanya dipanggil dari gestur pengguna, bukan berkala.
    fn with_volume(&mut self, f: impl Fn(&IAudioEndpointVolume) -> Result<()>) {
        let current = unsafe { Self::default_device() }.map(|(_, id)| id).ok();
        if self.volume.is_none() || current.as_ref().map_or(false, |id| *id != self.volume_dev) {
            self.restart_volume();
        }
        let ok = self.volume.as_ref().map_or(false, |(v, _)| f(v).is_ok());
        if !ok {
            self.restart_volume();
            if let Some((v, _)) = &self.volume {
                let _ = f(v);
            }
        }
    }

    /// Ubah volume master sebanyak `delta` persen (0–100). Menaikkan volume juga membatalkan mute.
    /// Peek volume muncul sendiri lewat IAudioEndpointVolumeCallback.
    pub fn set_volume_step(&mut self, delta: i32) {
        self.with_volume(|v| unsafe {
            let cur = (v.GetMasterVolumeLevelScalar()? * 100.0).round() as i32;
            let pct = (cur + delta).clamp(0, 100);
            if delta > 0 && v.GetMute()?.as_bool() {
                v.SetMute(false, std::ptr::null())?;
            }
            v.SetMasterVolumeLevelScalar(pct as f32 / 100.0, std::ptr::null())
        });
    }

    pub fn toggle_mute(&mut self) {
        self.with_volume(|v| unsafe {
            let muted = v.GetMute()?.as_bool();
            v.SetMute(!muted, std::ptr::null())
        });
    }

    /// (terhubung ke AC, persentase baterai jika ada)
    pub fn power_status() -> (bool, Option<u32>) {
        let mut s = SYSTEM_POWER_STATUS::default();
        unsafe {
            if GetSystemPowerStatus(&mut s).is_err() {
                return (true, None);
            }
        }
        let on_ac = s.ACLineStatus != 0;
        let pct = if s.BatteryLifePercent == 255 || (s.BatteryFlag & 128) != 0 {
            None
        } else {
            Some(s.BatteryLifePercent as u32)
        };
        (on_ac, pct)
    }

    pub fn network_connected() -> bool {
        match NetworkInformation::GetInternetConnectionProfile() {
            Ok(p) => p
                .GetNetworkConnectivityLevel()
                .map(|l| l.0 >= 1) // LocalAccess ke atas
                .unwrap_or(false),
            Err(_) => false,
        }
    }

    /// Daftarkan/lepas Raw Input mouse. Hanya aktif selama mata tidur, agar gerakan
    /// mouse tidak membangunkan proses saat tidak diperlukan.
    pub fn watch_mouse(&mut self, on: bool) {
        if on == self.mouse {
            return;
        }
        let dev = RAWINPUTDEVICE {
            usUsagePage: 0x01,
            usUsage: 0x02,
            dwFlags: if on { RIDEV_INPUTSINK } else { RIDEV_REMOVE },
            hwndTarget: if on { self.hwnd } else { HWND::default() },
        };
        unsafe {
            if RegisterRawInputDevices(&[dev], size_of::<RAWINPUTDEVICE>() as u32).is_ok() {
                self.mouse = on;
            }
        }
    }

    /// Proses WM_INPUT: aktivitas pengguna dan perubahan Caps Lock.
    pub fn on_raw_input(&mut self, lparam: LPARAM) -> RawEvent {
        let mut ev = RawEvent { activity: false, caps: None };
        ev.caps = self.read_raw(lparam, &mut ev.activity);
        ev
    }

    fn read_raw(&mut self, lparam: LPARAM, activity: &mut bool) -> Option<bool> {
        unsafe {
            let mut size = 0u32;
            let hdr = size_of::<RAWINPUTHEADER>() as u32;
            GetRawInputData(HRAWINPUT(lparam.0 as *mut _), RID_INPUT, None, &mut size, hdr);
            if size == 0 || size as usize > 256 {
                return None;
            }
            let mut buf = [0u8; 256];
            let got = GetRawInputData(
                HRAWINPUT(lparam.0 as *mut _),
                RID_INPUT,
                Some(buf.as_mut_ptr() as *mut _),
                &mut size,
                hdr,
            );
            if got == u32::MAX {
                return None;
            }
            let ri = &*(buf.as_ptr() as *const RAWINPUT);
            *activity = true;
            if ri.header.dwType != RIM_TYPEKEYBOARD.0 {
                return None;
            }
            let kb = ri.data.keyboard;
            if kb.VKey != VK_CAPITAL.0 {
                return None;
            }
            let up = (kb.Flags as u32 & RI_KEY_BREAK) != 0;
            if !up && !self.caps_down {
                self.caps_down = true;
                self.caps_on = !self.caps_on;
                return Some(self.caps_on);
            }
            if up {
                self.caps_down = false;
            }
            None
        }
    }

    pub fn stop(&mut self) {
        self.watch_mouse(false);
        unsafe {
            if let Some((vol, cb)) = self.volume.take() {
                let _ = vol.UnregisterControlChangeNotify(&cb);
            }
            if self.appbar {
                let mut abd = APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, hWnd: self.hwnd, ..Default::default() };
                SHAppBarMessage(ABM_REMOVE, &mut abd);
                self.appbar = false;
            }
            for h in self._power.drain(..) {
                let _ = UnregisterPowerSettingNotification(h);
            }
        }
        if let Some(t) = self._net_token.take() {
            let _ = NetworkInformation::RemoveNetworkStatusChanged(t);
        }
    }
}

/// Suara waktu sholat: `adzan.wav` di folder data jika ada, selain itu suara pengingat Windows.
pub fn play_adzan() {
    let f = crate::util::data_dir().join("adzan.wav");
    if f.exists() {
        let w = crate::util::wide(&f.display().to_string());
        unsafe {
            if PlaySoundW(PCWSTR(w.as_ptr()), None, SND_FILENAME | SND_ASYNC | SND_NODEFAULT).as_bool() {
                return;
            }
        }
    }
    play_sound(true);
}

pub fn play_sound(chime: bool) {
    unsafe {
        let alias = if chime { w!("Notification.Reminder") } else { w!("Notification.Default") };
        let _ = PlaySoundW(alias, None, SND_ALIAS | SND_ASYNC | SND_NODEFAULT);
    }
}
