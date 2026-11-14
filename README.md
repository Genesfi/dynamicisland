# Dynamic Island (Native Windows 10 & 11 in Rust)

Sebuah aplikasi **Dynamic Island** desktop overlay modern berkinerja tinggi dan ultra-ringan untuk **Windows 10 & 11**, dibangun sepenuhnya menggunakan **Rust murni** dengan **DirectComposition**, **Direct2D Hardware Acceleration**, **DirectWrite**, native **WinRT UserNotificationListener**, **Windows Media Session (GSMTC)**, dan integrasi WebSocket **YTMPX**.

---

## ⚡ Perbandingan & Benchmark

| Metrik | WPF / C# (misal: NabyScape) | Electron / Webview | **Dynamic Island (Rust Native)** |
| :--- | :--- | :--- | :--- |
| **Idle RAM** | 80 MB – 250 MB | 250 MB – 600 MB+ | **~35 MB** (Termasuk DirectX & WinRT) |
| **Idle CPU** | 0.5% – 2.5% | 1.0% – 5.0% | **0.0%** (Event-driven & Sleep Settled) |
| **Ukuran Executable** | 50 MB+ (.NET runtime) | 120 MB+ | **~1.1 MB** (Standalone single `.exe`) |
| **Console Window** | Butuh launcher tambahan | Node wrapper tersembunyi | **Native Windows Subsystem (Tanpa CMD)** |
| **Akselerasi Grafis** | WPF Composition | Chromium GPU Raster | **DirectComposition + Direct2D GPU Direct** |
| **Transparansi & Klik**| Standard Layered Window | Web Transparent Window | **Hardware Premultiplied Alpha + Dynamic GDI HRGN** |
| **Pengaturan (Settings)** | Form XAML berat | HTML / React form | **Fluent UI Direct2D Hardware Accelerated** |

---

## 🌟 Fitur Utama & Keunggulan

### 1. 🛡️ Dynamic Window Clipping Region (Zero Click Blocking)
* Menggunakan pemotongan region OS tingkat kernel (**GDI `SetWindowRgn`**) yang secara dinamis mengikuti bentuk kapsul kapsul secara presisi.
* Area di luar kapsul kapsul **secara fisik tidak ada bagi Windows Window Manager**.
* **Hasil:** Anda dapat mengklik tab browser, tombol bookmark, kolom pencarian, atau breadcrumb File Explorer tepat di samping kapsul tanpa terhalang sedikit pun.
* Disertai bantalan padding 16px dinamis sehingga pancaran efek **Soft Ambient Glow** tetap tampil halus dan tidak terpotong.

### 2. 🎬 Fullscreen Auto-Hide (Netflix, YouTube, Game)
* Otomatis mendeteksi saat ada aplikasi latar depan yang masuk ke mode layar penuh (**Fullscreen**), seperti menonton film di **Netflix**, video **YouTube Fullscreen**, media player (VLC/MPC), maupun bermain **Game**.
* Saat Fullscreen aktif, Island secara mulus meluncur keluar layar (`y: -100px`) dan mematikan opasitasnya (0%).
* Saat keluar dari fullscreen (misal menekan `Esc`), Island meluncur kembali secara elegan tanpa jeda.

### 3. 🧭 Smart Dynamic Contour Avoidance
* **Otomatis Bergeser di Browser:** Saat jendela browser (Chrome, Edge, Brave, Firefox, Opera, Vivaldi) aktif dan menempel di bagian atas layar, Island otomatis meluncur turun sejauh **34px** (`contour_nudge_y`), memberikan ruang penuh bagi tab browser agar tetap terlihat dan mudah diklik.
* **Kembali ke Posisi Atas:** Saat beralih ke **File Explorer**, Desktop, atau aplikasi lain, Island otomatis meluncur kembali ke posisi paling atas (`0px`).
* Jarak pergeseran dapat diatur secara bebas melalui menu Settings.

### 4. 🎵 Kontrol Media Lanjutan & Integrasi YTMPX
* **Dukungan Penuh YouTube Music Desktop (Tauri) & Browser:** Terhubung via WebSocket `ws://127.0.0.1:8765` (YTMPX) dan GSMTC Windows.
* **Buka / Toggle Pemutar Musik:** Mengklik kapsul musik akan langsung memanggil atau memulihkan jendela aplikasi Tauri YTM/Spotify dari system tray secara native.
* **Single-Route & Anti Double-Skip:** Mengatasi bug *double next/prev* dengan merutekan sinyal media secara eksklusif dan dilengkapi filter **debounce hardware 250ms**.
* **Cover Art Cache & Auto-Rehydration:** Gambar album tersimpan di RAM cache (`COVER_CACHE`) sehingga gambar pada piringan vinyl **tidak akan pernah hilang** saat kembali dari mode Fullscreen.

