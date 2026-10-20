# Topline — Spesifikasi Visual

Sumber: prototipe kanvas "Topline — Windows Notch Prototype" (artifact Claude milik pemilik project) dan design.md pemilik (glassmorphism, palet **A · Electric Ultramarine**). Nilai di bawah sudah diterapkan di `apps/topline/src/theme.rs` dan `view.rs`.

## Warna
| Token | Nilai | Pemakaian |
|---|---|---|
| navy | `#0B1660` | teks di atas tombol/chip putih |
| ultramarine | `#0A4BF0` | gradient |
| electric | `#2271E9` | centang to-do, aksen interaktif |
| sky | `#40A9FD` | progress air, ring fokus, equalizer, ikon aktif |
| ice | `#F2F6FF` | teks utama, tombol utama |
| mint | `#43BA95` | status "sedang diputar", charging, istirahat |
| amber | `#F5B544` | peringatan (Wi-Fi putus, baterai lemah); hanya untuk aksen kecil |
| text-2 | ice 72% | deskripsi |
| text-3 | ice 62% | metadata, ikon tab tidak aktif |

**Kaca notch:** HostBackdropBrush + tint `rgba(8,12,30,.45)`, garis tepi `rgba(255,255,255,.14)`. Mode tanpa kaca: `rgba(11,22,96,.92)`.
**Permukaan dalam (kartu, tombol sekunder):** isi `rgba(255,255,255,.05–.06)`, border `.10–.14`. Hover: isi `.14`.
**Baris to-do yang sedang diketik:** `#141A2E` pekat (sama dengan latar kontrol EDIT), border sky 70%, glow electric 18%.

## Tipografi
- Geist untuk teks, Geist Mono untuk label (huruf kapital, tracking 0.06em, 11–12 px). Ikon memakai Segoe Fluent Icons.
- Judul widget 18 / 500 · teks 13–15 / 400 · angka besar (timer, liter) 48–56 / 400 tabular.

## Ukuran (DIP)
| Bagian | Nilai |
|---|---|
| Jendela visual (tetap) | 640 × 540 |
| Idle | 124 × 36 (hanya jam/mata), 208 × 36 (dengan musik/fokus), radius bawah 18 |
| Idle + caption lirik | 420 × 60: isi pill tetap di tengah atas, baris lirik 13/500 ice 92% di bawahnya ("♪" saat jeda) |
| Peek | 340 × 44, radius 22 |
| Expanded | lebar 592, radius 28 |
| Tata letak expanded | padding samping 20, atas 12; header 40; jarak 16; tab 48; bawah 14 |
| Tinggi isi widget | Musik 164 · Fokus 120 · Air 168 · To-do 28 + 4 + baris × 44 + 6 + 44 (maks. 6 baris) · Sholat 168 · Clipboard 28 + 4 + baris × 44 (maks. 6) · Launcher kosong 120, 1 baris 104, 2 baris 220 · Lirik 188 |
| Jendela Pengaturan | 760 × 560, menu samping 212 (item 40, pill 20), isi x 236, baris 60 dalam kartu `.05`/`.10` radius 16, sakelar 44 × 24 (sky saat aktif), stepper bulat 32, tombol kaca 32, chip tab 34, segmen bahasa (aktif: pill putih teks navy). Tint kaca rgba(9,13,34,.77); mode tanpa kaca navy pekat |
| Tile launcher | (552 − 3 × 12) / 4 = 129 × 104, jarak 12, radius 20, ikon 48 di tengah atas (y + 14), nama 12 px satu baris dengan elipsis |
| Tombol | tinggi 44 (pill), ikon bulat 40–52 |
| Tab | pill wadah 48, item 52 × 40, indikator aktif `.14` |

Animasi: cubic-bezier(0.22, 1, 0.36, 1), 420 ms untuk ukuran, fade isi 260 ms (jeda 60 ms).

## Komponen per tampilan
- **Idle:** jam di tengah. Kiri: cover mini 20 px (musik) atau ikon timer (fokus berjalan). Kanan: equalizer 4 batang (DWM) saat musik diputar, atau sisa menit fokus "18m". Mode Personality: dua mata (lihat di bawah).
- **Idle → Personality (mata):** dua mata 8 × 13 DIP, radius 4, jarak pusat ke pusat 18 DIP, di tengah pill 124 × 36, warna ice. Mata adalah visual Composition (`ShapeVisual` + rounded-rect geometry di `notch.rs`), dan semua gerakan dijalankan DWM. Transisi antar-ekspresi 320 ms (menutup ke tidur 800 ms) dengan easing standar. Ekspresi menurut prioritas (`EyeMood` di core):

  | Ekspresi | Pemicu | Bentuk |
  |---|---|---|
  | Happy (2,5 s) | tugas dicentang, target air tercapai, sesi fokus selesai | "^ ^" (dua batang 2,4 × 8 diputar ±35°), memantul 2 DIP sekali |
  | Greeting (1,2 s) | input pertama setelah mata tidur, atau notch dibuka | membesar 1,15×, memantul, kedip ganda cepat |
  | Sleeping | 5 menit tanpa input (`eye_sleep_minutes`), atau 1 menit pada 23:00–05:00 | garis 9 × 2 radius 1, napas Scale.Y 1 ↔ 1,4 per 4 s, tiga "z" naik dan memudar |
  | LowBattery | baterai ≤ 20% dan tidak mengisi daya | 8 × 8 lebih rendah (kelopak turun), amber, kedip lambat per 11,3 s |
  | Focus | Pomodoro fase Fokus berjalan | 9 × 7, sky, kedip jarang per 12,7 s |
  | Music | musik diputar | normal + ayunan X ±1,5 DIP per 1,2 s |
  | Normal | bawaan | kedip alami: satu timeline 7,3 s (kedip di 0,42, kedip ganda di 0,86/0,88), 1 → 0,1 → 1 dalam 140 ms |

  Saat kursor di atas notch, mata melirik ke arah kursor (maks ±2 DIP horizontal, ±1 DIP vertikal), dan kembali ke tengah saat kursor pergi (kecuali saat tidur). Jika "Animation effects" Windows dimatikan, bentuk ekspresi tetap ditampilkan, tapi tanpa kedip, ayunan, pantulan, lirikan, atau "z".
