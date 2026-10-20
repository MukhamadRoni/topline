//! Installer Topline: memasang untuk pengguna saat ini (tanpa hak admin) ke
//! `%LOCALAPPDATA%\Programs\Topline`, membuat shortcut Start Menu, entri
//! "Aplikasi terpasang", autostart opsional, dan uninstaller.
//!
//! Argumen:
//!   (tanpa argumen)   pasang dengan dialog
//!   /S                pasang diam-diam (untuk pengujian), autostart aktif
//!   /S /nolaunch      pasang diam-diam tanpa menjalankan aplikasi
//!   /uninstall        copot (dipakai oleh "Aplikasi terpasang")
#![windows_subsystem = "windows"]

use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::Com::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

mod payload {
    include!(concat!(env!("OUT_DIR"), "/payload.rs"));
}

const VERSION: &str = env!("CARGO_PKG_VERSION");
const UNINSTALL_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Topline";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

/// Installer mengikuti bahasa tampilan Windows: Indonesia jika Windows berbahasa Indonesia,
/// selain itu Inggris.
fn t<'a>(id: &'a str, en: &'a str) -> &'a str {
    // LANG_INDONESIAN = 0x21 (primary language id)
    if unsafe { GetUserDefaultUILanguage() } & 0x3ff == 0x21 {
        id
    } else {
        en
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn msg(text: &str, flags: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    let t = wide(text);
    unsafe { MessageBoxW(None, PCWSTR(t.as_ptr()), w!("Topline Setup"), flags | MB_SETFOREGROUND) }
}

fn install_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Programs")
        .join("Topline")
}

fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Topline")
}

fn programs_dir() -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Programs, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

fn shortcut_path() -> Option<PathBuf> {
    programs_dir().map(|d| d.join("Topline.lnk"))
}

/// Tutup Topline yang sedang berjalan dan tunggu sampai jendelanya hilang.
fn close_running_app() {
    unsafe {
        for _ in 0..50 {
            match FindWindowW(w!("ToplineNotch"), None) {
                Ok(h) if !h.is_invalid() => {
                    let _ = PostMessageW(h, WM_CLOSE, WPARAM(0), LPARAM(0));
                    std::thread::sleep(Duration::from_millis(100));
                }
                _ => break,
            }
        }
    }
    std::thread::sleep(Duration::from_millis(300));
}

