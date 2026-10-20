//! Launcher mini: drop target OLE di jendela notch, pembacaan item shell dari IDataObject,
//! ikon (IShellItemImageFactory → WIC → cache PNG), membuka target (ShellExecuteEx), dan menu
//! konteks tile. Hanya referensi yang disimpan; file pengguna tidak pernah disalin/dipindah.

use std::cell::Cell;
use std::collections::HashMap;
use std::mem::size_of;
use std::path::PathBuf;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Gdi::DeleteObject;
use windows::Win32::System::Com::{CoTaskMemFree, IDataObject};
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::{MODIFIERKEYS_FLAGS, SFGAO_FOLDER, SFGAO_STREAM};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use topline_core::{Lang, Pin, PinInput, PinKind};

use crate::gfx::Gfx;
use crate::util;

/// Kejadian drag OLE yang diteruskan ke App.
pub enum DropEvent {
    Enter,
    Leave,
    Drop(Vec<PinInput>),
}

/// Folder aplikasi (AppsFolder) dalam bentuk parsing name; diganti alias `shell:AppsFolder\`.
const APPS_FOLDER: &str = "::{4234D49B-0245-4DF3-B780-3893943456E1}\\";

// ------------------------------------------------------------------ drop target

#[implement(IDropTarget)]
pub struct DropTarget {
    hwnd: isize,
    /// Drag yang sedang berlangsung berisi item shell yang bisa disematkan.
    accept: Cell<bool>,
}

impl DropTarget {
    pub fn new(hwnd: HWND) -> Self {
        Self { hwnd: hwnd.0 as isize, accept: Cell::new(false) }
    }
}

/// Efek drop: selalu LINK (hanya referensi). Jika sumber tidak mengizinkan LINK, pakai COPY;
/// keduanya tidak membuat sumber menghapus/memindahkan apa pun. MOVE tidak pernah dipakai.
fn choose(allowed: DROPEFFECT, ok: bool) -> DROPEFFECT {
    if !ok {
        DROPEFFECT_NONE
    } else if allowed.0 & DROPEFFECT_LINK.0 != 0 {
        DROPEFFECT_LINK
    } else if allowed.0 & DROPEFFECT_COPY.0 != 0 {
        DROPEFFECT_COPY
    } else {
        DROPEFFECT_NONE
    }
}

