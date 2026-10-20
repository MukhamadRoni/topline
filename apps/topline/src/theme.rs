//! Design tokens dari design.md (palet A · Electric Ultramarine) dan ukuran notch.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub f32, pub f32, pub f32, pub f32);

impl Rgba {
    pub const fn hex(rgb: u32, a: f32) -> Self {
        Rgba(
            ((rgb >> 16) & 0xff) as f32 / 255.0,
            ((rgb >> 8) & 0xff) as f32 / 255.0,
            (rgb & 0xff) as f32 / 255.0,
            a,
        )
    }
    pub const fn with_alpha(self, a: f32) -> Self {
        Rgba(self.0, self.1, self.2, a)
    }
}

pub const NAVY: Rgba = Rgba::hex(0x0B1660, 1.0);
pub const ULTRAMARINE: Rgba = Rgba::hex(0x0A4BF0, 1.0);
pub const ELECTRIC: Rgba = Rgba::hex(0x2271E9, 1.0);
pub const SKY: Rgba = Rgba::hex(0x40A9FD, 1.0);
pub const ICE: Rgba = Rgba::hex(0xF2F6FF, 1.0);
pub const MINT: Rgba = Rgba::hex(0x43BA95, 1.0);
pub const AMBER: Rgba = Rgba::hex(0xF5B544, 1.0);

pub const TEXT_1: Rgba = ICE;
pub const TEXT_2: Rgba = ICE.with_alpha(0.72);
pub const TEXT_3: Rgba = ICE.with_alpha(0.62);

pub const GLASS_FILL: Rgba = Rgba(1.0, 1.0, 1.0, 0.06);
pub const GLASS_FILL_HOVER: Rgba = Rgba(1.0, 1.0, 1.0, 0.14);
pub const GLASS_BORDER: Rgba = Rgba(1.0, 1.0, 1.0, 0.14);
pub const CARD_FILL: Rgba = Rgba(1.0, 1.0, 1.0, 0.05);
pub const CARD_BORDER: Rgba = Rgba(1.0, 1.0, 1.0, 0.10);
pub const TRACK: Rgba = Rgba(1.0, 1.0, 1.0, 0.14);
/// Latar baris to-do yang sedang diketik (pekat, sama dengan warna kolom EDIT).
pub const EDIT_FILL: Rgba = Rgba::hex(0x141A2E, 1.0);
pub const EDIT_FILL_RGB: (u8, u8, u8) = (0x14, 0x1A, 0x2E);

/// Tint kaca notch: rgba(8,12,30,.45). Fallback tanpa blur: rgba(11,22,96,.92).
pub const NOTCH_TINT: (u8, u8, u8, u8) = (8, 12, 30, 115);
pub const NOTCH_TINT_SOLID: (u8, u8, u8, u8) = (11, 22, 96, 235);

// ---- ukuran (DIP)
/// Ukuran jendela visual (tetap). Notch dianimasikan di dalamnya.
pub const WIN_W: f32 = 640.0;
pub const WIN_H: f32 = 540.0;

pub const IDLE_H: f32 = 36.0;
pub const IDLE_W_SMALL: f32 = 124.0;
pub const IDLE_W_WIDE: f32 = 208.0;
pub const IDLE_R: f32 = 18.0;

pub const PEEK_W: f32 = 340.0;
pub const PEEK_H: f32 = 44.0;
pub const PEEK_R: f32 = 22.0;

pub const EXP_W: f32 = 592.0;
pub const EXP_R: f32 = 28.0;
pub const PAD_X: f32 = 20.0;
pub const PAD_TOP: f32 = 12.0;
pub const HEADER_H: f32 = 40.0;
pub const GAP: f32 = 16.0;
pub const TABS_H: f32 = 48.0;
pub const PAD_BOTTOM: f32 = 14.0;

pub const ANIM_MS: u64 = 420;

// ---- glyph Segoe Fluent Icons (bawaan Windows 11)
pub const G_PLAY: char = '\u{E768}';
pub const G_PAUSE: char = '\u{E769}';
pub const G_PREV: char = '\u{E892}';
pub const G_NEXT: char = '\u{E893}';
pub const G_CHEVRON_UP: char = '\u{E70E}';
pub const G_ADD: char = '\u{E710}';
pub const G_CHECK: char = '\u{E73E}';
pub const G_MUSIC: char = '\u{E8D6}';
pub const G_TIMER: char = '\u{E916}';
pub const G_RESET: char = '\u{E72C}';
pub const G_TODO: char = '\u{E9D5}';
pub const G_VOLUME: char = '\u{E767}';
pub const G_MUTE: char = '\u{E74F}';
pub const G_WIFI: char = '\u{E701}';
pub const G_BOLT: char = '\u{E945}';
pub const G_BELL: char = '\u{EA8F}';
pub const G_KEYBOARD: char = '\u{E92E}';
pub const G_SKIP: char = '\u{E893}';
pub const G_MOON: char = '\u{E708}';
pub const G_PASTE: char = '\u{E77F}';
pub const G_PIN: char = '\u{E718}';
pub const G_CAMERA: char = '\u{E722}';
pub const G_MIC: char = '\u{E720}';
pub const G_DOWNLOAD: char = '\u{E896}';
pub const G_CODE: char = '\u{E943}';
pub const G_CANCEL: char = '\u{E711}';
pub const G_LOCATION: char = '\u{E707}';
pub const G_FOLDER: char = '\u{E8B7}';
pub const G_DOCUMENT: char = '\u{E8A5}';

// ---- launcher (DIP)
pub const PIN_COLS: usize = 4;
pub const PIN_GAP: f32 = 12.0;
pub const PIN_W: f32 = (EXP_W - PAD_X * 2.0 - PIN_GAP * 3.0) / 4.0;
pub const PIN_H: f32 = 104.0;
pub const PIN_R: f32 = 20.0;
pub const PIN_ICON: f32 = 48.0;
