use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let assets = root.join("assets");

    // Font Geist dibundel ke dalam exe jika tersedia (diunduh oleh build.ps1).
    // Jika tidak ada, aplikasi memakai Segoe UI Variable bawaan Windows 11.
    let fonts = [
        ("GEIST_REGULAR", "fonts/Geist-Regular.ttf"),
        ("GEIST_MEDIUM", "fonts/Geist-Medium.ttf"),
        ("GEIST_MONO", "fonts/GeistMono-Regular.ttf"),
    ];
    let mut code = String::new();
    for (name, rel) in fonts {
        let p = assets.join(rel);
        println!("cargo:rerun-if-changed={}", p.display());
        if p.exists() {
            code.push_str(&format!(
                "pub static {}: &[u8] = include_bytes!(r\"{}\");\n",
                name,
                p.canonicalize().unwrap().display()
            ));
        } else {
            code.push_str(&format!("pub static {}: &[u8] = &[];\n", name));
        }
    }
    let ico = assets.join("topline.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    code.push_str(&format!(
        "pub static APP_ICON: &[u8] = include_bytes!(r\"{}\");\n",
        ico.canonicalize().unwrap().display()
    ));
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("embedded.rs");
    std::fs::write(out, code).unwrap();

    // Ikon exe + manifest + info versi: objek COFF dari windres (dibuat build.ps1).
    println!("cargo:rerun-if-env-changed=TOPLINE_APP_RES");
    if let Ok(res) = std::env::var("TOPLINE_APP_RES") {
        if std::path::Path::new(&res).exists() {
            println!("cargo:rerun-if-changed={}", res);
            println!("cargo:rustc-link-arg-bins={}", res);
        }
    }
}
