//! Turns yt-dlp's raw `-J` metadata into the friendly preset cards shown in the UI.
//! Port of the Python `_summarize`, tolerant of non-YouTube sites: progressive
//! formats (video+audio in one file), unknown codecs, missing heights/sizes,
//! and sites with no separate audio streams.

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Card {
    pub label: String,
    pub best: bool,
    pub chips: Vec<String>,
    pub size: String,
    pub spec: String,
}

fn s<'a>(f: &'a Value, key: &str) -> Option<&'a str> {
    f.get(key).and_then(Value::as_str)
}

fn n(f: &Value, key: &str) -> Option<f64> {
    f.get(key).and_then(Value::as_f64).filter(|v| *v > 0.0)
}

fn codec_absent(f: &Value, key: &str) -> bool {
    matches!(s(f, key), None | Some("none"))
}

fn has_video(f: &Value) -> bool {
    !codec_absent(f, "vcodec") || n(f, "height").is_some() || n(f, "width").is_some()
}

fn audio_only(f: &Value) -> bool {
    codec_absent(f, "vcodec") && !codec_absent(f, "acodec") && n(f, "height").is_none()
}

fn progressive(f: &Value) -> bool {
    !codec_absent(f, "acodec")
}

fn bitrate(f: &Value) -> f64 {
    n(f, "tbr").or_else(|| n(f, "vbr")).or_else(|| n(f, "abr")).unwrap_or(0.0)
}

fn audio_rate(f: &Value) -> f64 {
    n(f, "abr").or_else(|| n(f, "tbr")).unwrap_or(0.0)
}

/// Best-effort size of one format: exact if reported, else bitrate x duration.
pub fn est_bytes(f: Option<&Value>, duration: f64) -> f64 {
    let Some(f) = f else { return 0.0 };
    if let Some(sz) = n(f, "filesize").or_else(|| n(f, "filesize_approx")) {
        return sz;
    }
    let br = bitrate(f);
    if br > 0.0 && duration > 0.0 {
        br * 1000.0 / 8.0 * duration
    } else {
        0.0
    }
}

pub fn fmt_bytes(b: f64) -> String {
    if b <= 0.0 {
        return "size unknown".into();
    }
    let mb = b / (1024.0 * 1024.0);
    if mb >= 1024.0 {
        format!("~{:.2} GB", mb / 1024.0)
    } else if mb >= 10.0 {
        format!("~{:.0} MB", mb)
    } else {
        format!("~{:.1} MB", mb)
    }
}

fn max_by<'a>(it: impl Iterator<Item = &'a Value>, key: fn(&Value) -> f64) -> Option<&'a Value> {
    it.fold(None, |acc: Option<&Value>, f| match acc {
        Some(a) if key(a) >= key(f) => Some(a),
        _ => Some(f),
    })
}