### 5. 🏷️ Deteksi Layanan Streaming & Ikon Vektor Resmi
Mendeteksi secara cerdas berbagai platform media dan menampilkan ikon vektor resmi:
* **Netflix:** Ikon resmi squircle hitam arang dengan pita huruf **"N" merah signature** (`#E50914`).
* **YouTube Music & YouTube:** Ikon play button merah resmi (`#FF0000`).
* **Spotify:** Ikon gelombang suara hijau vibrant (`#1DB954`).
* **Discord, WhatsApp, Telegram:** Ikon resmi dengan palet warna brand asli.
* **Disney+, Prime Video, Twitch, SoundCloud, Bilibili:** Terdeteksi otomatis beserta nama dan warna khasnya.
* **Web Media / Browser Fallback:** Menampilkan nama asli browser (Edge, Chrome, Firefox) jika memutar video web umum.

### 6. 💽 Piringan Vinyl Berputar (Spinning Vinyl Disc)
* Animasi piringan hitam vinyl berputar saat lagu diputar, lengkap dengan:
  * 4 alur lingkaran konsentris (*microgrooves*).
  * 2 berkas kilau cahaya specular anisotropik 180 derajat.
  * Gambar album asli berputar di tengah piringan dengan lubang poros (*spindle hole*).
* **WASAPI Audio Metering:** Visualizer bar equalizer dinamis yang bereaksi terhadap dentuman audio nyata dari kartu suara PC Anda.

### 7. ⏱️ Hover-to-Expand dengan Dwell Time
* Untuk mencegah Island melebar tanpa sengaja saat kursor mouse sekilas melintasi tepi atas layar, diterapkan **hover dwell delay 180ms**.
* Island hanya akan melebar menampilkan kontrol penuh jika kursor diarahkan secara sengaja.

### 8. ⚙️ Panel Pengaturan Fluent UI Direct2D
Klik kanan ikon di tray bar pojok kanan bawah $\rightarrow$ pilih **⚙ Settings...** (atau double click ikon tray):
* **Slider Interaktif:**
  * Jarak dari Atas Layar (*Top Margin*).
  * Jarak Nudge Kontur Browser (*Browser Contour Nudge*).
  * Lebar Kapsul Musik (*Compact Music Width*).
  * Durasi Tampil Notifikasi (*Display Duration*).
  * Opasitas Island (*Island Opacity*).
  * Intensitas Ambient Glow (*Glow Intensity*).
  * Kecepatan Putaran Vinyl (*Vinyl Spin Speed*).
* **Toggle Sakelar:**
  * Dynamic Album Glow Color (Warna glow menyala sesuai warna dominan album).
  * Enable Media Island & Vinyl Animation.
  * Hover Expand Media.
  * Idle Click-Through.
  * Smart Dynamic Contour.
  * Run on Windows Startup (Autostart).
* **Pilihan Gaya Tampilan:**
  * *Modern Minimal*, *Sleek Capsule*, *Notch Dynamic*.
  * Tampilan Compact: *Combined (Title + Wave)*, *Title Only*, *Wave Only*.
* **Auto-Save Instant:** Setiap perubahan langsung disimpan ke `config.txt` dan diperbarui secara instan tanpa perlu restart.

---

## 🚀 Cara Menjalankan

### Menjalankan Binary Langsung:
Jalankan file executable hasil kompilasi:
> [`target/release/dynamic_island.exe`](file:///f:/Native%20Win/DynamicNotification/target/release/dynamic_island.exe)

### Kompilasi dari Source Code:
Pastikan Rust toolchain sudah terpasang di komputer Anda:
```powershell
# Jalankan mode release
cargo run --release

# Atau kompilasi binary mandiri
cargo build --release
```

---

## ⌨️ Pintasan & Interaksi

| Aksi | Fungsi |
| :--- | :--- |
| **Arahkan Kursor (>180ms)** | Membuka kontrol media penuh (Play/Pause, Prev, Next, Waveform) |
| **Klik Tombol Prev / Next** | Pindah lagu (dilengkapi proteksi debounce anti double-skip) |
| **Klik Tombol Play / Pause**| Menghentikan sementara atau melanjutkan pemutaran lagu |
| **Klik Permukaan Kapsul**   | Membuka/memulihkan aplikasi pemutar musik (Tauri YTM / Spotify / Browser) |
| **Klik di Luar Kapsul**     | Menutup panel ekspansi kembali ke bentuk kapsul ramping |
| **Klik Kanan Ikon Tray**    | Membuka menu konteks (Settings, Autostart, Exit) |
| **Tekan `Esc`**             | Menutup ekspansi Island secara halus |

---

## 📄 Lisensi
Dilisensikan di bawah **MIT License**. Bebas dimodifikasi, didistribusikan, dan digunakan untuk proyek pribadi maupun komersial.
