mod engine;
mod formats;
mod progress;

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

const CANCELLED: &str = "__cancelled__";
const CONVERTIBLE: &[&str] = &["webm", "mkv", "flv", "ts", "mov", "avi", "m4v", "wmv"];

/// Shared state for the one long-running operation (download or conversion).
#[derive(Default)]
struct Ops {
    busy: AtomicBool,
    cancel: AtomicBool,
    pid: Mutex<Option<u32>>,
    ytdlp: Mutex<Option<PathBuf>>,
}

// ------------------------------------------------------------------ helpers

/// No console window flashes on Windows; own process group on macOS/Linux
/// so cancel can stop yt-dlp *and* the ffmpeg it spawns.
fn hide(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
}

#[cfg(windows)]
fn kill_tree(pid: u32) {
    // /T = the whole tree (yt-dlp.exe's own child + any ffmpeg it started)
    let mut cmd = Command::new("taskkill");
    cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
    hide(&mut cmd);
    let _ = cmd.stdout(Stdio::null()).stderr(Stdio::null()).status();
}

#[cfg(unix)]
fn kill_tree(pid: u32) {
    // The child leads its own process group (see `hide`), so a negative id
    // signals the whole group: yt-dlp plus any ffmpeg it spawned.
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
    }
}

/// ffmpeg is bundled as a sidecar next to the app's own executable
/// (Contents/MacOS on a Mac). Falls back to one on PATH during development.
fn ffmpeg_path() -> Option<PathBuf> {
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let p = std::env::current_exe().ok()?.parent()?.join(name);
    p.is_file().then_some(p)
}

fn ffmpeg_cmd() -> PathBuf {
    ffmpeg_path().unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

#[derive(Deserialize, Clone, Default)]
struct Cookies {
    mode: String, // "none" | "browser" | "file"
    value: String,
}

fn cookie_args(c: &Cookies) -> Vec<String> {
    match c.mode.as_str() {
        "browser" if !c.value.is_empty() => vec!["--cookies-from-browser".into(), c.value.clone()],
        "file" if !c.value.is_empty() => vec!["--cookies".into(), c.value.clone()],
        _ => vec![],
    }
}

/// Retry flag used when a certificate error shows up: makes yt-dlp trust the
/// operating system's certificate store instead of its bundled one.
fn system_certs_args() -> Vec<String> {
    vec!["--compat-options".into(), "no-certifi".into()]
}

/// Run a process, feeding each stdout line to `on_line`; returns
/// (exited successfully, full stderr). Registers the pid so cancel can kill it.
fn run_streaming(
    ops: &Ops,
    mut cmd: Command,
    mut on_line: impl FnMut(&str),
) -> Result<(bool, String), String> {
    hide(&mut cmd);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("could not start process: {e}"))?;
    {
        let mut pid = ops.pid.lock().unwrap();
        *pid = Some(child.id());
        if ops.cancel.load(SeqCst) {
            kill_tree(child.id()); // cancel arrived before the pid was known
        }
    }
    let mut stderr = child.stderr.take().unwrap();
    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    });
    let stdout = child.stdout.take().unwrap();
    for line in BufReader::new(stdout).split(b'\n').map_while(Result::ok) {
        on_line(&String::from_utf8_lossy(&line));
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    *ops.pid.lock().unwrap() = None;
    let err = err_thread.join().unwrap_or_default();
    Ok((status.success(), err))
}

fn ytdlp(ops: &Ops) -> Result<PathBuf, String> {
    ops.ytdlp
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "yt-dlp is still being prepared, try again in a moment".into())
}

async fn local_version(bin: PathBuf) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = Command::new(&bin);
        cmd.arg("--version");
        hide(&mut cmd);
        let out = cmd.output().ok()?;
        let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && !v.is_empty()).then_some(v)
    })
    .await
    .ok()
    .flatten()
}

// ------------------------------------------------------------------ engine

