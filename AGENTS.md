# AGENTS.md — Panduan untuk agent yang melanjutkan Topline

Baca file ini dulu, lalu `docs/ARCHITECTURE.md`, `docs/DESIGN.md`, dan `docs/ROADMAP.md`.

## Apa ini
**Topline** adalah notch "hidup" di tengah atas layar Windows 11. Isinya: pill idle (jam/musik/fokus/mata), widget yang bisa dibuka (Musik, Fokus/Pomodoro, Air minum, To-do), dan "peek" (kabar singkat sistem). Dibuat dengan **Rust native** (Win32 + Windows.UI.Composition + Direct2D/DirectWrite), tanpa WebView, **offline-first** (satu-satunya akses jaringan: lirik dari LRCLIB), target **0% CPU saat idle**.

Status: **v0.1 selesai dan teruji** di Windows 11 25H2, 1920x1080, skala 100%:
- CPU idle 0,000 s/10 s
- RAM sekitar 66–84 MB
- topline.exe 820 KB, setup.exe 1,26 MB
- uji pasang/copot silent lolos

Bahasa UI: **Indonesia dan Inggris** (`Settings.language`: auto/id/en). Setiap teks UI ditulis berpasangan di tempat dipakai: `lang.t("Teks", "Text")` (core: `Lang`, view: `c.t(..)`, Pengaturan: `l.t(..)`). Komentar kode tetap berbahasa Indonesia; ikuti gaya itu.

## Perintah cepat (PowerShell, dari root repo)
```powershell
powershell -ExecutionPolicy Bypass -File build.ps1              # build lengkap -> dist\topline.exe + dist\setup.exe
powershell -ExecutionPolicy Bypass -File build.ps1 -SkipSetup   # aplikasi saja
dist\topline.exe                                                 # jalankan versi uji
dist\setup.exe /S /nolaunch                                      # pasang diam-diam (uji)
"$env:LOCALAPPDATA\Programs\Topline\uninstall.exe" /S /uninstall # copot diam-diam
```
Unit test core (Rust murni, jalan di OS apa pun):
```powershell
$env:RUSTUP_HOME="$PWD\.toolchain\rustup"; $env:CARGO_HOME="$PWD\.toolchain\cargo"; $env:PATH="$env:CARGO_HOME\bin;$env:PATH"
cargo test -p topline-core -p topline-store -p topline-lyrics
```

### Lingkungan build (penting)
- Toolchain Rust **terpasang lokal** di `.toolchain\` (`stable-x86_64-pc-windows-gnu`, rustc 1.99). Tidak ada di PATH sistem. `build.ps1` mengatur `RUSTUP_HOME`, `CARGO_HOME`, dan `PATH`, lalu memasang toolchain jika belum ada.
- **Tidak ada Visual Studio / MSVC.** Target yang dipakai `x86_64-pc-windows-gnu` dengan linker bawaan Rust.
- `C:\MinGW\bin` (MinGW-w64, x86_64) **sengaja dikeluarkan dari PATH** saat `cargo build`, supaya gcc-nya tidak tertukar dengan linker Rust. MinGW hanya dipakai untuk `windres` (membuat objek resource ikon/manifest/versi). Hasilnya di `target\res\*.res.o`, lalu disambungkan lewat env `TOPLINE_APP_RES` / `TOPLINE_SETUP_RES` → `build.rs` → `cargo:rustc-link-arg-bins`.
- Font Geist / Geist Mono (OFL) diunduh `build.ps1` dari Google Fonts ke `assets\fonts\` dan disematkan ke exe (`include_bytes!` lewat `build.rs` → `embedded.rs`). Jika tidak ada, aplikasi memakai Segoe UI Variable.
- `build.ps1` menutup `topline.exe` yang berjalan dari folder ini sebelum menimpa `dist\topline.exe`.

### `.tools\` (build runner jarak jauh, opsional)
Dipakai sesi sebelumnya untuk menjalankan build dari luar mesin. `start-build-runner.bat` menjalankan `.tools\watch.ps1`, yang mengeksekusi setiap `.tools\req\NNN-*.ps1` dan menulis log ke `.tools\out\`. Skrip bantu:
- `.tools\run.ps1`: menjalankan dist\topline.exe secara terlepas lewat WMI
- `.tools\shot.ps1`: tangkapan layar area tengah atas
- `.tools\stat.ps1`: RAM/CPU proses

Kalau Anda bekerja langsung di mesin ini, **tidak perlu memakai runner**. Jangan memakai `Start-Process -Wait` untuk menjalankan topline.exe, karena opsi itu ikut menunggu proses turunan dan membuat skrip macet.

## Peta repo
```
Cargo.toml                 workspace (windows = 0.58, windows-core = 0.58, serde)
build.ps1                  build lengkap (toolchain, font, resource, exe, installer)
crates/topline-core/       SEMUA logika produk, tanpa Windows: Core, Command, SystemEvent, Effects,
                           View/Widget, FocusState, PeekKind, Persisted/Settings/Todo/WaterDay,
                           parse_task(), Clock, EyeMood (ekspresi mata + jadwal tidur), Clip (riwayat
                           clipboard), ProgressState. prayer.rs: jadwal sholat astronomis + daftar kota.
                           gesture.rs: gestur scroll (ScrollAccumulator: akumulasi delta, ambang 120,
                           cooldown; ScrollGestures: zona → volume/lagu/tab, tahan hover 1,5 s).
                           Pin/PinKind/PinInput (Persisted.pins), Widget::Launcher, MAX_PINS = 8.
                           LyricsState/LyricsResult, Core::caption(), lyrics_visible().
                           settings.rs: Toggle/Number (batas, langkah, format) untuk jendela Pengaturan.
                           i18n.rs: Language (auto/id/en) → Lang, Lang::t(id, en).
                           64 unit test.
