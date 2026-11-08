//! Keeps yt-dlp itself up to date.
//!
//! On every launch the app asks GitHub for the newest yt-dlp release and, if the
//! copy in the app's data folder is older (or missing), downloads the official
//! standalone binary, verifies it against yt-dlp's published SHA-256 checksums,
//! and swaps it in. Because that binary carries its own certificate bundle, this
//! also refreshes the SSL certificates yt-dlp trusts.
//!
//! The update check itself goes through `reqwest` with `native-tls`, which uses
//! the operating system's certificate store (kept current by Windows Update /
//! macOS updates), so the updater can never be locked out by stale certificates.

use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

const REPO: &str = "https://github.com/yt-dlp/yt-dlp";

/// Name of the official release asset for this platform.
pub fn asset_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "yt-dlp_macos" // universal2: Apple Silicon + Intel
    } else if cfg!(target_arch = "aarch64") {
        "yt-dlp_linux_aarch64"
    } else {
        "yt-dlp_linux"
    }
}

/// File name used inside the app's data folder.
pub fn local_name() -> &'static str {
    if cfg!(windows) {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    }
}

pub fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("UniversalVideoDownloader/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())
}

/// Newest release tag, e.g. "2026.10.26".
pub async fn latest_tag(client: &reqwest::Client) -> Result<String, String> {
    // 1) GitHub API
    let api = client
        .get("https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .send()
        .await;
    if let Ok(resp) = api {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                if let Some(tag) = v.get("tag_name").and_then(|t| t.as_str()) {
                    return Ok(tag.to_string());
                }
            }
        }
    }
    // 2) Fallback that doesn't count against the API rate limit:
    //    /releases/latest redirects to /releases/tag/<version>
    let resp = client
        .head(format!("{REPO}/releases/latest"))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    resp.url()
        .as_str()
        .rsplit_once("/tag/")
        .map(|(_, t)| t.trim_end_matches('/').to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "could not determine the latest yt-dlp version".into())
}

fn expected_hash(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let hash = it.next()?;
        let name = it.next()?.trim_start_matches('*');
        (name == asset).then(|| hash.to_ascii_lowercase())
    })
}

/// Download release `tag` to `dest`, verifying its checksum. `dest` is only
/// replaced after the new file is complete and verified.
pub async fn download(
    client: &reqwest::Client,
    tag: &str,
    dest: &Path,
    mut on_progress: impl FnMut(f64),
) -> Result<(), String> {
    let base = format!("{REPO}/releases/download/{tag}");
    let asset = asset_name();

    let sums = client
        .get(format!("{base}/SHA2-256SUMS"))
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("checksum list: {e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let expected = expected_hash(&sums, asset).ok_or("checksum for this platform not published")?;

    let mut resp = client
        .get(format!("{base}/{asset}"))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let total = resp.content_length().unwrap_or(0);

    let part = dest.with_extension("part");
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut done: u64 = 0;
    loop {
        let chunk = match resp.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => {
                drop(file);
                let _ = std::fs::remove_file(&part);
                return Err(e.to_string());
            }
        };
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        hasher.update(&chunk);
        done += chunk.len() as u64;
        if total > 0 {
            on_progress(done as f64 / total as f64 * 100.0);
        }
    }
    drop(file);

    let got = format!("{:x}", hasher.finalize());
    if got != expected {
        let _ = std::fs::remove_file(&part);
        return Err("downloaded yt-dlp failed checksum verification".into());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    std::fs::rename(&part, dest).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_right_checksum_line() {
        let sums = "aaa111  yt-dlp\nBBB222  yt-dlp.exe\nccc333  yt-dlp_macos\nddd  yt-dlp_x86.exe\n";
        assert_eq!(expected_hash(sums, "yt-dlp.exe").as_deref(), Some("bbb222"));
        assert_eq!(expected_hash(sums, "yt-dlp_macos").as_deref(), Some("ccc333"));
        assert_eq!(expected_hash(sums, "yt-dlp_linux"), None);
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Real network test against yt-dlp's GitHub releases.
    /// Run with: UVD_TAG=2026.10.26 cargo test live_ -- --ignored
    #[test]
    #[ignore]
    fn live_download_and_verify() {
        let tag = std::env::var("UVD_TAG").expect("set UVD_TAG");
        let dir = std::env::temp_dir().join("uvd-live-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join(local_name());
        let mut last = 0.0;
        tauri::async_runtime::block_on(async {
            let c = client().unwrap();
            download(&c, &tag, &dest, |p| last = p).await.unwrap();
        });
        assert!(last > 99.0, "progress should reach 100, got {last}");
        let out = std::process::Command::new(&dest).arg("--version").output().unwrap();
        let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
        println!("downloaded yt-dlp reports version {v}");
        assert_eq!(v, tag);
    }
}