#[derive(Serialize)]
struct EngineInfo {
    version: String,
    /// "current" | "updated" | "offline" | "update-failed"
    status: String,
    note: String,
    ffmpeg: bool,
}

/// Runs at launch: make sure the newest yt-dlp (with fresh certificates) is installed.
#[tauri::command]
async fn prepare_engine(app: AppHandle, ops: State<'_, Arc<Ops>>) -> Result<EngineInfo, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("bin");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let bin = dir.join(engine::local_name());

    let local = if bin.is_file() { local_version(bin.clone()).await } else { None };
    let client = engine::client()?;

    let (status, note) = match engine::latest_tag(&client).await {
        Ok(tag) if local.as_deref() == Some(tag.as_str()) => ("current", String::new()),
        Ok(tag) => {
            let app2 = app.clone();
            let res = engine::download(&client, &tag, &bin, move |pct| {
                let _ = app2.emit("engine-progress", pct);
            })
            .await;
            match res {
                Ok(()) => ("updated", String::new()),
                Err(e) if local.is_some() => ("update-failed", e),
                Err(e) => return Err(format!("Could not download yt-dlp: {e}")),
            }
        }
        Err(e) if local.is_some() => ("offline", e),
        Err(e) => {
            return Err(format!(
                "Could not reach GitHub to download yt-dlp (first launch needs internet): {e}"
            ))
        }
    };

    let version = local_version(bin.clone()).await.unwrap_or_else(|| "?".into());
    *ops.ytdlp.lock().unwrap() = Some(bin);
    Ok(EngineInfo {
        version,
        status: status.into(),
        note,
        ffmpeg: ffmpeg_path().is_some(),
    })
}

// ------------------------------------------------------------------ formats

#[derive(Serialize)]
struct FetchResult {
    title: String,
    cards: Vec<formats::Card>,
}