crates/topline-lyrics/     parser .lrc (offset, multi-stempel), baris aktif + next_change, clean_title/queries
                           (judul YouTube "Artis - Judul"), pick_best (hasil LRCLIB). 5 unit test.
                           JANGAN menambah dependensi Windows di sini.
crates/topline-store/      Store: data.json dengan tulis atomik (tmp + rename), .bak jika rusak. 3 unit test.
apps/topline/src/
  main.rs                  modul, konstanta WM_APP_*, include embedded.rs
  app.rs                   App: wndproc, timer, hover, region, hotkey, tray menu, perakitan semua bagian
  notch.rs                 pohon visual Composition (backdrop, tint, content surface, eq, mata), animasi ukuran;
                           struct Eyes: mata Personality sebagai ShapeVisual, semua animasi ekspresi oleh DWM
  view.rs                  tata letak + penggambaran semua tampilan (idle/peek/4 widget/tab) + hit target
  gfx.rs                   D3D11/D2D/DWrite, font in-memory, Painter (rrect, text, label mono, icon, arc,
                           droplet, gradient, bitmap)
  theme.rs                 design tokens, ukuran (DIP), glyph Segoe Fluent Icons
  media.rs                 thread GSMTC (MTA): snapshot lagu, cover, perintah play/next/prev
  sysmon.rs                daya, jaringan, volume (COM callback), Raw Input (Caps Lock, aktivitas; mouse hanya
                           saat mata tidur), AppBar fullscreen, suara
  tray.rs                  ikon tray + menu
  editbox.rs               input to-do inline: jendela popup kecil berisi kontrol EDIT standar
  lyrics.rs                thread lirik: folder lirik pengguna → cache (lyrics\cache\<hash>.lrc/.txt/.inst/.none)
                           → LRCLIB /api/search lewat WinHTTP; hasil → WM_APP_LYRICS (Box<Reply>)
  glass.rs                 Glass: kaca untuk jendela tambahan (DesktopWindowTarget di compositor notch,
                           HostBackdropBrush + tint, surface D2D, animasi muncul); prepare_window() untuk DWM
  traymenu.rs              menu klik kanan ikon tray: popup kaca (logo, aksi, sakelar, sholat, bahasa, keluar);
                           sakelar/bahasa tidak menutup menu, klik di luar (WA_INACTIVE) atau Esc menutup.
                           tray::show_menu (menu Win32) hanya cadangan jika popup gagal dibuat
  settings.rs              jendela Pengaturan: kaca Composition sendiri, bingkai sendiri (WM_NCCALCSIZE di
                           settings_wndproc), kontrol digambar sendiri (sakelar, stepper, chip, segmen bahasa),
                           keyboard (Tab/Enter/Spasi/←→/↑↓/Esc)
  launcher.rs              launcher: DropTarget (#[implement(IDropTarget)]), read_items (SHCreateShellItemArray-
                           FromDataObject → path / shell:AppsFolder\AUMID), Icons (IShellItemImageFactory →
                           WIC → PNG di %LOCALAPPDATA%\Topline\icons → ID2D1Bitmap), launch (ShellExecuteEx),
                           reveal (SHOpenFolderAndSelectItems), menu konteks tile
  clip.rs                  clipboard: AddClipboardFormatListener, baca teks (abaikan format rahasia), tulis teks
  watch.rs                 thread pengamat tanpa polling: kamera/mikrofon (RegNotifyChangeKeyValue pada
                           ConsentStore) dan unduhan selesai (ReadDirectoryChangesW di folder Downloads);
                           locate(): lokasi sekali jalan via Geolocator (untuk jadwal sholat)
  util.rs                  Clock lokal, folder data, autostart registry, ikon dari .ico, log debug,
                           GetLastInputInfo, SPI_GETCLIENTAREAANIMATION, flag demo mata
