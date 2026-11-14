# Dynamic Island for Windows (Native Rust)

A modern, high-performance, ultra-lightweight **Dynamic Island desktop overlay for Windows 10 & 11**, built from the ground up in pure **Rust** utilizing **DirectComposition**, **Direct2D (Hardware Accelerated)**, **DirectWrite**, native **WinRT UserNotificationListener**, **Windows Global System Media Transport Controls (GSMTC)**, and **YTMPX WebSocket** integration.

---

## ⚡ Highlights & Architecture Comparison

| Metric | Traditional WPF / .NET | Electron / Webview-based | **Dynamic Island (Native Rust)** |
| :--- | :--- | :--- | :--- |
| **Idle Memory** | 80 MB – 250 MB | 250 MB – 600 MB+ | **~35 MB** (Includes DirectX & WinRT) |
| **Idle CPU** | 0.5% – 2.5% | 1.0% – 5.0% | **0.0%** (Event-driven & Sleep Settled) |
| **Binary Footprint** | 50 MB+ (.NET runtime) | 120 MB+ | **~1.1 MB** (Standalone single `.exe`) |
| **Console Window** | Requires custom launcher | Hidden via node wrapper | **Native Windows Subsystem (No CMD window)** |
| **Graphics Engine** | Software / WPF Composition | Chromium GPU Rasterizer | **DirectComposition + Direct2D (GPU Direct)** |
| **Transparency & Clipping** | Layered Window GDI | Transparent Chromium View | **Hardware Premultiplied Alpha + Dynamic GDI HRGN** |
| **Settings UI** | Heavy XAML Form | Web Form / React | **Hardware-Accelerated Direct2D Fluent UI** |

---

## 🌟 Key Features

### 1. 🛡️ Dynamic Window Clipping Region (Zero Click Interference)
* Utilizes dynamic kernel-level OS window region clipping (**GDI `SetWindowRgn`**) that tightly contours to the exact pill capsule shape.
* Pixels outside the visible capsule **physically do not exist** to the Windows Window Manager.
* **Benefit:** You can click browser tabs, bookmark bars, address bars, or File Explorer breadcrumbs directly adjacent to the island with 100% precision and zero hit-blocking.
* Features dynamic 16px padding so soft ambient luminous glow remains butter-smooth and unclipped.

### 2. 🎬 Fullscreen Auto-Hide (Netflix, YouTube, Games)
* Automatically detects when any active foreground application enters **Fullscreen** mode (e.g. streaming **Netflix**, **YouTube Fullscreen**, video players like VLC, or **3D Games**).
* Smoothly glides off-screen (`y: -100px`) and transitions opacity to 0% with hardware region zeroing.
* Automatically and seamlessly slides back into view when exiting fullscreen mode.

### 3. 🧭 Smart Dynamic Contour Avoidance
* **Browser Tab Strip Clearance:** When a web browser (Chrome, Edge, Brave, Firefox, Opera, Vivaldi) is maximized or docked at the top of the monitor, the island automatically nudges down by **34px** (`contour_nudge_y`), perfectly positioning itself below the browser tabs so all tabs remain unobstructed and clickable.
* **Top Flush Auto-Restoration:** Automatically returns flush to the top edge (`0px`) when switching to File Explorer, Desktop, or non-browser windows.
* Nudge distance is fully customizable via the interactive Settings UI.

### 4. 🎵 Advanced Media Controls & YTMPX Integration
* **YouTube Music Desktop (Tauri) & Web Players:** Integrates via real-time WebSocket (`ws://127.0.0.1:8765` YTMPX) and Windows GSMTC.
* **Native Window Activation:** Clicking the capsule brings or restores the player application (Tauri YTM, Spotify, or browser tab) natively from the system tray.
* **Single-Route Transport & Anti-Double-Skip:** Media commands (Play/Pause, Previous, Next) are routed exclusively through dedicated channels with **250ms hardware debounce**, completely preventing accidental double skips.
* **Cover Art In-Memory Cache (`COVER_CACHE`):** Artwork bytes are cached in memory with auto-rehydration so album covers on the spinning vinyl never disappear across fullscreen transitions.

