//! Dua bahasa UI: Indonesia dan Inggris. Teks ditulis berpasangan di tempat dipakai
//! (`lang.t("Teks", "Text")`) agar mudah dirawat dan diperiksa compiler.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Pilihan bahasa di Pengaturan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Language {
    /// Ikuti bahasa tampilan Windows (Indonesia jika Windows berbahasa Indonesia, selain itu Inggris).
    #[default]
    Auto,
    Id,
    En,
}

/// Bahasa yang benar-benar dipakai (Auto sudah diselesaikan oleh lapisan aplikasi).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    Id,
    En,
}

impl Language {
    /// `windows_is_indonesian`: bahasa tampilan Windows (GetUserDefaultUILanguage).
    pub fn resolve(self, windows_is_indonesian: bool) -> Lang {
        match self {
            Language::Id => Lang::Id,
            Language::En => Lang::En,
            Language::Auto if windows_is_indonesian => Lang::Id,
            Language::Auto => Lang::En,
        }
    }
}

impl Lang {
    /// Pilih teks sesuai bahasa.
    pub fn t<'a>(self, id: &'a str, en: &'a str) -> &'a str {
        match self {
            Lang::Id => id,
            Lang::En => en,
        }
    }

    pub fn is_en(self) -> bool {
        self == Lang::En
    }

    /// Pemisah desimal: "2,5" / "2.5".
    pub fn decimal(self, s: &str) -> String {
        match self {
            Lang::Id => s.replace('.', ","),
            Lang::En => s.to_string(),
        }
    }
}

/// Data lama (sebelum ada pilihan bahasa) selalu berbahasa Indonesia; jangan berubah setelah update.
pub fn legacy_language() -> Language {
    Language::Id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_auto_from_windows() {
        assert_eq!(Language::Auto.resolve(true), Lang::Id);
        assert_eq!(Language::Auto.resolve(false), Lang::En);
        assert_eq!(Language::Id.resolve(false), Lang::Id);
        assert_eq!(Language::En.resolve(true), Lang::En);
        assert_eq!(Lang::En.t("Halo", "Hello"), "Hello");
        assert_eq!(Lang::Id.decimal("2.5"), "2,5");
    }
}