- **Header expanded:** label tanggal mono di kiri ("SEL · 20 OKT" / "TUE · OCT 20"), jam di tengah, ikon roda gigi (Pengaturan, tanpa lingkaran sampai di-hover) dan tombol tutup bulat chevron di kanan.
- **Lirik:** label "Judul · Artis" 12/500 text-2 dan sumber mono ("LRCLIB" / "FILE LOKAL"); lima baris 32 px: aktif 20/500 putih di tengah, lainnya 15 text-3; hover isi `.06`. Pesan keadaan (mencari, tidak ditemukan, online mati, gagal) 18/500 + keterangan 13 text-2. Ikon tab: tiga garis rrect.
- **Bahasa:** semua teks punya versi Indonesia dan Inggris dengan panjang mirip; label mono tetap kapital.
- **Musik:** cover 96 px radius 20, pill status (mint saat diputar), judul, artis, progress dengan waktu mono, tombol prev / play (putih 52 px) / next.
- **Fokus:** ring 112 px (sky saat fokus, mint saat istirahat), label "SESI n / 4", waktu 56 px, segmen "Fokus 25 | Istirahat 5", tombol utama Mulai/Jeda/Lanjut, sekunder Ulang/Lewati.
- **Air minum:** label, angka besar "1,25 / 2,0 L", bar gradient electric→sky, keterangan pengingat berikutnya, tombol "+250 ml" (utama), "+500 ml", undo; kartu grafik 8 jam terakhir.
- **To-do:** "Hari ini" + "n / m SELESAI"; baris 44 px dengan lingkaran centang, teks (dicoret saat selesai), jam mono; baris "Tambah tugas" bergaris putus-putus dengan hint "WIN + ALT + N"; saat mengetik, baris berubah jadi kolom pekat.
- **Sholat:** label "Sholat berikutnya · Kota", nama waktu 18/500, jam 48 px, "dalam 1 j 12 mnt", tombol kaca "Suara adzan aktif/mati". Kartu kanan 220 × 168 berisi 7 baris 22 px (Imsak–Isya); waktu berikutnya disorot sky 18%, yang lewat text-3.
- **Clipboard:** "Riwayat clipboard" + "HAPUS SEMUA" (label mono), baris 44 px: ikon tempel (centang mint + "DISALIN" setelah disalin), teks satu baris dengan elipsis, tombol pin 32 px (sky saat disematkan). Klik kanan menghapus.
- **Indikator privasi (idle):** titik 6 px di ujung kanan pill, mint = kamera, amber = mikrofon.
- **Launcher:** grid 4 × 2. Tile isi `.05`, border `.10`, hover `.14`. Target hilang: ikon 35%, nama text-3, label mono "TIDAK DITEMUKAN" amber 80%; klik → tile berubah jadi "Lepas sematan?" dengan pill putih "Lepas" + kaca "Batal". Tile yang diseret: isi `.14`, border sky 1,5. Keadaan kosong: zona putus-putus putih 26% + ikon kotak 2 × 2 + "Seret aplikasi, file, atau folder ke sini." Saat drag OLE di atas notch: lapisan navy pekat + sky 12% dengan garis putus-putus sky dan teks "Lepaskan untuk menyematkan" (amber + "Maksimal 8..." jika penuh). Pesan batas sematan menggantikan label tanggal di header selama 3,5 s (amber, 12 px).
- **Tab:** ikon Launcher = kotak 2 × 2 digambar dengan rrect (bukan glyph), ikon Sholat = bulan sabit (E708), Clipboard = tempel (E77F). Tab hanya tampil jika fiturnya aktif.
- **Peek:** ikon di kiri, teks, nilai di kanan. Pengingat minum punya tombol putih "+250 ml". Peek baru: waktu sholat (bulan, sky), kamera/mikrofon dipakai (mint/amber), progress (bar sky + persen, atau "BERJALAN"), selesai/gagal (centang mint / silang amber), unduhan selesai (panah unduh), gagal membuka sematan (silang amber, "Tidak bisa membuka <nama>"), ganti lagu dari gestur scroll (ikon next/prev sky + judul lagu baru, atau "Lagu berikutnya"/"Lagu sebelumnya" sampai judul tersedia; 1,6 s, prioritas sama dengan volume).
