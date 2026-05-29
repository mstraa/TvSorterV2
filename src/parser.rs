use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::path::Path;

static QUALITY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(2160p|1080p|720p|480p)\b").unwrap());
static RELEASE_TRAIL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(2160p|1080p|720p|480p|multi|vff|vfq|vf2|vf|truefrench|french|hdrip|web[ ._-]?dl|webrip|hdtv|bluray|bdrip|brrip|dvdrip|x264|x265|h[ ._-]?264|h[ ._-]?265|hevc|aac|ac3|ddp?5?[ ._-]?1|proper|repack)\b.*$",
    )
    .unwrap()
});
static YEAR_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|[\s._(-])((?:19|20)\d{2})(?:$|[\s._)-])").unwrap());
static YEAR_TOKEN_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?:19|20)\d{2}").unwrap());
static SXXEYY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\bS(?P<season>\d{1,2})E(?P<episode>\d{1,3})\b").unwrap());
static ONE_X_TWO_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(?P<season>\d{1,2})x(?P<episode>\d{1,3})\b").unwrap());
static EYY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(?:^|[\s._-])E(?P<episode>\d{1,3})(?:$|[\s._-])").unwrap());
static SEASON_EP_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\bseason[\s._-]*(?P<season>\d{1,2})[\s._-]*episode[\s._-]*(?P<episode>\d{1,3})\b")
        .unwrap()
});
static BRACKETS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\[\](){}]").unwrap());
static SEPARATORS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[._-]+").unwrap());
static WHITESPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ParsedMedia {
    pub source_name: String,
    pub title: String,
    pub year: Option<i64>,
    pub season: i64,
    pub episode: i64,
    pub episode_title: String,
    pub quality: String,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default()
}

struct EpisodeMatch {
    season: i64,
    episode: i64,
    start: Option<usize>,
    end: Option<usize>,
}

fn find_episode(stem: &str) -> EpisodeMatch {
    for re in [&*SXXEYY_RE, &*ONE_X_TWO_RE, &*SEASON_EP_RE] {
        if let Some(caps) = re.captures(stem) {
            let m = caps.get(0).unwrap();
            return EpisodeMatch {
                season: caps["season"].parse().unwrap_or(1),
                episode: caps["episode"].parse().unwrap_or(1),
                start: Some(m.start()),
                end: Some(m.end()),
            };
        }
    }
    if let Some(caps) = EYY_RE.captures(stem) {
        let m = caps.get(0).unwrap();
        return EpisodeMatch {
            season: 1,
            episode: caps["episode"].parse().unwrap_or(1),
            start: Some(m.start()),
            end: Some(m.end()),
        };
    }
    EpisodeMatch {
        season: 1,
        episode: 1,
        start: None,
        end: None,
    }
}

pub fn detect_quality(value: &str) -> String {
    QUALITY_RE
        .captures(value)
        .map(|caps| caps[1].to_lowercase())
        .unwrap_or_else(|| "Unknown".to_string())
}

fn find_year(stem: &str) -> Option<i64> {
    YEAR_RE
        .captures(stem)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

fn extract_film_year(stem: &str) -> (Option<i64>, String) {
    let matches: Vec<_> = YEAR_TOKEN_RE.find_iter(stem).collect();
    match matches.last() {
        None => (None, stem.to_string()),
        Some(m) => {
            let title_stem = format!("{} {}", &stem[..m.start()], &stem[m.end()..]);
            (m.as_str().parse().ok(), title_stem)
        }
    }
}

fn clean_tokens(value: &str) -> String {
    let value = BRACKETS_RE.replace_all(value, " ");
    let value = SEPARATORS_RE.replace_all(&value, " ");
    let value = WHITESPACE_RE.replace_all(&value, " ");
    title_case(value.trim())
}

/// Mimics Python's str.title(): uppercase the first letter of each run of
/// alphabetic characters, lowercase the rest.
fn title_case(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut prev_alpha = false;
    for ch in value.chars() {
        if ch.is_alphabetic() {
            if prev_alpha {
                result.extend(ch.to_lowercase());
            } else {
                result.extend(ch.to_uppercase());
            }
            prev_alpha = true;
        } else {
            result.push(ch);
            prev_alpha = false;
        }
    }
    result
}

fn clean_title(value: &str) -> String {
    let value = YEAR_RE.replace_all(value, " ");
    clean_tokens(&value)
}

fn clean_film_title(value: &str) -> String {
    let value = RELEASE_TRAIL_RE.replace_all(value, " ");
    clean_tokens(&value)
}

fn clean_episode_title(value: &str) -> String {
    let value = RELEASE_TRAIL_RE.replace_all(value, " ");
    clean_tokens(&value)
}

pub fn parse_media_filename(path: &Path) -> ParsedMedia {
    let source_name = file_name(path);
    let stem = file_stem(path);
    let episode = find_episode(&stem);
    let quality = detect_quality(&stem);
    let year = find_year(&stem);
    let title_source = match episode.start {
        Some(start) => &stem[..start],
        None => &stem,
    };
    let title = clean_title(title_source);
    let episode_title_source = match episode.end {
        Some(end) => &stem[end..],
        None => "",
    };
    let episode_title = clean_episode_title(episode_title_source);
    ParsedMedia {
        source_name,
        title: if title.is_empty() {
            "Unknown Show".to_string()
        } else {
            title
        },
        year,
        season: episode.season,
        episode: episode.episode,
        episode_title: if episode_title.is_empty() {
            "Episode".to_string()
        } else {
            episode_title
        },
        quality,
    }
}

pub fn parse_film_filename(path: &Path) -> ParsedMedia {
    let source_name = file_name(path);
    let stem = file_stem(path);
    let quality = detect_quality(&stem);
    let (year, title_stem) = extract_film_year(&stem);
    let title = clean_film_title(&title_stem);
    ParsedMedia {
        source_name,
        title: if title.is_empty() {
            "Unknown Film".to_string()
        } else {
            title
        },
        year,
        season: 0,
        episode: 0,
        episode_title: "Film".to_string(),
        quality,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parses_standard_tv_filename() {
        let parsed = parse_media_filename(&PathBuf::from("Fringe.2008.S01E01.Pilot.1080p.mkv"));
        assert_eq!(parsed.title, "Fringe");
        assert_eq!(parsed.year, Some(2008));
        assert_eq!(parsed.season, 1);
        assert_eq!(parsed.episode, 1);
        assert_eq!(parsed.episode_title, "Pilot");
        assert_eq!(parsed.quality, "1080p");
    }

    #[test]
    fn parses_anime_e_only() {
        let parsed = parse_media_filename(&PathBuf::from("[Group] Cowboy Bebop - E05 [720p].mkv"));
        assert_eq!(parsed.season, 1);
        assert_eq!(parsed.episode, 5);
        assert_eq!(parsed.quality, "720p");
    }

    #[test]
    fn parses_film_year() {
        let parsed = parse_film_filename(&PathBuf::from("Blade Runner 2049 2017 2160p.mkv"));
        assert_eq!(parsed.title, "Blade Runner 2049");
        assert_eq!(parsed.year, Some(2017));
        assert_eq!(parsed.quality, "2160p");
    }

    #[test]
    fn unknown_quality_defaults() {
        let parsed = parse_media_filename(&PathBuf::from("Show.S02E03.mkv"));
        assert_eq!(parsed.quality, "Unknown");
        assert_eq!(parsed.season, 2);
        assert_eq!(parsed.episode, 3);
    }
}
