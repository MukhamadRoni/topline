use std::path::PathBuf;

fn main() {
    // Installer menyematkan topline.exe hasil build sebelumnya (lihat build.ps1).
    let exe = std::env::var("TOPLINE_EXE").unwrap_or_else(|_| {
        let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
        root.join("target/release/topline.exe").display().to_string()
    });
    println!("cargo:rerun-if-env-changed=TOPLINE_EXE");
    println!("cargo:rerun-if-changed={}", exe);
    let exe = PathBuf::from(&exe);
    if !exe.exists() {
        panic!("topline.exe belum ada di {}. Build aplikasi dulu: cargo build --release -p topline", exe.display());
    }
    let ico = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../assets/topline.ico");
    let code = format!(
        "pub static APP_EXE: &[u8] = include_bytes!(r\"{}\");\npub static APP_ICO: &[u8] = include_bytes!(r\"{}\");\n",
        exe.canonicalize().unwrap().display(),
        ico.canonicalize().unwrap().display()
    );
    std::fs::write(PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("payload.rs"), code).unwrap();
    println!("cargo:rerun-if-env-changed=TOPLINE_SETUP_RES");
    if let Ok(res) = std::env::var("TOPLINE_SETUP_RES") {
        if std::path::Path::new(&res).exists() {
            println!("cargo:rerun-if-changed={}", res);
            println!("cargo:rustc-link-arg-bins={}", res);
        }
    }
}
