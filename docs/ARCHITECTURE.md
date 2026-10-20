# Topline — Arsitektur

> Status: **v0.1 terimplementasi** · 20 Okt 2026 · Rust native (Win32 + Windows.UI.Composition + Direct2D/DirectWrite)

## Status v0.1 (diuji di Windows 11 25H2, 1920x1080, skala 100%)
| Item | Hasil |
|---|---|
| CPU saat idle | 0,000 detik per 10 detik |
| RAM (working set) | sekitar 66–84 MB (target < 30–40 MB belum tercapai) |
| Ukuran | topline.exe 820 KB · setup.exe 1,26 MB |
| Kaca | HostBackdropBrush berfungsi + tint rgba(8,12,30,.45) |
| Installer | Uji pasang/copot silent lolos; data pengguna tetap tersimpan |

## Keputusan
| # | Keputusan |
|---|---|
| 1 | Rust native + Windows Composition. Tanpa WebView. Logika produk di `topline-core`, terpisah dari UI. |
| 2 | Autostart lewat `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` (tanpa admin) |
| 3 | Hotkey `RegisterHotKey`: Win+Alt+T (buka/tutup), Win+Alt+N (tambah tugas) |
| 4 | Per-Monitor DPI Aware v2 (manifest + `SetProcessDpiAwarenessContext`) |
| 5 | Offline-first. Satu-satunya akses jaringan: lirik dari LRCLIB (aktif secara default atas keputusan pemilik, bisa dimatikan; hanya judul, artis, durasi; di-cache). File `.lrc` pengguna dipakai lebih dulu |
| 11 | Dua bahasa UI (Indonesia, Inggris), teks berpasangan di tempat dipakai (`Lang::t`). Auto mengikuti bahasa tampilan Windows |
| 6 | Hanya Windows 11 (22H2 ke atas) |
| 7 | Input to-do inline lewat kontrol EDIT standar di jendela popup kecil di atas baris; notch tidak pernah mengambil fokus |
| 8 | Installer per-user buatan sendiri (Rust) ke `%LOCALAPPDATA%\Programs\Topline` |
| 9 | Data di satu file JSON dengan tulis atomik: `%LOCALAPPDATA%\Topline\data.json` |
| 10 | Toolchain `x86_64-pc-windows-gnu` lokal di `.toolchain\`; ikon/manifest via `windres` (MinGW-w64) |

## Prinsip performa
- Tidak ada polling.
- Konten digambar ulang hanya saat ada perubahan.
- Animasi (ukuran, clip, fade, equalizer) dijalankan oleh DWM.
- Satu timer core ke deadline terdekat (Pomodoro, pengingat air, to-do, kedaluwarsa peek, ekspresi mata sementara, waktu tidur mata, pergantian hari).
- Ticker UI per detik hanya saat widget Musik (sedang diputar) atau Fokus (berjalan) terbuka; selain itu per menit.

## Rendering
```
Win32 window (WS_EX_NOREDIRECTIONBITMAP | TOOLWINDOW | TOPMOST | NOACTIVATE), 640x540 DIP tetap,
di tengah atas monitor utama. Hit-test dibatasi SetWindowRgn ke ukuran notch saat ini.
 └─ DesktopWindowTarget
     └─ root
         └─ container (AnchorPoint 0.5,0 · Size dianimasikan · clip rounded-rect digeser ke atas
             │          sebesar radius agar sudut atas tetap siku)
             ├─ backdrop  HostBackdropBrush   (butuh DWMWA_USE_HOSTBACKDROPBRUSH)
             ├─ tint      rgba(8,12,30,.45)   (mode tanpa kaca: rgba(11,22,96,.92), backdrop disembunyikan)
             ├─ content   CompositionDrawingSurface 640x540 ← Direct2D/DirectWrite; Offset.X mengikuti
             │            container lewat ExpressionAnimation (selalu di tengah)
             ├─ eq        4 SpriteVisual, Scale.Y dianimasikan tanpa henti oleh DWM (idle + musik diputar)
             └─ eyes      mata Personality: root (Offset ikut tengah container) → look (lirikan) → sway
                          (ayunan/pantulan) → 2 ShapeVisual (Scale.Y = kedip/napas), masing-masing berisi 2
                          sprite shape rounded-rect (bertumpuk; diputar ±35° untuk "^"), + 3 visual "z"