fn set_effect(effect: *mut DROPEFFECT, ok: bool) {
    if !effect.is_null() {
        unsafe { *effect = choose(*effect, ok) };
    }
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(&self, data: Option<&IDataObject>, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let ok = data.map_or(false, |d| unsafe { SHCreateShellItemArrayFromDataObject::<_, IShellItemArray>(d) }.is_ok());
        self.accept.set(ok);
        set_effect(effect, ok);
        if ok {
            crate::app::drop_event(HWND(self.hwnd as *mut _), DropEvent::Enter);
        }
        Ok(())
    }

    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        set_effect(effect, self.accept.get());
        Ok(())
    }

    fn DragLeave(&self) -> Result<()> {
        if self.accept.replace(false) {
            crate::app::drop_event(HWND(self.hwnd as *mut _), DropEvent::Leave);
        }
        Ok(())
    }

    fn Drop(&self, data: Option<&IDataObject>, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let items = data.map(read_items).unwrap_or_default();
        set_effect(effect, !items.is_empty());
        let accepted = self.accept.replace(false);
        let hwnd = HWND(self.hwnd as *mut _);
        if !items.is_empty() {
            crate::app::drop_event(hwnd, DropEvent::Drop(items));
        } else if accepted {
            crate::app::drop_event(hwnd, DropEvent::Leave);
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ item shell

fn display_name(item: &IShellItem, sigdn: SIGDN) -> Option<String> {
    unsafe {
        let p = item.GetDisplayName(sigdn).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

/// `::{AppsFolder}\AUMID` → `shell:AppsFolder\AUMID`; item virtual lain `::{..}` → `shell:::{..}`.
fn normalize(target: String) -> String {
    if target.len() > APPS_FOLDER.len() && target[..APPS_FOLDER.len()].eq_ignore_ascii_case(APPS_FOLDER) {
        format!("shell:AppsFolder\\{}", &target[APPS_FOLDER.len()..])
    } else if target.starts_with("::") {
        format!("shell:{}", target)
    } else {
        target
    }
}

fn extension(target: &str) -> String {
    std::path::Path::new(target).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Item di folder Aplikasi (Start menu, aplikasi Store): parsing name-nya kadang hanya AUMID
/// tanpa awalan, jadi dikenali dari induknya.
fn apps_folder_target(item: &IShellItem) -> Option<String> {
    let parent = unsafe { item.GetParent() }.ok()?;
    let p = display_name(&parent, SIGDN_DESKTOPABSOLUTEPARSING)?;
    if !p.eq_ignore_ascii_case(APPS_FOLDER.trim_end_matches('\\')) {
        return None;
    }
    Some(format!("shell:AppsFolder\\{}", display_name(item, SIGDN_PARENTRELATIVEPARSING)?))
}

fn pin_input(item: &IShellItem) -> Option<PinInput> {
    let target = match apps_folder_target(item) {
        Some(t) => t,
        None => normalize(display_name(item, SIGDN_DESKTOPABSOLUTEPARSING)?),
    };
    let name = display_name(item, SIGDN_NORMALDISPLAY).unwrap_or_default();
    let attrs = unsafe { item.GetAttributes(SFGAO_FOLDER | SFGAO_STREAM) }.unwrap_or_default();
    let ext = extension(&target);
    let kind = if target.to_lowercase().starts_with("shell:appsfolder\\") || matches!(ext.as_str(), "exe" | "lnk" | "url" | "appref-ms") {
        PinKind::App
    } else if attrs.0 & SFGAO_FOLDER.0 != 0 && attrs.0 & SFGAO_STREAM.0 == 0 {
        // .zip punya SFGAO_FOLDER juga, tapi juga STREAM: tetap file
        PinKind::Folder
    } else {
        PinKind::File
    };
    Some(PinInput { kind, target, name })
}

/// Item dari CF_HDROP maupun Shell IDList Array (Start menu, aplikasi Store).
pub fn read_items(data: &IDataObject) -> Vec<PinInput> {
    unsafe {
        let Ok(arr) = SHCreateShellItemArrayFromDataObject::<_, IShellItemArray>(data) else { return Vec::new() };
        let n = arr.GetCount().unwrap_or(0);
        (0..n).filter_map(|i| arr.GetItemAt(i).ok()).filter_map(|it| pin_input(&it)).collect()
    }
}

/// Target masih ada? Alias `shell:` dianggap selalu ada. Dipanggil hanya saat launcher digambar.
pub fn exists(target: &str) -> bool {
    target.starts_with("shell:") || std::path::Path::new(target).exists()
}

/// "Jalankan sebagai administrator" hanya untuk .exe/.lnk.
pub fn can_elevate(target: &str) -> bool {
    matches!(extension(target).as_str(), "exe" | "lnk")
}

// ------------------------------------------------------------------ buka

/// Buka target dengan aplikasi default (atau verb "runas"). false = gagal (bukan dibatalkan).
pub fn launch(hwnd: HWND, target: &str, admin: bool) -> bool {
    let file = util::wide(target);
    unsafe {
        // Notch tidak pernah aktif; izinkan jendela baru tampil di depan.
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            hwnd,
            lpVerb: if admin { w!("runas") } else { PCWSTR::null() },
            lpFile: PCWSTR(file.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        match ShellExecuteExW(&mut sei) {
            Ok(()) => true,
            Err(e) => e.code() == ERROR_CANCELLED.to_hresult(),
        }
    }
}

/// Tampilkan file/folder di Explorer dengan item terpilih.
pub fn reveal(target: &str) {
    let w = util::wide(target);
    unsafe {
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        if SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut pidl, 0, None).is_ok() && !pidl.is_null() {
            let _ = SHOpenFolderAndSelectItems(pidl, None, 0);
            CoTaskMemFree(Some(pidl as *const _));
        }
    }
}

pub const MENU_REVEAL: usize = 1;
pub const MENU_ADMIN: usize = 2;
pub const MENU_UNPIN: usize = 3;

/// Menu konteks tile; kembalikan id item (0 = batal).
pub fn pin_menu(owner: HWND, pin: &Pin, l: Lang) -> usize {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return 0 };
        let add = |id: usize, text: &str, enabled: bool| {
            let w = util::wide(text);
            let flags = MF_STRING | if enabled { MF_ENABLED } else { MF_GRAYED };
            let _ = AppendMenuW(menu, flags, id, PCWSTR(w.as_ptr()));
        };
        add(MENU_REVEAL, l.t("Buka lokasi file", "Open file location"), !pin.target.starts_with("shell:"));
        add(MENU_ADMIN, l.t("Jalankan sebagai administrator", "Run as administrator"), can_elevate(&pin.target));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        add(MENU_UNPIN, l.t("Lepas sematan", "Unpin"), true);
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        let _ = SetForegroundWindow(owner);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, p.x, p.y, 0, owner, None);
        let _ = PostMessageW(owner, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        cmd.0 as usize
    }
}

// ------------------------------------------------------------------ ikon

/// Ikon sematan: cache memori per target + PNG di %LOCALAPPDATA%\Topline\icons.
#[derive(Default)]
pub struct Icons {
    map: HashMap<String, Option<ID2D1Bitmap>>,
    px: u32,
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

fn icon_path(target: &str, px: u32) -> PathBuf {
    util::data_dir().join("icons").join(format!("{:016x}-{}.png", fnv(&target.to_lowercase()), px))
}

fn load_icon(gfx: &Gfx, target: &str, px: u32) -> Option<ID2D1Bitmap> {
    let path = icon_path(target, px);
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(b) = gfx.decode_image(&bytes) {
            return Some(b);
        }
    }
    unsafe {
        let w = util::wide(target);
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(w.as_ptr()), None).ok()?;
        let factory: IShellItemImageFactory = item.cast().ok()?;
        let size = SIZE { cx: px as i32, cy: px as i32 };
        let hbmp = factory.GetImage(size, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok()?;
        let _ = std::fs::create_dir_all(path.parent()?);
        let saved = gfx.save_png(hbmp, &path);
        let _ = DeleteObject(hbmp);
        if let Err(e) = saved {
            util::log(&format!("icon png error: {e}"));
            return None;
        }
    }
    std::fs::read(&path).ok().and_then(|b| gfx.decode_image(&b).ok())
}

impl Icons {
    /// Pastikan ikon semua sematan ada di memori (ekstraksi hanya untuk yang belum pernah dimuat).
    /// Ikon sematan yang sudah dilepas dibuang dari memori dan disk.
    pub fn ensure(&mut self, gfx: &Gfx, pins: &[Pin], scale: f32) {
        let px = (48.0 * scale).round() as u32;
        if px != self.px {
            self.map.clear();
            self.px = px;
        }
        for p in pins {
            if !self.map.contains_key(&p.target) {
                let b = load_icon(gfx, &p.target, px);
                self.map.insert(p.target.clone(), b);
            }
        }
        let gone: Vec<String> = self.map.keys().filter(|k| !pins.iter().any(|p| &p.target == *k)).cloned().collect();
        for k in gone {
            self.map.remove(&k);
            let _ = std::fs::remove_file(icon_path(&k, px));
        }
    }

    pub fn get(&self, target: &str) -> Option<&ID2D1Bitmap> {
        self.map.get(target).and_then(|b| b.as_ref())
    }
}
