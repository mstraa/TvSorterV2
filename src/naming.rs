use once_cell::sync::Lazy;
use regex::Regex;
use std::path::{Path, PathBuf};

static INVALID_CHARS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"[<>:"/\\|?*\x00-\x1f]"#).unwrap());
static WHITESPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

pub fn sanitize_component(value: &str) -> String {
    let cleaned = INVALID_CHARS.replace_all(value, " ");
    let cleaned = WHITESPACE_RE.replace_all(&cleaned, " ");
    let cleaned = cleaned.trim();
    let cleaned = cleaned.trim_end_matches([' ', '.']).trim();
    if cleaned.is_empty() {
        "Unknown".to_string()
    } else {
        cleaned.to_string()
    }
}

pub fn show_folder_name(title: &str, year: Option<i64>) -> String {
    let safe_title = sanitize_component(title);
    match year {
        Some(year) => format!("{safe_title} ({year})"),
        None => safe_title,
    }
}

fn extension(source_path: &Path) -> String {
    source_path
        .extension()
        .map(|ext| format!(".{}", ext.to_string_lossy()))
        .unwrap_or_default()
}

pub fn episode_filename(
    title: &str,
    year: Option<i64>,
    season: i64,
    episode: i64,
    episode_title: &str,
    quality: &str,
    extension: &str,
) -> String {
    let show = show_folder_name(title, year);
    let safe_episode_title = sanitize_component(episode_title);
    let safe_quality = sanitize_component(quality);
    format!("{show} - S{season:02}E{episode:02} - {safe_episode_title} - {safe_quality}{extension}")
}

#[allow(clippy::too_many_arguments)]
pub fn destination_path(
    output_root: &Path,
    title: &str,
    year: Option<i64>,
    season: i64,
    episode: i64,
    episode_title: &str,
    quality: &str,
    source_path: &Path,
) -> PathBuf {
    let show = show_folder_name(title, year);
    let season_dir = format!("Season {season:02}");
    let filename = episode_filename(
        title,
        year,
        season,
        episode,
        episode_title,
        quality,
        &extension(source_path),
    );
    output_root.join(show).join(season_dir).join(filename)
}

pub fn film_destination_path(
    output_root: &Path,
    title: &str,
    year: Option<i64>,
    quality: &str,
    source_path: &Path,
) -> PathBuf {
    let show = show_folder_name(title, year);
    let safe_quality = sanitize_component(quality);
    let filename = format!("{show} - {safe_quality}{}", extension(source_path));
    output_root.join(filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn builds_tv_destination() {
        let path = destination_path(
            &PathBuf::from("/out/TV"),
            "Fringe",
            Some(2008),
            1,
            1,
            "Pilot",
            "1080p",
            &PathBuf::from("a.mkv"),
        );
        assert_eq!(
            path,
            PathBuf::from("/out/TV/Fringe (2008)/Season 01/Fringe (2008) - S01E01 - Pilot - 1080p.mkv")
        );
    }

    #[test]
    fn builds_film_destination() {
        let path = film_destination_path(
            &PathBuf::from("/out/Films"),
            "Blade Runner 2049",
            Some(2017),
            "2160p",
            &PathBuf::from("br.mkv"),
        );
        assert_eq!(
            path,
            PathBuf::from("/out/Films/Blade Runner 2049 (2017) - 2160p.mkv")
        );
    }

    #[test]
    fn sanitizes_invalid_chars() {
        assert_eq!(sanitize_component("a/b:c?"), "a b c");
        assert_eq!(sanitize_component("   "), "Unknown");
    }
}