apps/topline-setup/        installer per-user (menyematkan topline.exe via env TOPLINE_EXE)
assets/topline.ico         ikon exe/tray/installer (multi-ukuran 16–256; sama dengan assets/branding/topline.ico)
assets/logo/               logo.png (1024 px, sumber ikon), banner.png (README, 1280×640)
assets/branding/           paket merek: ikon PNG 16–1024, banner 1280×640 dan 1500×500 (gelap/terang), topline.ico
docs/                      ARCHITECTURE.md, DESIGN.md, ROADMAP.md
```

## Alur data (wajib dipahami sebelum mengubah)
1. Event Windows masuk ke `App::on_message`. Callback dari thread lain memakai `PostMessageW(WM_APP_*)`.
2. App memanggil `core.apply(Command, &now)` / `core.system(SystemEvent, &now)` / `core.tick(&now)`, yang mengembalikan `Effects { save, redraw, sound }`.
3. `App::effects()` menyimpan (store) dan memutar suara. Setelah itu `App::refresh()`:
   - `view::draw()` menggambar ke drawing surface dan menghasilkan `Frame { w, h, r, hits, edit, eq }`
   - `notch.set_size()` menganimasikan ukuran lewat DWM
   - `set_region()` mengatur hit-test
   - kolom input to-do ditampilkan atau disembunyikan
   - `schedule()` mengatur timer
4. Timer:
   - `T_CORE`: ke `core.next_deadline_ms()`
   - `T_TICK`: per detik hanya saat Musik diputar atau Fokus berjalan dan widgetnya terbuka; selain itu per pergantian menit
   - `T_HOVER_OPEN` 140 ms, `T_HOVER_CLOSE` 450 ms
   - `T_REGION`: mengecilkan region setelah animasi selesai
   - `T_EYE_DEMO`: hanya saat `TOPLINE_EYE_DEMO=1` dalam mode debug (lihat Verifikasi)
   - `T_LYRIC`: sekali jalan ke pergantian baris lirik berikutnya (hanya saat lirik tampil dan musik diputar)

### Event tambahan
- **Lirik.** `on_media()` → `SystemEvent::Track { key }` (judul+artis) → `Effects.fetch_lyrics` → `lyrics_worker.request()` (thread tidur di `recv`). Hasil: `WM_APP_LYRICS` → `SystemEvent::Lyrics`. Baris aktif dihitung dari `MediaInfo::position_now`; `schedule()` memasang satu timer `T_LYRIC` ke pergantian baris berikutnya hanya saat `core.lyrics_visible()` (tab Lirik terbuka atau caption idle, musik diputar). Klik baris → `MediaCmd::Seek` (`TryChangePlaybackPositionAsync`).
- **Pengaturan.** Tray / ikon roda gigi / `topline settings [bagian]` (`WM_APP_SETTINGS`, WPARAM = indeks bagian) → `App::open_settings_at`. Perubahan lewat `Command::SetToggle/StepNumber/SetTabVisible/SetLanguage`; `App::command` lalu memanggil `sync_settings` (kaca, clipboard listener, bahasa) dan menggambar ulang jendela Pengaturan. Autostart (registry) dan kota (menu `tray::prayer_menu`) ditangani App.
- **Bahasa.** `App::apply_language()` menyelesaikan `Settings.language` (Auto → `GetUserDefaultUILanguage`, 0x21 = Indonesia) ke `core.lang`, dipanggil saat mulai, setelah setiap command, dan saat `WM_SETTINGCHANGE`. Data lama tanpa field `language` dibaca sebagai Indonesia (`legacy_language`), instalasi baru = Auto. Installer memakai bahasa Windows.
- **Drag & drop (OLE).** `run()` memanggil `OleInitialize` (tetap STA, kompatibel dengan DispatcherQueue); `App::new` mendaftarkan `launcher::DropTarget` lewat `RegisterDragDrop`, `shutdown()` memanggil `RevokeDragDrop`. Callback IDropTarget berjalan di UI thread → `app::drop_event()` memakai `try_borrow_mut` pada `APP`; jika App sedang dipinjam, event dikirim ulang lewat `WM_APP_DROP` (LPARAM = `Box<DropEvent>`). Enter → `Command::DragEnter` (notch membuka Launcher; region membesar di `refresh()` sebelum animasi), Leave → `DragLeave` (menutup lagi jika dibuka oleh drag), Drop → `AddPins`. Efek drop selalu LINK (COPY jika sumber tidak mengizinkan LINK), tidak pernah MOVE.
- Launcher: `Effects.launch` → dibuka setelah `refresh()` di `App::command` (`launcher::launch`, gagal → `SystemEvent::LaunchFailed`). Ikon dan keberadaan target hanya dicek di `refresh()` saat widget Launcher tampil. Atur ulang tile: `WM_LBUTTONDOWN` pada `Hit.drag` → `SetCapture`, geser > 6 DIP → `reorder` (pratinjau di view), `WM_LBUTTONUP` → `Command::MovePin`.
- Setiap kali tampilan berubah, `refresh()` menegaskan ulang `HWND_TOPMOST` (aplikasi yang baru dibuka bisa menyusup di atas notch).
- `WM_CLIPBOARDUPDATE` → `clip::read_text` → `SystemEvent::Clipboard`. `Effects.clipboard` → `clip::set_text`.
- `WM_MOUSEWHEEL`/`WM_MOUSEHWHEEL` (koordinat layar → `ScreenToClient` / `scale`) → zona dari `Frame.scroll` → `core.scroll()` → `GestureAction`: volume (`sysmon.set_volume_step`, peek datang dari callback volume), `media.command(Next/Prev)` + peek `Track`, atau pindah tab. `WM_MBUTTONUP` → `core.middle_click()` (play/pause atau `sysmon.toggle_mute`). Hover-open ditahan lewat `core.gestures.hover_blocked(now)` (timestamp, tanpa timer).
- `WM_APP_PRIVACY` (dari thread watch) → `watch::privacy_scan()` di UI thread → `SystemEvent::Privacy`.
- `WM_APP_DOWNLOAD` (LPARAM = `Box<String>`, diambil alih penerima) → `SystemEvent::DownloadDone`. `Effects.reveal` → `explorer /select`.
- `WM_COPYDATA` (dwData `TLPR`, isi `status\nlabel`) dari `topline progress ...` → `SystemEvent::Progress`. Perintah CLI ini tidak pernah menjalankan instance baru.
- Jadwal sholat dihitung ulang saat tanggal/kota berubah (`Core::rollover`), dan waktu berikutnya ikut `next_deadline_ms()`.
- Tray → Jadwal sholat → "Lokasi saat ini" → `watch::locate` (thread MTA, `Geolocator`) → `WM_APP_LOCATION` (LPARAM = `Box<GeoPoint>` atau 0) → `SystemEvent::Location`. `Settings.prayer_location` menggantikan koordinat kota; `prayer_city` berisi kota terdekat untuk label. Memilih kota dari menu menghapus lokasi.

### Mata Personality
- `view::draw()` tidak menggambar mata; `Frame.eyes = Some(core.eye_mood())` saat Personality + Idle. `notch.set_eyes(mood, motion)` hanya memasang animasi saat ekspresi berubah. Kedip, ayunan, napas, dan "z" berupa loop keyframe di DWM.
- Tidur tanpa polling: `schedule()` membaca `GetLastInputInfo` sekali lalu memanggil `core.set_last_input()`; deadline tidur (lastInput + `eye_sleep_minutes`, atau + 1 menit pada 23:00–05:00) ikut `next_deadline_ms()`. Selama tidur, `sys.watch_mouse(true)` mendaftarkan Raw Input mouse. Input pertama → `SystemEvent::UserActivity` → Greeting, lalu registrasi mouse dilepas. Semua `Command` juga membangunkan mata.
- `util::animations_enabled()` (dibaca ulang saat `WM_SETTINGCHANGE`): jika mati, mata statis tanpa loop.

Fitur baru: tambahkan state/logika di **core** (beserta test), `Command`/`SystemEvent` baru jika perlu, gambar di **view.rs**, dan hubungkan event OS di **app.rs/sysmon.rs**.

## Aturan dan konvensi
- **Tanpa polling.** Pakai event OS atau satu deadline timer. Jangan menambah loop/timer berkala yang berjalan saat idle. Setelah perubahan, verifikasi CPU idle tetap ~0 (lihat bagian Verifikasi).
- **Animasi lewat Composition (DWM)**, bukan redraw per frame dari aplikasi.
- **Gambar ulang hanya saat ada perubahan.** `refresh()` menggambar seluruh surface, dan itu murah selama tidak dipanggil terus-menerus.
- Ukuran dalam **DIP** di view/theme. `notch.rs` mengalikan dengan `scale`, karena visual DesktopWindowTarget memakai piksel fisik.
- Notch **tidak boleh mengambil fokus** (`WS_EX_NOACTIVATE`, `MA_NOACTIVATE`). Input teks hanya lewat `editbox.rs`.
- Warna/ukuran/teks mengikuti `docs/DESIGN.md`: hanya satu tombol utama (pill putih, teks navy) per tampilan; teks kecil minimal 11–12 px.
- Teks UI selalu dua bahasa: `lang.t("Indonesia", "English")`. Jangan menulis teks UI satu bahasa.
- Jalankan `cargo test -p topline-core -p topline-store` setelah mengubah core.

## Jebakan windows-rs 0.58 yang sudah ditemui
- **Konstanta yang tidak ter-import di `match` menjadi binding variabel** dan menangkap semua kasus (bug nyata: `WM_MOUSELEAVE`). Selalu periksa warning "unreachable pattern" / "variable should have a snake case name". Konstanta yang didefinisikan manual di app.rs: `WM_MOUSELEAVE = 0x02A3`, `ABN_FULLSCREENAPP = 2`, `EM_LIMITTEXT = 0x00C5`, `EM_SETCUEBANNER = 0x1501`.
- Token event WinRT bertipe `windows::Foundation::EventRegistrationToken` (bukan i64).
- `#[implement]` butuh dependensi `windows-core` dan trait diimplementasikan untuk `Foo_Impl`.
- `CreateMutexW` dan `RegCreateKeyExW` butuh fitur `Win32_Security`.
- `ID2D1Factory1::CreateStrokeStyle` meminta `PROPERTIES1`. Cast ke `ID2D1Factory` untuk versi lama. Untuk metode RenderTarget dari DeviceContext, pakai `let rt: &ID2D1RenderTarget = ctx;`.
- `SetCursor` menerima `HCURSOR` langsung (pakai `.unwrap_or_default()`, bukan `.ok()`).
- `RegisterPowerSettingNotification` menerima `DEVICE_NOTIFY_WINDOW_HANDLE` langsung (tanpa `.0`).

