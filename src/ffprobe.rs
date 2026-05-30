use std::path::Path;
use std::process::Command;

/// Audio metadata read from a file's container/format tags via ffprobe.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MusicTags {
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<i64>,
}

/// Probe a video file's vertical resolution with `ffprobe` and map it to a
/// quality label. Returns `None` when ffprobe is unavailable or the height
/// cannot be determined. This is the V1 "ffprobe resolution fallback" gap.
pub fn probe_quality(path: &Path) -> Option<String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=height",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let height: i64 = text.lines().next()?.trim().parse().ok()?;
    Some(quality_from_height(height))
}

pub fn quality_from_height(height: i64) -> String {
    // Map by nearest standard tier using inclusive lower bounds.
    if height >= 1800 {
        "2160p".to_string()
    } else if height >= 900 {
        "1080p".to_string()
    } else if height >= 600 {
        "720p".to_string()
    } else if height >= 380 {
        "480p".to_string()
    } else {
        "Unknown".to_string()
    }
}

/// Probe an audio file's `format` tags with `ffprobe` and extract artist,
/// album, and year. Returns all-`None` fields when ffprobe is unavailable or
/// the tags are absent. Never panics: every failure degrades to `None`.
pub fn probe_music_tags(path: &Path) -> MusicTags {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            // Tag keys are case-insensitive in practice; request both common
            // casings plus album_artist so we can fall back on it.
            "format_tags=artist,album,date,year,album_artist,ARTIST,ALBUM,DATE,YEAR,ALBUM_ARTIST",
            "-of",
            "json",
        ])
        .arg(path)
        .output();
    let output = match output {
        Ok(output) if output.status.success() => output,
        _ => return MusicTags::default(),
    };
    parse_music_tags_json(&String::from_utf8_lossy(&output.stdout))
}

/// Parse the ffprobe `-of json` output for `format_tags`. Split out so it can be
/// unit-tested without invoking ffprobe.
fn parse_music_tags_json(text: &str) -> MusicTags {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(_) => return MusicTags::default(),
    };
    let tags = match value.get("format").and_then(|f| f.get("tags")) {
        Some(tags) => tags,
        None => return MusicTags::default(),
    };

    // Case-insensitive lookup over the tag object: returns the first non-empty
    // string value whose key matches any of `keys` (compared lowercased).
    let lookup = |keys: &[&str]| -> Option<String> {
        let object = tags.as_object()?;
        for (key, value) in object {
            let lower = key.to_lowercase();
            if keys.contains(&lower.as_str()) {
                if let Some(text) = value.as_str() {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
        None
    };

    let artist = lookup(&["artist"]).or_else(|| lookup(&["album_artist"]));
    let album = lookup(&["album"]);
    let year = lookup(&["date", "year"]).and_then(|d| first_year(&d));
    MusicTags {
        artist,
        album,
        year,
    }
}

/// Extract the first 4-digit run from a date/year tag (e.g. "2001-05-04" -> 2001).
fn first_year(value: &str) -> Option<i64> {
    let mut digits = String::new();
    for ch in value.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            if digits.len() == 4 {
                return digits.parse().ok();
            }
        } else {
            digits.clear();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_heights() {
        assert_eq!(quality_from_height(2160), "2160p");
        assert_eq!(quality_from_height(1080), "1080p");
        assert_eq!(quality_from_height(720), "720p");
        assert_eq!(quality_from_height(480), "480p");
        assert_eq!(quality_from_height(120), "Unknown");
    }

    #[test]
    fn parses_music_tags_case_insensitive() {
        let json = r#"{
            "format": {
                "tags": {
                    "ARTIST": "Daft Punk",
                    "Album": "Discovery",
                    "date": "2001-03-12"
                }
            }
        }"#;
        let tags = parse_music_tags_json(json);
        assert_eq!(tags.artist.as_deref(), Some("Daft Punk"));
        assert_eq!(tags.album.as_deref(), Some("Discovery"));
        assert_eq!(tags.year, Some(2001));
    }

    #[test]
    fn parses_music_tags_album_artist_fallback() {
        let json = r#"{"format":{"tags":{"album_artist":"Various","album":"Mix","year":"1999"}}}"#;
        let tags = parse_music_tags_json(json);
        assert_eq!(tags.artist.as_deref(), Some("Various"));
        assert_eq!(tags.year, Some(1999));
    }

    #[test]
    fn music_tags_empty_when_no_tags() {
        assert_eq!(parse_music_tags_json("{}"), MusicTags::default());
        assert_eq!(parse_music_tags_json("not json"), MusicTags::default());
    }

    #[test]
    fn first_year_from_date() {
        assert_eq!(first_year("2001-05-04"), Some(2001));
        assert_eq!(first_year("May 2010"), Some(2010));
        assert_eq!(first_year("no year"), None);
    }
}
