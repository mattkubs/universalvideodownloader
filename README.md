# Universal Video Downloader (Tauri / Rust)

A Rust + Tauri rewrite of the Python/PySide6 app. Same design and features, much
smaller and instant to launch.

## What's different from the Python version

| | Python (PyInstaller) | Tauri (this) |
|---|---|---|
| App size | ~150 MB single exe | ~8 MB app + ~70 MB bundled ffmpeg |
| Launch | 5-10 s (unpacks itself every time) | instant |
| yt-dlp | frozen at build time, rebuild to update | **downloaded fresh on launch, auto-updated** |
| SSL certificates | frozen at build time | **refreshed with every yt-dlp update + OS store fallback** |
| Windows + Mac | separate builds by hand | one workflow builds both |

## How updates and certificates work

On every launch the app:

1. Asks GitHub for the newest yt-dlp release.
2. If the copy in its data folder is older (or missing), downloads the official
   standalone yt-dlp, **verifies its SHA-256** against yt-dlp's published
   checksums, and swaps it in. The footer shows the version and whether it's
   up to date.
3. Because that yt-dlp carries its own up-to-date certificate bundle, updating it
   also refreshes the certificates. If a site still fails with
   `CERTIFICATE_VERIFY_FAILED`, the app automatically retries once using the
   operating system's certificate store (which Windows Update / macOS keep current).

The update check itself uses the OS certificate store, so it can never be locked
out by stale certificates. Offline? It keeps using the copy it already has.

yt-dlp lives in:
- Windows: `%LOCALAPPDATA%\work.kubs.universalvideodownloader\bin\`
- macOS: `~/Library/Application Support/work.kubs.universalvideodownloader/bin/`

## Building (no Rust needed on your PC)

GitHub builds everything in the cloud:

1. Put this project in a GitHub repository (the `.github/workflows/build.yml` file must be included).
2. Open the **Actions** tab, choose **Build app**, click **Run workflow**.
3. When both jobs are green, open the run's **Summary** page and download from **Artifacts**:
   - `Universal-Video-Downloader-Windows`: an installer (`..._x64-setup.exe`) and a **portable zip**
     (unzip anywhere, run `Universal Video Downloader.exe`, keep `ffmpeg.exe` next to it)
   - `Universal-Video-Downloader-macOS`: the `.app` (unsigned: first launch with right-click, Open)

First launch needs internet once to fetch yt-dlp (about 18 MB on Windows).

## Building locally (optional)

Requires Rust (rustup.rs), Node.js, and on Windows the "Desktop development with C++"
workload from Visual Studio Build Tools. Put an ffmpeg binary at
`src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe` (name ends with your Rust
target triple, see `rustc -vV`), then:

```
npm install
npx tauri build
```

## Project layout

```
src/                 UI (HTML, CSS, JS)
src-tauri/src/
  lib.rs             commands: formats, download, convert, cancel, pickers
  engine.rs          yt-dlp auto-updater (GitHub + SHA-256 verification)
  formats.rs         turns yt-dlp metadata into the quality cards
  progress.rs        parses yt-dlp / ffmpeg progress output
.github/workflows/   cloud build for Windows + macOS
```