```
- Region membesar segera saat notch melebar, dan mengecil setelah animasi selesai (timer `T_REGION`).
- Satuan visual = piksel fisik (`scale` = DPI/96). Penggambaran Direct2D memakai `SetDpi(96*scale)` sehingga view menulis dalam DIP.

## Proses dan thread
```
UI thread (STA, DispatcherQueueController, GetMessage loop)
 ├─ App (app.rs): wndproc notch + jendela host tray, timer, hover, hotkey
 ├─ Core (topline-core): state, dipanggil sinkron
 ├─ Notch/Gfx/View: Composition + D2D
 ├─ EditBox: popup + EDIT (subclass: Enter/Esc/blur → PostMessage)
 ├─ SysMon: daya (WM_POWERBROADCAST), Raw Input (WM_INPUT), AppBar
 ├─ Launcher: IDropTarget (OleInitialize + RegisterDragDrop), ikon shell, ShellExecuteEx
 ├─ Jendela Pengaturan: DesktopWindowTarget kedua (compositor yang sama), dibuat saat dibuka
 └─ Menu tray: popup kaca (glass.rs) yang sama, dibuat saat klik kanan dan dihancurkan saat ditutup
Thread lirik: tidur di recv() → folder/cache/LRCLIB (WinHTTP, HTTPS) → PostMessage(WM_APP_LYRICS)
Thread media (MTA): GSMTC manager + event sesi → snapshot di Mutex → PostMessage(WM_APP_MEDIA)
Callback threadpool: NetworkStatusChanged → WM_APP_NET; IAudioEndpointVolumeCallback → WM_APP_VOLUME
Thread privacy: tidur di WaitForMultipleObjects (event registry + stop) → WM_APP_PRIVACY
Thread downloads: tidur di ReadDirectoryChangesW → WM_APP_DOWNLOAD
```

## Event sistem
| Sumber | API |
|---|---|
| Media | `GlobalSystemMediaTransportControlsSessionManager` |
| Daya | `RegisterPowerSettingNotification` + `GetSystemPowerStatus` |
| Jaringan | `NetworkInformation.NetworkStatusChanged` + `GetInternetConnectionProfile` |
| Volume | `IAudioEndpointVolumeCallback` |
| Caps Lock | Raw Input `RIDEV_INPUTSINK` (status dilacak sendiri dari tekan tombol) |
| Full-screen | AppBar `ABN_FULLSCREENAPP` (notch disembunyikan) |
| Input terakhir (mata tidur) | `GetLastInputInfo` dibaca setiap penjadwalan; Raw Input mouse (`RIDEV_INPUTSINK`) hanya selama tidur, dilepas (`RIDEV_REMOVE`) setelah bangun |
| Efek animasi | `SPI_GETCLIENTAREAANIMATION`, dibaca ulang saat `WM_SETTINGCHANGE` |
| Clipboard | `AddClipboardFormatListener` → `WM_CLIPBOARDUPDATE`; format `ExcludeClipboardContentFromMonitorProcessing`, `Clipboard Viewer Ignore`, dan `CanIncludeInClipboardHistory = 0` diabaikan |
| Kamera/mikrofon | Thread `privacy`: `RegNotifyChangeKeyValue` pada `HKCU\...\CapabilityAccessManager\ConsentStore\{webcam,microphone}` (subtree). `LastUsedTimeStop == 0` = sedang dipakai |
| Unduhan | Thread `downloads`: `ReadDirectoryChangesW` (FILE_NAME) di folder Downloads; rename dari `.crdownload/.part/.partial/.download/.opdownload` ke nama akhir = selesai |
| Drag & drop ke notch | OLE `IDropTarget` di jendela notch; item dibaca dengan `SHCreateShellItemArrayFromDataObject` (CF_HDROP dan Shell IDList Array, termasuk aplikasi Store dari Start). Hanya referensi yang disimpan (`Persisted.pins`) |
| Lirik | Lagu baru dari GSMTC → thread lirik; `GET https://lrclib.net/api/search` (User-Agent Topline/versi). Pergantian baris lewat satu timer deadline |
| Bahasa Windows | `GetUserDefaultUILanguage` (saat mulai dan `WM_SETTINGCHANGE`) untuk `Language::Auto` |
| Progress | `topline progress ...` mengirim `WM_COPYDATA` ke instance yang berjalan |
| Lokasi (jadwal sholat) | `Geolocator.GetGeopositionAsync` sekali jalan saat pengguna memilih "Lokasi saat ini"; zona waktu dari `GetTimeZoneInformation`. Tanpa pelacakan berkala |
| Gestur scroll | `WM_MOUSEWHEEL` / `WM_MOUSEHWHEEL` / `WM_MBUTTONUP` di jendela notch (Windows mengirim roda ke jendela di bawah kursor). Volume diubah lewat `IAudioEndpointVolume`; objek dibuat ulang jika perangkat default berganti (dicek hanya saat gestur) |
| Suara | `PlaySound` alias `Notification.Reminder` / `Notification.Default` |

