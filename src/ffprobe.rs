use std::path::Path;
use std::process::Command;

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
}
