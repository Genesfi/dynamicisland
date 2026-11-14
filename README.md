# Dynamic Island (Native Windows in Rust)

A high-performance, ultra-lightweight **Dynamic Island desktop overlay for Windows 10 & 11**, built in pure **Rust** with **DirectComposition**, **Direct2D**, **DirectWrite**, native **WinRT UserNotificationListener**, and **Windows Media Transport Controls (GSMTC)**.

---

## ⚡ Highlights

| Metric | WPF / C# (e.g. NabyScape) | Electron / Webview | **Dynamic Island (Rust)** |
| :--- | :--- | :--- | :--- |
| **Idle RAM** | 80 MB – 250 MB | 250 MB – 500 MB+ | **~6 MB – 12 MB** |
| **Idle CPU** | 0.5% – 2.5% | 1.0% – 5.0% | **0.0%** (Event-driven) |
| **Executable Size** | 50 MB+ (.NET runtime) | 120 MB+ | **~400 KB** (Standalone .exe) |
| **Console Window** | Terminal required or C# launcher | Hidden via node wrapper | **Native Windows Subsystem (Tanpa Jendela CMD)** |
| **Rendering** | WPF Composition | Chromium GPU raster | **DirectComposition + Direct2D (GPU Direct)** |
| **Transparency** | Standard Layered | Web transparent window | **Hardware Premultiplied Alpha + Click-Through** |
| **Settings UI** | Heavy XAML window | HTML form | **Native Win32 Dialog with Windows 11 Dark Mode** |
| **App Icons** | Generic raster or hardcoded | Web assets | **Genuine App Icons (Extracted from shortcuts/exe + HD Brand PNGs)** |

---

## 🌟 Fitur Unggulan

### 1. Pengaturan Grafis (Settings UI)
* Klik kanan ikon tray di pojok kanan bawah lalu pilih **⚙ Settings...** (atau double click ikon tray).
* **Fitur yang dapat diatur:**
  * **Jarak dari Atas Layar (Top Margin px):** Sesuaikan posisi vertikal Island agar cocok dengan bezel monitor Anda.
  * **Durasi Notifikasi Tampil (Detik):** Atur berapa lama Island melebar sebelum kembali mengecil (default: 5.5 detik).
  * **Toggle Media & Audio Visualizer:** Aktifkan/nonaktifkan integrasi Spotify & YouTube Music.
* **Instant Hot-Reload:** Saat menekan tombol **"Simpan & Terapkan"**, pengaturan langsung disimpan ke `config.txt` dan diterapkan ke Island yang sedang berjalan secara real-time tanpa perlu restart aplikasi!

### 2. Ikon Aplikasi Asli (Genuine App Icons)
* Menampilkan ikon resmi berwarna beresolusi tinggi di samping notifikasi:
  * **WhatsApp:** Logo resmi WhatsApp hijau.
  * **Discord:** Logo resmi Discord blurple.
  * **Microsoft Edge:** Logo resmi Microsoft Edge.
  * **Google Chrome:** Logo resmi Google Chrome.
  * **Spotify & Telegram:** Logo resmi Spotify dan Telegram.
  * **Aplikasi Lainnya (Photoshop, Blender, Steam, VS Code, dll.):** Diekstrak langsung secara otomatis dari shortcut Start Menu (`.lnk`) dan file `.exe` sistem menggunakan Win32 Shell API (`SHGetFileInfoW`).
  * **Fallback Anggun:** Jika suatu aplikasi tidak memiliki ikon sistem, Island otomatis menggambar badge sirkular dengan warna brand dan huruf inisial.

### 3. Notifikasi Windows 10 & 11 Asli
* Menangkap notifikasi nyata dari sistem operasi Windows secara instan menggunakan WinRT `UserNotificationListener`.
* Notifikasi demo / mock palsu telah dinonaktifkan sehingga semua yang tampil adalah notifikasi nyata dari OS.

### 4. Media & Now Playing Island
* Mendeteksi otomatis lagu yang sedang diputar di Windows (Spotify / YouTube Music / Browser).
* **Saat lagu berganti:** Island melebar mengumumkan judul lagu dan artis (*Now Playing*).
* **Saat lagu berputar:** Menampilkan animasi audio equalizer bar dinamis di sisi kanan Island.

### 5. Mode Background Murni (No CMD Window)
* Dikompilasi dengan `#![windows_subsystem = "windows"]`.
* Tidak ada jendela hitam Command Prompt yang mengganggu saat dijalankan.

---

## 🚀 Cara Menjalankan

Jalankan file executable hasil kompilasi:
> [`target/release/dynamic_island.exe`](file:///f:/Native%20Win/DynamicNotification/target/release/dynamic_island.exe)

Atau jalankan melalui terminal:
```powershell
cargo run --bin dynamic_island --release --offline
```
