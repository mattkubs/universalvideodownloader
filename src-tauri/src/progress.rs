//! Parsers for the text that yt-dlp and ffmpeg print while working.

use serde::Serialize;

/// Progress template handed to yt-dlp. Every line it prints looks like:
/// `UVD|downloaded|total|total_estimate|speed|eta|vcodec` ("NA" when unknown).
pub const TEMPLATE: &str = "download:UVD|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|\
%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(info.vcodec)s";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DlProgress {
    pub pct: f64,
    pub speed: String,
    pub eta: String,
    /// "video", "audio" or "" when the site doesn't say
    pub stream: String,
}

fn num(v: &str) -> Option<f64> {
    v.trim().parse::<f64>().ok().filter(|x| x.is_finite())
}

pub fn parse_dl_line(line: &str) -> Option<DlProgress> {
    let rest = line.trim().strip_prefix("UVD|")?;
    let p: Vec<&str> = rest.split('|').collect();
    if p.len() < 6 {
        return None;
    }
    let done = num(p[0]).unwrap_or(0.0);
    let total = num(p[1]).or_else(|| num(p[2])).unwrap_or(0.0);
    let pct = if total > 0.0 { (done / total * 100.0).clamp(0.0, 100.0) } else { 0.0 };
    let speed = num(p[3]).map(fmt_speed).unwrap_or_default();
    let eta = num(p[4]).map(|s| fmt_eta(s as u64)).unwrap_or_default();
    let stream = match p.get(5).map(|s| s.trim()) {
        Some("none") => "audio",
        Some("NA") | Some("") | None => "",
        Some(_) => "video",
    }
    .to_string();
    Some(DlProgress { pct, speed, eta, stream })
}

pub fn fmt_speed(bps: f64) -> String {
    let mb = bps / 1_000_000.0;
    if mb >= 1.0 {
        format!("{mb:.1} MB/s")
    } else {
        format!("{:.0} KB/s", bps / 1000.0)
    }
}

pub fn fmt_eta(secs: u64) -> String {
    if secs >= 3600 {
        format!("ETA {}h {:02}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("ETA {}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("ETA {secs}s")
    }
}

fn hms(t: &str) -> Option<f64> {
    let mut it = t.trim().split(':');
    let h = num(it.next()?)?;
    let m = num(it.next()?)?;
    let s = num(it.next()?)?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// Total duration from `ffmpeg -i file` stderr: "Duration: 00:01:02.50, ..."
pub fn parse_duration(stderr: &str) -> Option<f64> {
    let i = stderr.find("Duration:")?;
    let rest = &stderr[i + "Duration:".len()..];
    let t = rest.trim_start().split(',').next()?;
    hms(t).filter(|d| *d > 0.0)
}

/// Seconds encoded so far, from one `ffmpeg -progress pipe:1` line.
/// Note: `out_time_ms` is actually microseconds (a long-standing ffmpeg quirk).
pub fn parse_out_time(line: &str) -> Option<f64> {
    let line = line.trim();
    if let Some(v) = line.strip_prefix("out_time_us=").or_else(|| line.strip_prefix("out_time_ms=")) {
        return num(v).filter(|x| *x >= 0.0).map(|us| us / 1_000_000.0);
    }
    line.strip_prefix("out_time=").and_then(hms)
}

pub fn is_cert_error(text: &str) -> bool {
    let t = text.to_ascii_uppercase();
    t.contains("CERTIFICATE_VERIFY_FAILED") || t.contains("SSL: CERTIFICATE")
}

/// Last meaningful error line from yt-dlp's stderr, trimmed for the UI.
pub fn last_error(stderr: &str) -> String {
    let line = stderr
        .lines()
        .rev()
        .find(|l| l.starts_with("ERROR:"))
        .or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()))
        .unwrap_or("Unknown error")
        .trim();
    let line = line.strip_prefix("ERROR: ").unwrap_or(line);
    let mut out: String = line.chars().take(160).collect();
    if line.chars().count() > 160 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_ytdlp_lines() {
        // captured from a real yt-dlp 2026.10.26 run with TEMPLATE
        let p = parse_dl_line("UVD|1047552|2100411|NA|2088693.957090457|0|NA").unwrap();
        assert!((p.pct - 49.87).abs() < 0.01);
        assert_eq!(p.speed, "2.1 MB/s");
        assert_eq!(p.eta, "ETA 0s");
        assert_eq!(p.stream, "");
        let first = parse_dl_line("UVD|1024|2100411|NA|NA|NA|NA").unwrap();
        assert_eq!(first.speed, "");
        assert_eq!(first.eta, "");
    }

    #[test]
    fn estimate_and_stream_kind() {
        let p = parse_dl_line("UVD|500|NA|1000|NA|75|none").unwrap();
        assert_eq!(p.pct, 50.0);
        assert_eq!(p.eta, "ETA 1m 15s");
        assert_eq!(p.stream, "audio");
        assert_eq!(parse_dl_line("UVD|1|2|NA|NA|NA|avc1.640028").unwrap().stream, "video");
        assert!(parse_dl_line("[download] something else").is_none());
    }

    #[test]
    fn ffmpeg_parsing() {
        let err = "Input #0, matroska,webm, from 'x.webm':\n  Duration: 00:01:02.50, start: 0.0, bitrate: 900 kb/s";
        assert_eq!(parse_duration(err), Some(62.5));
        assert_eq!(parse_duration("Duration: N/A, bitrate"), None);
        assert_eq!(parse_out_time("out_time_ms=31250000"), Some(31.25));
        assert_eq!(parse_out_time("out_time_us=1500000"), Some(1.5));
        assert_eq!(parse_out_time("out_time=00:00:10.500000"), Some(10.5));
        assert_eq!(parse_out_time("out_time=N/A"), None);
    }

    #[test]
    fn errors() {
        assert!(is_cert_error("ERROR: [SSL: CERTIFICATE_VERIFY_FAILED] certificate verify failed"));
        assert!(!is_cert_error("ERROR: HTTP Error 403: Forbidden"));
        let e = "WARNING: x\nERROR: [youtube] abc: Sign in to confirm your age\n";
        assert_eq!(last_error(e), "[youtube] abc: Sign in to confirm your age");
    }
}
