# Topline — Roadmap dan Backlog

Kriteria umum untuk setiap item: CPU idle tetap ~0, tanpa polling, build tanpa warning baru, dan test core ditambah jika ada logika baru.

## v0.2 (prioritas)
1. **Lirik tersinkron (offline-first)**
   - Sumber berurutan: tag `SYLT`/`USLT` file lagu → `.lrc` di sebelah file → cache lokal → opsi "Unduh lirik online" dari LRCLIB (**nonaktif secara default**, hasil di-cache selamanya). Jika LRCLIB dipakai, yang dikirim hanya judul, artis, dan durasi.
   - Catatan: GSMTC tidak memberikan path file. Untuk pemutar lokal, cari berdasarkan judul/artis di folder Musik pengguna (opsional), atau andalkan cache/opsi online.
   - UI: widget/tab "Lirik" baru (lihat prototipe: notch terbelah, baris aktif putih 20 px, baris lain 62%, klik baris untuk seek via `TryChangePlaybackPositionAsync`).
   - Parser `.lrc` dan pemilihan baris aktif ditaruh di crate baru `topline-lyrics` (Rust murni) beserta test.
2. **Shelf file**
   - `IDropTarget` dan OLE init sudah ada (launcher). Shelf = area sementara terpisah dari sematan, simpan referensi path (keputusan default), kartu 116 px. Klik membuka (`ShellExecuteW`), seret keluar via `IDropSource` + `IDataObject` (CF_HDROP), klik kanan untuk menghapus.
3. **Jendela Pengaturan** (native, lihat artboard "Pengaturan" di prototipe)
   - Opsi: mulai bersama Windows, monitor, menyingkir saat full-screen, cara membuka, Personality, widget aktif, target dan interval air minum, durasi fokus/istirahat, peek sistem.
   - Semua field sudah ada di `Settings`, kecuali pemilihan monitor dan widget aktif.
4. **Turunkan RAM** (sekarang 66–84 MB, target < 40 MB)
   - Profil: perangkat D3D hardware vs WARP, `res_ctx`, font collection, cache text format, WinRT.
   - Coba `SetProcessWorkingSetSize` setelah idle, serta melepas cover bitmap saat tidak ada media.

## v0.3
- Baterai headset Bluetooth (DeviceWatcher + properti baterai); peek saat earbuds tersambung.
- Jadwal sholat: pengingat X menit sebelum waktu, hitung mundur di pill idle.
- Riwayat clipboard: opsi gambar dan opsi simpan ke disk.
- Pemilihan monitor + perpindahan saat monitor berubah (`WM_DISPLAYCHANGE` sudah ditangani untuk monitor utama).
- Drop shadow notch (`DropShadow` dengan mask rounded-rect).
- Aksesibilitas: UI Automation provider untuk notch (tombol/tab yang bisa dibaca pembaca layar), plus navigasi keyboard saat notch dibuka via hotkey.
- Animasi pegas (`SpringVector2NaturalMotionAnimation`) untuk buka/tutup, dan indikator tab yang bergeser.

## Selesai setelah v0.1
- 🚀 Launcher mini (tab kelima): seret aplikasi (.exe, .lnk, aplikasi Store dari Start), file, atau folder ke notch dalam keadaan apa pun. Maks. 8 sematan, duplikat diabaikan, hanya referensi (efek drop LINK). Klik membuka (ShellExecuteEx), seret tile untuk mengatur ulang, klik kanan: buka lokasi file / jalankan sebagai administrator / lepas sematan. Target hilang tampil redup. Ikon di-cache sebagai PNG. RAM +~3 MB dengan 8 sematan.
- 🖱️ Gestur scroll di notch: scroll vertikal = volume (2% per takik, delta touchpad diakumulasi), horizontal / Shift + scroll = lagu berikutnya/sebelumnya (cooldown 450 ms), scroll di bar tab = pindah widget, klik tengah = play/pause (atau mute). Hover tidak membuka notch selama 1,5 s sejak scroll. Peek `Track` dengan judul lagu baru. Bisa dimatikan dari tray.
- Mata Personality hidup: kedip, 7 ekspresi (`EyeMood`), tidur tanpa polling (deadline core + Raw Input mouse hanya saat tidur), lirikan ke kursor, menghormati "Animation effects" Windows. Semuanya visual DWM, CPU idle tetap ~0. Demo: `TOPLINE_EYE_DEMO=1` (mode debug).
- Ikon dan banner baru (`assets/logo/logo.png`, `assets/logo/banner.png`, paket lengkap di `assets/branding/`).
- 🕌 Jadwal sholat offline (metode Kemenag, 44 kota per wilayah dari menu tray) dengan widget "Sholat", peek saat waktunya tiba, dan suara adzan opsional (`adzan.wav` di folder data). Opsi "Lokasi saat ini" memakai `Geolocator` Windows (sekali jalan, atas permintaan) dan memberi label kota terdekat. Diuji terhadap jadwal resmi Jakarta dan Surabaya (selisih ≤ 2 menit).
- 🎙️ Indikator kamera (titik mint) dan mikrofon (titik amber) di pill idle, dengan peek "Kamera/Mikrofon dipakai · <aplikasi>". Sumbernya ConsentStore, dipantau dengan `RegNotifyChangeKeyValue`.
- 📋 Riwayat clipboard (teks, hanya di memori, 20 entri + yang disematkan) dengan widget "Clipboard": klik untuk menyalin ulang, ikon pin untuk menyematkan, klik kanan untuk menghapus. Format rahasia dari pengelola sandi diabaikan.
- ⬇️ `topline progress <start|0-100|done|fail> [label]` untuk melaporkan progress build/skrip, serta peek "Unduhan selesai" dari folder Downloads (`ReadDirectoryChangesW`; klik untuk membukanya di Explorer).