## Model core
- `View`: `Idle` | `Peek` | `Expanded(Widget)`; `Widget`: Music, Lyrics, Focus, Water, Todo, Launcher, Prayer, Clipboard. `Core::tabs()` menyembunyikan Sholat (tanpa kota), Clipboard (riwayat nonaktif), dan tab di `Settings.hidden_tabs` (Musik selalu tampil).
- Jadwal sholat (`prayer.rs`): posisi matahari (algoritme praytimes.org), Subuh -20°, Isya -18°, Ashar Syafi'i, ihtiyath +2 menit, Imsak = Subuh - 10 menit. Hanya waktu terakhir yang lewat yang diingatkan (mis. setelah sleep).
- `Command`: Toggle, Open, Close, Switch, FocusStartPause/Reset/Skip, AddWater, UndoWater, ToggleTodo, DeleteTodo, StartAddTodo, SubmitTodo, CancelAddTodo, SetPersonality, SetOpenOnHover, SetScrollGestures, ActivatePeek, DismissPeek, DragEnter, DragLeave, AddPins, RemovePin, MovePin, LaunchPin, AskRemovePin, SetToggle, StepNumber, SetTabVisible, SetLanguage. Gestur scroll lewat `Core::scroll()` / `Core::middle_click()` (bukan Command) yang mengembalikan `GestureAction`.
- `SystemEvent`: Volume, Power, Network, CapsLock, Media, UserActivity, Clipboard, Privacy, Progress, DownloadDone, Location, LaunchFailed, Track, Lyrics. Event pertama Power/Network hanya mencatat status awal, tanpa peek.
- `EyeMood` (Happy > Greeting > Sleeping > LowBattery > Focus > Music > Normal) dihitung oleh `Core::eye_mood()`. Happy/Greeting punya waktu kedaluwarsa, dan waktu tidur dihitung dari `set_last_input()`; ketiganya masuk ke `next_deadline_ms()` (hanya saat Personality aktif), jadi tidak ada timer tambahan.
- Peek: prioritas 1–5. Peek berprioritas lebih tinggi menggantikan yang sedang tampil. Saat notch terbuka, hanya peek prioritas ≥ 4 yang disimpan di antrean untuk ditampilkan nanti.
- `Effects { save, redraw, sound, clipboard, reveal, launch, fetch_lyrics }` dikembalikan ke App.
- Lirik: `LyricsState` (NoMedia, Loading, Ready, NotFound, Offline, Failed). Hasil untuk lagu lama diabaikan (kunci judul+artis).

## Pertanyaan terbuka
- Distribusi: MSIX/Store atau installer biasa + code signing?
- Shelf (seret keluar): launcher sudah memakai referensi path; shelf mengikuti keputusan yang sama kecuali diputuskan lain.
- Cara menurunkan RAM.