## Verifikasi setelah perubahan
1. `build.ps1 -SkipSetup` harus selesai **tanpa error dan tanpa warning baru**.
2. Jalankan `dist\topline.exe`, lalu coba hover, klik, setiap tab, Win+Alt+T, Win+Alt+N, klik kanan to-do/peek, dan menu tray.
3. CPU idle:
   ```powershell
   $p=Get-Process topline; $c=$p.CPU; sleep 10; $p.Refresh(); $p.CPU-$c
   ```
   Hasilnya harus sekitar 0.
4. Uji fitur v0.1+: pilih kota di tray → Jadwal sholat, lalu buka tab Sholat. Salin teks lalu buka tab Clipboard. Jalankan `dist\topline.exe progress 42 build` lalu `progress done build`. Rename file `x.crdownload` → `x` di Downloads.
   Catatan: saat mengirim pesan ke jendela dari PowerShell, `FindWindow("ToplineNotch", $null)` gagal karena PowerShell mengubah `$null` menjadi string kosong. Pakai judul `"Topline"`. Jangan memakai `Set-Content`/`Get-Content` untuk file sumber (PowerShell 5.1 merusak UTF-8 tanpa BOM).
5. Demo mata: dengan Personality aktif dan mode debug (build debug, atau file `debug` di bawah), jalankan dengan env `TOPLINE_EYE_DEMO=1`. Semua ekspresi akan diputar bergantian setiap 3 detik.
   Untuk menguji mata statis, matikan Settings > Accessibility > Visual effects > Animation effects.
6. Log diagnostik: buat file kosong `%LOCALAPPDATA%\Topline\debug`. Log akan ditulis ke `debug.log` di folder yang sama (lihat `util::log`). Hapus file `debug` itu setelah selesai.
7. Sebelum rilis: `build.ps1` penuh, lalu uji `setup.exe /S /nolaunch` dan copot `/S /uninstall`.

## Data pengguna
`%LOCALAPPDATA%\Topline\data.json` (Persisted: settings, todos, water, next_id). Field baru di struct `Settings` atau `Persisted` harus punya default (`#[serde(default)]` sudah ada), supaya data lama tetap terbaca.

## Lanjutkan dari sini
Lihat `docs/ROADMAP.md`. Prioritas berikutnya: shelf file, penurunan RAM, dan pemilihan monitor.