fn write_retry(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut last = None;
    for _ in 0..20 {
        match std::fs::write(path, bytes) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
    Err(last.unwrap())
}

fn reg_set_str(key: HKEY, name: &str, value: &str) {
    let n = wide(name);
    let v = wide(value);
    unsafe {
        let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
        let _ = RegSetValueExW(key, PCWSTR(n.as_ptr()), 0, REG_SZ, Some(bytes));
    }
}

fn reg_set_dword(key: HKEY, name: &str, value: u32) {
    let n = wide(name);
    unsafe {
        let _ = RegSetValueExW(key, PCWSTR(n.as_ptr()), 0, REG_DWORD, Some(&value.to_le_bytes()));
    }
}

fn reg_open(path: &str) -> Option<HKEY> {
    let p = wide(path);
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(p.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()
        .ok()?;
    }
    Some(key)
}

fn create_shortcut(lnk: &Path, target: &Path, icon: &Path) -> Result<()> {
    unsafe {
        let sl: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        sl.SetPath(&HSTRING::from(target.as_os_str()))?;
        if let Some(dir) = target.parent() {
            sl.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        }
        sl.SetIconLocation(&HSTRING::from(icon.as_os_str()), 0)?;
        sl.SetDescription(&HSTRING::from(t("Topline - notch hidup untuk Windows", "Topline - a living notch for Windows")))?;
        let pf: IPersistFile = sl.cast()?;
        pf.Save(&HSTRING::from(lnk.as_os_str()), TRUE)?;
    }
    Ok(())
}

fn install(silent: bool, launch: bool) -> std::result::Result<(), String> {
    let dir = install_dir();
    if !silent {
        let q = format!(
            "{}\n{}",
            t(
                &format!("Pasang Topline {VERSION}?\n\nTopline dipasang hanya untuk akun Windows ini (tanpa hak admin) di:"),
                &format!("Install Topline {VERSION}?\n\nTopline is installed for this Windows account only (no admin rights) in:"),
            ),
            dir.display()
        );
        if msg(&q, MB_YESNO | MB_ICONQUESTION) != IDYES {
            return Ok(());
        }
    }
    close_running_app();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{} {}: {e}", t("Tidak bisa membuat folder", "Couldn't create folder"), dir.display()))?;
    let exe = dir.join("topline.exe");
    let ico = dir.join("topline.ico");
    let uninst = dir.join("uninstall.exe");
    write_retry(&exe, payload::APP_EXE).map_err(|e| {
        format!("{}: {e}\n{}", t("Tidak bisa menyalin topline.exe", "Couldn't copy topline.exe"), t("Tutup Topline lalu coba lagi.", "Close Topline and try again."))
    })?;
    write_retry(&ico, payload::APP_ICO).map_err(|e| e.to_string())?;
    if let Ok(me) = std::env::current_exe() {
        if me != uninst {
            let _ = std::fs::copy(&me, &uninst);
        }
    }

    if let Some(lnk) = shortcut_path() {
        if let Some(d) = lnk.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = create_shortcut(&lnk, &exe, &ico);
    }

    if let Some(key) = reg_open(UNINSTALL_KEY) {
        reg_set_str(key, "DisplayName", "Topline");
        reg_set_str(key, "DisplayVersion", VERSION);
        reg_set_str(key, "Publisher", "Topline");
        reg_set_str(key, "DisplayIcon", &ico.display().to_string());
        reg_set_str(key, "InstallLocation", &dir.display().to_string());
        reg_set_str(key, "UninstallString", &format!("\"{}\" /uninstall", uninst.display()));
        reg_set_str(key, "QuietUninstallString", &format!("\"{}\" /S /uninstall", uninst.display()));
        reg_set_dword(key, "NoModify", 1);
        reg_set_dword(key, "NoRepair", 1);
        reg_set_dword(key, "EstimatedSize", (payload::APP_EXE.len() / 1024) as u32 + 64);
        unsafe {
            let _ = RegCloseKey(key);
        }
    }

    let autostart = silent
        || msg(
            t(
                "Jalankan Topline otomatis setiap Windows dimulai?\n\n(Bisa diubah kapan saja di Pengaturan Topline.)",
                "Start Topline automatically with Windows?\n\n(You can change this anytime in Topline Settings.)",
            ),
            MB_YESNO | MB_ICONQUESTION,
        ) == IDYES;
    if let Some(key) = reg_open(RUN_KEY) {
        let name = wide("Topline");
        unsafe {
            if autostart {
                reg_set_str(key, "Topline", &format!("\"{}\"", exe.display()));
            } else {
                let _ = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
            }
            let _ = RegCloseKey(key);
        }
    }

    if launch {
        let _ = std::process::Command::new(&exe).current_dir(&dir).spawn();
    }
    if !silent {
        msg(
            t(
                "Topline sudah terpasang dan berjalan.\n\nArahkan kursor ke tengah atas layar, atau tekan Win + Alt + T.\nTambah tugas cepat: Win + Alt + N.\nPengaturan ada di ikon Topline di system tray.",
                "Topline is installed and running.\n\nMove the cursor to the top center of the screen, or press Win + Alt + T.\nQuick add task: Win + Alt + N.\nSettings are in the Topline icon in the system tray.",
            ),
            MB_OK | MB_ICONINFORMATION,
        );
    }
    Ok(())
}

fn uninstall(silent: bool) -> std::result::Result<(), String> {
    let dir = install_dir();
    // Uninstaller tidak bisa menghapus dirinya sendiri: jalankan salinan dari %TEMP%.
    if let Ok(me) = std::env::current_exe() {
        if me.parent() == Some(dir.as_path()) {
            let tmp = std::env::temp_dir().join(format!("topline-uninstall-{}.exe", std::process::id()));
            if std::fs::copy(&me, &tmp).is_ok() {
                let mut c = std::process::Command::new(&tmp);
                if silent {
                    c.arg("/S");
                }
                let _ = c.arg("/uninstall").arg("/from-temp").spawn();
                return Ok(());
            }
        }
    }
    if !silent && msg(t("Copot Topline dari komputer ini?", "Uninstall Topline from this computer?"), MB_YESNO | MB_ICONQUESTION) != IDYES {
        return Ok(());
    }
    close_running_app();
    if let Some(key) = reg_open(RUN_KEY) {
        let name = wide("Topline");
        unsafe {
            let _ = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
            let _ = RegCloseKey(key);
        }
    }
    unsafe {
        let k = wide(UNINSTALL_KEY);
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()));
    }
    if let Some(lnk) = shortcut_path() {
        let _ = std::fs::remove_file(lnk);
    }
    for _ in 0..20 {
        if std::fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let data = data_dir();
    if !silent
        && data.exists()
        && msg(
            t(
                "Hapus juga data Topline (daftar tugas, catatan air minum, pengaturan)?",
                "Also delete Topline data (tasks, water log, settings)?",
            ),
            MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2,
        ) == IDYES
    {
        let _ = std::fs::remove_dir_all(&data);
    }
    if !silent {
        msg(t("Topline sudah dicopot.", "Topline has been uninstalled."), MB_OK | MB_ICONINFORMATION);
    }
    Ok(())
}

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let args: Vec<String> = std::env::args().skip(1).map(|a| a.to_lowercase()).collect();
    let silent = args.iter().any(|a| a == "/s");
    let result = if args.iter().any(|a| a == "/uninstall") {
        uninstall(silent)
    } else {
        install(silent, !args.iter().any(|a| a == "/nolaunch"))
    };
    if let Err(e) = result {
        if !silent {
            msg(&format!("{}\n{e}", t("Terjadi kesalahan:", "Something went wrong:")), MB_OK | MB_ICONERROR);
        }
        std::process::exit(1);
    }
}