#[tauri::command]
async fn fetch_formats(
    ops: State<'_, Arc<Ops>>,
    url: String,
    cookies: Cookies,
) -> Result<FetchResult, String> {
    let bin = ytdlp(&ops)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut extra: Vec<String> = Vec::new();
        loop {
            let mut cmd = Command::new(&bin);
            cmd.args(["-J", "--no-playlist", "--no-warnings"])
                .args(cookie_args(&cookies))
                .args(&extra)
                .arg("--")
                .arg(&url);
            hide(&mut cmd);
            let out = cmd.output().map_err(|e| format!("could not start yt-dlp: {e}"))?;
            if out.status.success() {
                let info: serde_json::Value = serde_json::from_slice(&out.stdout)
                    .map_err(|e| format!("unexpected yt-dlp output: {e}"))?;
                let title = info
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("video")
                    .to_string();
                return Ok(FetchResult { title, cards: formats::summarize(&info) });
            }
            let err = String::from_utf8_lossy(&out.stderr).into_owned();
            if extra.is_empty() && progress::is_cert_error(&err) {
                extra = system_certs_args();
                continue;
            }
            return Err(progress::last_error(&err));
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

// ------------------------------------------------------------------ download

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadReq {
    url: String,
    spec: String,
    folder: String,
    cookies: Cookies,
    auto_convert: bool,
}

#[derive(Serialize, Clone)]
struct Done {
    ok: bool,
    cancelled: bool,
    msg: String,
}

fn do_download(app: &AppHandle, ops: &Ops, bin: &Path, req: &DownloadReq) -> Result<Option<PathBuf>, String> {
    let list = std::env::temp_dir().join(format!("uvd-files-{}.txt", std::process::id()));
    let mut extra: Vec<String> = Vec::new();
    loop {
        let _ = std::fs::remove_file(&list);
        let mut cmd = Command::new(bin);
        cmd.args(["-f", &req.spec, "-P", &req.folder, "-o", "%(title)s.%(ext)s"])
            .args(["--no-playlist", "--newline", "--progress"])
            .args(["--progress-template", progress::TEMPLATE])
            .args(["--print-to-file", "after_move:filepath"])
            .arg(&list);
        if let Some(ff) = ffmpeg_path() {
            cmd.arg("--ffmpeg-location").arg(ff);
        }
        cmd.args(cookie_args(&req.cookies)).args(&extra).arg("--").arg(&req.url);

        let app2 = app.clone();
        let (ok, err) = run_streaming(ops, cmd, |line| {
            if let Some(p) = progress::parse_dl_line(line) {
                let _ = app2.emit("dl-progress", p);
            }
        })?;
        if ops.cancel.load(SeqCst) {
            return Err(CANCELLED.into());
        }
        if ok {
            // UTF-8 file written by yt-dlp: safe for any title (accents, Japanese...)
            let file = std::fs::read_to_string(&list)
                .ok()
                .and_then(|s| s.lines().rev().find(|l| !l.trim().is_empty()).map(|l| PathBuf::from(l.trim())));
            let _ = std::fs::remove_file(&list);
            return Ok(file);
        }
        if extra.is_empty() && progress::is_cert_error(&err) {
            let _ = app.emit("dl-note", "Certificate problem, retrying with your system certificates…");
            extra = system_certs_args();
            continue;
        }
        return Err(progress::last_error(&err));
    }
}

fn unique_mp4(src: &Path) -> PathBuf {
    let dir = src.parent().unwrap_or(Path::new("."));
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or("video".into());
    let mut dst = dir.join(format!("{stem}.mp4"));
    let mut i = 1;
    while dst.exists() {
        dst = dir.join(format!("{stem} ({i}).mp4"));
        i += 1;
    }
    dst
}

fn convert_file(app: &AppHandle, ops: &Ops, src: &Path, delete_original: bool) -> Result<PathBuf, String> {
    let ff = ffmpeg_cmd();

    // duration, so progress can be shown as a percentage
    let mut probe = Command::new(&ff);
    probe.args(["-hide_banner", "-i"]).arg(src);
    hide(&mut probe);
    let total = probe
        .output()
        .ok()
        .and_then(|o| progress::parse_duration(&String::from_utf8_lossy(&o.stderr)))
        .unwrap_or(0.0);

    let dst = unique_mp4(src);
    let mut cmd = Command::new(&ff);
    cmd.args(["-y", "-loglevel", "error", "-i"])
        .arg(src)
        .args(["-c:v", "libx264", "-preset", "fast", "-crf", "18", "-pix_fmt", "yuv420p"])
        .args(["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"])
        .args(["-nostats", "-progress", "pipe:1"])
        .arg(&dst);

    let app2 = app.clone();
    let mut last = -1.0_f64;
    let (ok, err) = run_streaming(ops, cmd, |line| {
        if total > 0.0 {
            if let Some(t) = progress::parse_out_time(line) {
                let pct = (t / total * 100.0).min(100.0);
                if pct - last >= 0.2 {
                    last = pct;
                    let _ = app2.emit("conv-progress", pct);
                }
            }
        }
    })?;

    let usable = dst.metadata().map(|m| m.len() > 0).unwrap_or(false);
    if ops.cancel.load(SeqCst) {
        let _ = std::fs::remove_file(&dst); // never leave a half-written mp4
        return Err(CANCELLED.into());
    }
    if !ok || !usable {
        let _ = std::fs::remove_file(&dst);
        return Err(progress::last_error(&err));
    }
    let _ = app.emit("conv-progress", 100.0);
    if delete_original {
        let _ = std::fs::remove_file(src);
    }
    Ok(dst)
}

fn report_conversion(app: &AppHandle, res: Result<PathBuf, String>) {
    let done = match res {
        Ok(dst) => Done {
            ok: true,
            cancelled: false,
            msg: format!(
                "✓ Converted: {}",
                dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            ),
        },
        Err(e) if e == CANCELLED => Done { ok: false, cancelled: true, msg: "✕ Conversion cancelled".into() },
        Err(e) => Done { ok: false, cancelled: false, msg: format!("Conversion failed: {e}") },
    };
    let _ = app.emit("conv-done", done);
}

fn begin(ops: &Ops) -> Result<(), String> {
    if ops.busy.swap(true, SeqCst) {
        return Err("Another download or conversion is already running".into());
    }
    ops.cancel.store(false, SeqCst);
    Ok(())
}

fn finish(app: &AppHandle, ops: &Ops) {
    ops.busy.store(false, SeqCst);
    let _ = app.emit("op-end", ());
}

#[tauri::command]
fn start_download(app: AppHandle, ops: State<'_, Arc<Ops>>, req: DownloadReq) -> Result<(), String> {
    let bin = ytdlp(&ops)?;
    begin(&ops)?;
    let ops = Arc::clone(&ops);
    std::thread::spawn(move || {
        match do_download(&app, &ops, &bin, &req) {
            Err(e) if e == CANCELLED => {
                let _ = app.emit("dl-done", Done { ok: false, cancelled: true, msg: "✕ Download cancelled".into() });
            }
            Err(e) => {
                let _ = app.emit("dl-done", Done { ok: false, cancelled: false, msg: format!("Error: {e}") });
            }
            Ok(file) => {
                let _ = app.emit("dl-done", Done { ok: true, cancelled: false, msg: format!("✓ Saved to {}", req.folder) });
                let needs_convert = file.as_ref().filter(|f| {
                    f.is_file()
                        && f.extension()
                            .map(|e| CONVERTIBLE.contains(&e.to_string_lossy().to_lowercase().as_str()))
                            .unwrap_or(false)
                });
                if req.auto_convert && !ops.cancel.load(SeqCst) {
                    if let Some(f) = needs_convert {
                        let _ = app.emit("conv-start", ());
                        report_conversion(&app, convert_file(&app, &ops, f, true));
                    }
                }
            }
        }
        finish(&app, &ops);
    });
    Ok(())
}

#[tauri::command]
fn start_convert(app: AppHandle, ops: State<'_, Arc<Ops>>, path: String) -> Result<(), String> {
    let src = PathBuf::from(&path);
    if !src.is_file() {
        return Err("File not found".into());
    }
    begin(&ops)?;
    let ops = Arc::clone(&ops);
    std::thread::spawn(move || {
        let _ = app.emit("conv-start", ());
        report_conversion(&app, convert_file(&app, &ops, &src, false));
        finish(&app, &ops);
    });
    Ok(())
}

#[tauri::command]
fn cancel_op(ops: State<'_, Arc<Ops>>) {
    ops.cancel.store(true, SeqCst);
    if let Some(pid) = *ops.pid.lock().unwrap() {
        kill_tree(pid);
    }
}

// ------------------------------------------------------------------ pickers

#[tauri::command]
async fn pick_folder(app: AppHandle, current: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut d = app.dialog().file().set_title("Choose download folder");
        if !current.is_empty() {
            d = d.set_directory(current);
        }
        d.blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
async fn pick_file(app: AppHandle, kind: String, current: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut d = app.dialog().file();
        d = if kind == "cookies" {
            d.set_title("Choose a cookies.txt file").add_filter("Cookies file", &["txt"])
        } else {
            d.set_title("Choose a video to convert")
                .add_filter("Videos", &["webm", "mkv", "avi", "mov", "flv", "ts", "m4v", "wmv", "mp4"])
        };
        if !current.is_empty() {
            d = d.set_directory(current);
        }
        d.blocking_pick_file()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
fn default_folder(app: AppHandle) -> String {
    app.path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ------------------------------------------------------------------ entry

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(Ops::default()))
        .invoke_handler(tauri::generate_handler![
            prepare_engine,
            fetch_formats,
            start_download,
            start_convert,
            cancel_op,
            pick_folder,
            pick_file,
            default_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running Universal Video Downloader");
}