pub fn summarize(info: &Value) -> Vec<Card> {
    let empty = Vec::new();
    let formats: &Vec<Value> = info.get("formats").and_then(Value::as_array).unwrap_or(&empty);
    let duration = n(info, "duration").unwrap_or(0.0);

    let videos: Vec<&Value> = formats.iter().filter(|f| has_video(f) && !audio_only(f)).collect();
    let audios: Vec<&Value> = formats.iter().filter(|f| audio_only(f)).collect();

    let best_audio = max_by(audios.iter().copied(), audio_rate);
    let audio_bytes = est_bytes(best_audio, duration);

    let best_at = |h: f64| {
        max_by(
            videos.iter().copied().filter(|f| n(f, "height") == Some(h)),
            bitrate,
        )
    };
    let total = |f: Option<&Value>| -> f64 {
        let Some(f) = f else { return 0.0 };
        let v = est_bytes(Some(f), duration);
        if v <= 0.0 {
            0.0
        } else if progressive(f) {
            v // already carries its audio
        } else {
            v + audio_bytes
        }
    };

    let mut heights: Vec<f64> = videos.iter().filter_map(|f| n(f, "height")).collect();
    heights.sort_by(|a, b| b.partial_cmp(a).unwrap());
    heights.dedup();

    let mut cards = Vec::new();
    if !videos.is_empty() {
        let top = match heights.first() {
            Some(h) => best_at(*h),
            None => max_by(videos.iter().copied(), bitrate),
        };
        cards.push(Card {
            label: "Best".into(),
            best: true,
            chips: vec!["auto".into(), "video+audio".into()],
            size: fmt_bytes(total(top)),
            spec: "bestvideo+bestaudio/best".into(),
        });
    }

    for h in heights.iter().take(5) {
        let f = best_at(*h);
        let ext = f.and_then(|f| s(f, "ext")).unwrap_or("video").to_string();
        let h = *h as i64;
        cards.push(Card {
            label: format!("{h}p"),
            best: false,
            chips: vec![ext],
            size: fmt_bytes(total(f)),
            spec: format!("bestvideo[height<={h}]+bestaudio/best[height<={h}]"),
        });
    }

    if !audios.is_empty() {
        let pick = max_by(audios.iter().copied().filter(|f| s(f, "ext") == Some("m4a")), audio_rate)
            .or(best_audio);
        cards.push(Card {
            label: "Audio only".into(),
            best: false,
            chips: vec![pick.and_then(|f| s(f, "ext")).unwrap_or("audio").to_string()],
            size: fmt_bytes(est_bytes(pick, duration)),
            spec: "bestaudio[ext=m4a]/bestaudio".into(),
        });
    }

    // Bare generic-extractor output: always offer one catch-all card.
    if cards.is_empty() {
        let guess = formats.first().or(Some(info));
        cards.push(Card {
            label: "Best available".into(),
            best: true,
            chips: vec![guess.and_then(|f| s(f, "ext")).unwrap_or("auto").to_string()],
            size: fmt_bytes(est_bytes(guess, duration)),
            spec: "best".into(),
        });
    }
    cards
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn labels_sizes(cards: &[Card]) -> Vec<(String, String)> {
        cards.iter().map(|c| (c.label.clone(), c.size.clone())).collect()
    }

    #[test]
    fn youtube_style_split_streams() {
        let info = json!({"duration": 600, "formats": [
            {"height": 1080, "vcodec": "avc1", "acodec": "none", "ext": "mp4", "filesize": 350_000_000u64},
            {"height": 720, "vcodec": "vp9", "acodec": "none", "ext": "webm", "filesize": 160_000_000u64},
            {"vcodec": "none", "acodec": "mp4a", "ext": "m4a", "abr": 128, "filesize": 9_600_000u64}
        ]});
        let c = summarize(&info);
        assert_eq!(labels_sizes(&c), vec![
            ("Best".into(), "~343 MB".into()),
            ("1080p".into(), "~343 MB".into()),
            ("720p".into(), "~162 MB".into()),
            ("Audio only".into(), "~9.2 MB".into()),
        ]);
        assert_eq!(c[2].chips, vec!["webm"]);
    }

    #[test]
    fn progressive_site_no_double_audio() {
        let info = json!({"duration": 45, "formats": [
            {"height": 720, "vcodec": "avc1", "acodec": "mp4a", "ext": "mp4", "filesize": 18_000_000u64},
            {"height": 480, "vcodec": "avc1", "acodec": "mp4a", "ext": "mp4", "tbr": 1200},
            {"height": 270, "vcodec": "avc1", "acodec": "mp4a", "ext": "mp4"}
        ]});
        let c = summarize(&info);
        assert_eq!(labels_sizes(&c), vec![
            ("Best".into(), "~17 MB".into()),
            ("720p".into(), "~17 MB".into()),
            ("480p".into(), "~6.4 MB".into()),
            ("270p".into(), "size unknown".into()),
        ]);
    }

    #[test]
    fn generic_bare_page() {
        let info = json!({"formats": [{"ext": "mp4", "url": "https://x/v.mp4"}]});
        let c = summarize(&info);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].label, "Best available");
        assert_eq!(c[0].spec, "best");
    }

    #[test]
    fn single_format_info_without_formats_array() {
        // some extractors return a single-format info dict with no "formats" list
        let info = json!({"ext": "mp4", "url": "https://x/v.mp4"});
        let c = summarize(&info);
        assert_eq!(c[0].label, "Best available");
        assert_eq!(c[0].chips, vec!["mp4"]);
    }

    #[test]
    fn podcast_audio_only() {
        let info = json!({"duration": 3600, "formats": [
            {"vcodec": "none", "acodec": "mp3", "ext": "mp3", "abr": 128}
        ]});
        let c = summarize(&info);
        assert_eq!(labels_sizes(&c), vec![("Audio only".into(), "~55 MB".into())]);
        assert_eq!(c[0].chips, vec!["mp3"]);
    }
}
