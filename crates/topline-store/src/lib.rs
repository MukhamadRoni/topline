//! Penyimpanan lokal: satu file JSON, ditulis secara atomik (tulis ke file
//! sementara lalu rename) agar data tidak rusak jika aplikasi mati mendadak.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use topline_core::Persisted;

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self { path: dir.as_ref().join("data.json") }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Memuat data. File hilang atau rusak -> data default (file rusak disimpan
    /// sebagai `data.json.bak` agar bisa diperiksa).
    pub fn load(&self) -> Persisted {
        match fs::read(&self.path) {
            Ok(bytes) => match serde_json::from_slice::<Persisted>(&bytes) {
                Ok(p) => p,
                Err(_) => {
                    let _ = fs::copy(&self.path, self.path.with_extension("json.bak"));
                    Persisted::default()
                }
            },
            Err(_) => Persisted::default(),
        }
    }

    pub fn save(&self, data: &Persisted) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&serde_json::to_vec_pretty(data).map_err(std::io::Error::other)?)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use topline_core::{Persisted, Todo};

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("topline-store-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn roundtrip() {
        let dir = tmpdir("rt");
        let s = Store::new(&dir);
        let mut p = Persisted::default();
        p.todos.push(Todo { id: 1, text: "Rapat".into(), time: Some((16, 30)), date: "2026-10-20".into(), done: false, notified: false });
        p.water.total_ml = 750;
        s.save(&p).unwrap();
        assert_eq!(s.load(), p);
    }

    #[test]
    fn missing_or_corrupt_gives_default() {
        let dir = tmpdir("bad");
        let s = Store::new(&dir);
        assert_eq!(s.load(), Persisted::default());
        fs::create_dir_all(&dir).unwrap();
        fs::write(s.path(), b"{not json").unwrap();
        assert_eq!(s.load(), Persisted::default());
        assert!(dir.join("data.json.bak").exists());
    }

    #[test]
    fn partial_json_fills_defaults() {
        let dir = tmpdir("partial");
        let s = Store::new(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(s.path(), br#"{"settings":{"personality":true}}"#).unwrap();
        let p = s.load();
        assert!(p.settings.personality);
        assert_eq!(p.settings.water_goal_ml, 2000);
        assert_eq!(p.settings.eye_sleep_minutes, 5, "data lama tanpa field baru tetap terbaca");
    }
}