## Backlog fitur baru (usulan, belum dijadwalkan)
Urutan di bawah mengikuti kecocokan dengan prinsip Topline (offline, tanpa polling). Item bertanda ⚠ bertentangan dengan prinsip itu dan perlu diputuskan dulu.

1. 🔔 **Notifikasi WhatsApp/Telegram singkat di notch.** ⚠
   - `UserNotificationListener` (WinRT) butuh izin pengguna dan **identitas paket** (MSIX/sparse package). Ini terkait keputusan MSIX di bagian Distribusi.
   - Isi pesan bersifat pribadi: default hanya nama aplikasi + pengirim, isi pesan opsional.
2. 📅 **Rapat berikutnya dari Google Calendar.** ⚠ Butuh jaringan + OAuth, jadi bertentangan dengan "full offline".
   - Alternatif yang sesuai prinsip: baca kalender lokal Windows (`Windows.ApplicationModel.Appointments`, juga butuh identitas paket) atau URL `.ics` yang diunduh sesekali (opsional, nonaktif secara default, seperti LRCLIB).
   - Pengingat berikutnya menjadi deadline core, dan peek muncul 5 menit sebelum rapat.
3. 💬 **Chat AI assistant (pakai API key sendiri).** ⚠ Butuh jaringan, jadi fitur ini opsional dan **nonaktif secara default**.
   - **Cakupan: hanya chat** tanya-jawab teks. **Tidak agentic**: tanpa tool use/function calling, tanpa akses file, layar, atau clipboard otomatis, dan tanpa menjalankan perintah.
   - Penyedia dipilih pengguna dengan API key masing-masing: Anthropic (Claude), OpenAI, Google Gemini, OpenRouter, dan endpoint kompatibel OpenAI (termasuk Ollama/LM Studio lokal, yang tetap offline).
   - API key disimpan di Windows Credential Manager / DPAPI (`CredWrite` atau `CryptProtectData`), **bukan** di `data.json`.
   - Widget/tab "Chat" baru: input teks lewat `editbox.rs` (notch tetap tidak mengambil fokus), jawaban di-stream (SSE lewat WinHTTP di thread terpisah → `PostMessageW(WM_APP_*)`), riwayat percakapan sesi saat ini saja secara default (opsi simpan lokal), tombol Salin dan Hentikan. Hotkey opsional untuk membuka chat.
   - Logika percakapan (daftar pesan, status streaming, batas riwayat) di core/crate Rust murni beserta test; HTTP dan penyimpanan kredensial di lapisan aplikasi. Tanpa koneksi saat idle, dan request hanya dikirim saat pengguna menekan Kirim.
   - Pengaturan: penyedia, model, API key, system prompt singkat, batas token. Bergantung pada jendela Pengaturan (v0.2).

## Distribusi
- Code signing untuk setup.exe dan topline.exe (agar tidak diperingatkan SmartScreen).
- Opsi MSIX / Microsoft Store (belum diputuskan).
- Auto-update **tidak** dipakai (offline). Update dilakukan manual lewat setup.exe baru, yang otomatis menutup versi lama.

## Bug / perbaikan kecil yang diketahui
- Ikon glyph Segoe Fluent tertentu (`G_TODO` E9D5, `G_BELL` EA8F, `G_KEYBOARD` E92E, `G_BOLT` E945) belum diverifikasi visual di semua state. Periksa di peek dan tab.
- Tombol next/prev di widget Musik belum diberi umpan balik visual instan (gestur scroll sudah punya peek `Track`; play/pause sudah optimistis).
- Saat DPI berubah antar monitor, posisi kolom EDIT dihitung ulang, tapi belum diuji di monitor kedua.
- `README.md` dan `AGENTS.md` perlu diperbarui setiap kali fitur bertambah.