### 5. 🏷️ Multi-Platform Service Recognition & Official Badges
Intelligently identifies active media services and draws crisp vector brand badges:
* **Netflix:** Signature dark squircle (`#141414`) with the iconic red "N" badge (`#E50914`).
* **YouTube Music & YouTube:** Official red squircle with play symbol.
* **Spotify:** Authentic green squircle with triple soundwave arcs (`#1DB954`).
* **Discord, WhatsApp, Telegram:** Genuine vector brand icons with brand palettes.
* **Disney+, Prime Video, Twitch, SoundCloud, Bilibili:** Automatic service detection and custom brand colors.
* **Native Browser Fallback:** Identifies and displays the genuine browser name (Edge, Chrome, Firefox, Brave) when playing generic web media.

### 6. 💽 Spinning Vinyl Record with Real Album Artwork
* Smooth rotating vinyl record disc featuring:
  * 4 concentric microgroove rings.
  * Dual opposing 180° anisotropic specular light sheen highlights.
  * Centered real album artwork rotating with the disc and spindle hole cutout.
* **WASAPI Audio Metering:** Real-time PC audio peak metering that reacts dynamically to PC audio output.

### 7. ⏱️ Hover-to-Expand with Dwell Protection
* Features a **180ms hover dwell delay** to prevent accidental expansion when the mouse cursor briefly moves across the top of the screen.
* Smoothly expands to reveal full media playback controls only when deliberately hovered.

### 8. ⚙️ Interactive Fluent Direct2D Settings Window
Access via System Tray context menu $\rightarrow$ **⚙ Settings...** (or double-click the tray icon):
* **Sliders:**
  * Vertical Screen Offset (*Top Margin*).
  * Browser Clearance Distance (*Browser Contour Nudge*).
  * Compact Music Capsule Width (*Compact Music Width*).
  * Notification Display Duration (*Display Duration*).
  * Capsule Opacity (*Island Opacity*).
  * Luminous Ambient Glow (*Glow Intensity*).
  * Vinyl Rotation Speed (*Vinyl Spin Speed*).
* **Toggles:**
  * Dynamic Album Glow Color (Halo dynamically tints to match dominant artwork colors).
  * Media Island & Vinyl Animation.
  * Hover-to-Expand.
  * Idle Click-Through.
  * Smart Dynamic Contour.
  * Run on Windows Startup.
* **Visual Styles:**
  * *Modern Minimal*, *Sleek Capsule*, *Notch Dynamic*.
  * Compact Views: *Combined (Title + Wave)*, *Title Only*, *Wave Only*.
* **Instant Auto-Save:** Changes are persisted to `config.txt` and hot-reloaded into the running overlay without restarting.

---

## 🚀 Getting Started

### Run Pre-built Binary
Run the compiled standalone executable:
> [`target/release/dynamic_island.exe`](file:///f:/Native%20Win/DynamicNotification/target/release/dynamic_island.exe)

### Build from Source
Ensure you have the Rust toolchain installed:
```powershell
# Run directly in release mode
cargo run --release

# Or build the standalone release binary
cargo build --release
```

---

## ⌨️ Controls & Shortcuts

| Action | Result |
| :--- | :--- |
| **Hover Cursor (>180ms)** | Expands to full media controls (Play/Pause, Previous, Next, Waveform) |
| **Click Prev / Next** | Skip tracks (protected with 250ms anti-bounce debounce) |
| **Click Play / Pause** | Toggle playback |
| **Click Capsule Surface** | Brings / restores player window (Tauri YTM, Spotify, or browser) |
| **Click Outside Capsule** | Dismisses expansion back to sleek pill |
| **Right-Click Tray Icon** | Opens context menu (Settings, Autostart, Exit) |
| **Press `Esc`** | Smoothly dismisses expanded island view |

---

## 📄 License
Licensed under the **MIT License**. Free for personal, academic, and commercial use.
