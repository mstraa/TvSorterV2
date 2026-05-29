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
// `(?:v\d{1,2})?` tolerates anime version suffixes glued to the episode number
// (e.g. "05v2"), so the right episode is detected instead of falling through.
static SXXEYY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\bS(?P<season>\d{1,2})E(?P<episode>\d{1,3})(?:v\d{1,2})?\b").unwrap());
static ONE_X_TWO_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(?P<season>\d{1,2})x(?P<episode>\d{1,3})(?:v\d{1,2})?\b").unwrap());
static EYY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(?:^|[\s._-])E(?P<episode>\d{1,3})(?:v\d{1,2})?(?:$|[\s._-])").unwrap());
static SEASON_EP_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\bseason[\s._-]*(?P<season>\d{1,2})[\s._-]*episode[\s._-]*(?P<episode>\d{1,3})\b")
        .unwrap()
});
static BRACKETS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\[\](){}]").unwrap());
static SEPARATORS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"[._-]+").unwrap());
static WHITESPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

// Season detection from folder names: a whole segment like "Season 1", "Saison 02",
// "Series 3", "S01" — or an inline "season 2" inside a longer segment.
static SEASON_SEGMENT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^(?:season|saison|series|s)[\s._-]*0*(\d{1,2})$").unwrap());
static SEASON_INLINE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(?:season|saison|series)[\s._-]*0*(\d{1,2})\b").unwrap());

// Permissive episode markers for files that only carry a number, e.g.
// "12 - Title", "Episode 05", "Ep 7", "OVA 3", or a trailing "- 07".
static EP_KEYWORD_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:episode|épisode|ep|e|ova|oav|oad|#)[\s._-]*0*(?P<episode>\d{1,3})(?P<ver>v\d{1,2})?\b")
        .unwrap()
});
static LEADING_NUM_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[\s._-]*0*(?P<episode>\d{1,3})(?P<ver>v\d{1,2})?(?:$|[\s._-])").unwrap());
// A standalone number, optionally with a version suffix ("05v2") so the "v"
// doesn't block the match.
static STANDALONE_NUM_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)(?:^|[\s._-])0*(?P<episode>\d{1,3})(?P<ver>v\d{1,2})?(?:$|[\s._-])").unwrap());

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

/// Like `find_episode`, but for files that only carry an episode number with no
/// season marker (common when the show/season lives in the folder structure).
/// Falls back through keyword markers, a leading number, then any standalone
/// 1-3 digit number that is not a 4-digit year.
fn find_episode_loose(stem: &str) -> EpisodeMatch {
    let structured = find_episode(stem);
    if structured.start.is_some() {
        return structured;
    }
    for re in [&*EP_KEYWORD_RE, &*LEADING_NUM_RE, &*STANDALONE_NUM_RE] {
        if let Some(caps) = re.captures(stem) {
            let group = caps.name("episode").unwrap();
            // Skip the version suffix ("v2") so it doesn't leak into the title.
            let end = caps.name("ver").map(|m| m.end()).unwrap_or_else(|| group.end());
            return EpisodeMatch {
                season: 1,
                episode: group.as_str().parse().unwrap_or(1),
                start: Some(group.start()),
                end: Some(end),
            };
        }
    }
    structured
}

/// Detect a season number from the directory segments leading up to a file
/// (deepest folder first). Returns `None` when no segment looks like a season.
pub fn season_from_segments(segments: &[&str]) -> Option<i64> {
    for segment in segments.iter().rev() {
        if let Some(caps) = SEASON_SEGMENT_RE.captures(segment) {
            return caps[1].parse().ok();
        }
        if let Some(caps) = SEASON_INLINE_RE.captures(segment) {
            return caps[1].parse().ok();
        }
    }
    None
}

/// Build a `ParsedMedia` for an episode whose show identity comes from the
/// enclosing folder (`show_title`) rather than the filename. `season_hint` is the
/// season derived from the folder structure, when available; the filename can
/// still override it (e.g. an explicit S02E03).
pub fn parse_folder_episode(
    path: &Path,
    show_title: &str,
    season_hint: Option<i64>,
) -> ParsedMedia {
    let source_name = file_name(path);
    let stem = file_stem(path);
    let episode = find_episode_loose(&stem);
    let quality = detect_quality(&stem);

    // A season explicitly present in the filename wins; otherwise use the folder
    // hint; otherwise default to 1.
    let filename_season = if find_episode(&stem).start.is_some() {
        Some(episode.season)
    } else {
        None
    };
    let season = filename_season.or(season_hint).unwrap_or(1);

    let episode_title_source = match (episode.end, episode.start) {
        (Some(end), _) => &stem[end..],
        (None, _) => stem.as_str(),
    };
    let episode_title = clean_episode_title(episode_title_source);

    ParsedMedia {
        source_name,
        title: if show_title.is_empty() {
            "Unknown Show".to_string()
        } else {
            show_title.to_string()
        },
        year: None,
        season,
        episode: episode.episode,
        episode_title: if episode_title.is_empty() {
            "Episode".to_string()
        } else {
            episode_title
        },
        quality,
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

/// Derive a clean show title and optional year from a folder name, e.g.
/// "Black Lagoon (2006)" -> ("Black Lagoon", Some(2006)).
pub fn show_title_from_folder(name: &str) -> (String, Option<i64>) {
    let year = find_year(name);
    let title = clean_title(name);
    let title = if title.is_empty() {
        name.trim().to_string()
    } else {
        title
    };
    (title, year)
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
    fn folder_episode_reads_embedded_sxxeyy_and_title() {
        let parsed = parse_folder_episode(
            &PathBuf::from("Fringe.S01E01.Pilot.1080p.mkv"),
            "Fringe",
            None,
        );
        assert_eq!(parsed.title, "Fringe");
        assert_eq!(parsed.season, 1);
        assert_eq!(parsed.episode, 1);
        assert_eq!(parsed.episode_title, "Pilot");
        assert_eq!(parsed.quality, "1080p");
    }

    #[test]
    fn folder_episode_version_suffix() {
        let parsed = parse_folder_episode(
            &PathBuf::from("[I-R]Yakitate_Japan_05v2_xvid_vostfr.avi"),
            "Yakitate Japan",
            None,
        );
        assert_eq!(parsed.episode, 5);
        assert_eq!(parsed.title, "Yakitate Japan");
    }

    #[test]
    fn folder_episode_sxxeyy_version_suffix() {
        let parsed =
            parse_folder_episode(&PathBuf::from("Show.S02E07v2.mkv"), "Show", None);
        assert_eq!(parsed.season, 2);
        assert_eq!(parsed.episode, 7);
    }

    #[test]
    fn folder_episode_e_only() {
        let parsed = parse_folder_episode(
            &PathBuf::from("[Group] Cowboy Bebop - E05 [720p].mkv"),
            "Cowboy Bebop",
            None,
        );
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
    fn season_from_folder_segments() {
        assert_eq!(season_from_segments(&["Black Lagoon", "Season 2"]), Some(2));
        assert_eq!(season_from_segments(&["Black Lagoon", "Saison 03"]), Some(3));
        assert_eq!(season_from_segments(&["Show", "S01"]), Some(1));
        assert_eq!(season_from_segments(&["Show", "Extras"]), None);
    }

    #[test]
    fn folder_episode_uses_folder_title_and_season() {
        let parsed = parse_folder_episode(
            &PathBuf::from("12 - The Black Lagoon.mkv"),
            "Black Lagoon",
            Some(2),
        );
        assert_eq!(parsed.title, "Black Lagoon");
        assert_eq!(parsed.season, 2);
        assert_eq!(parsed.episode, 12);
        assert_eq!(parsed.episode_title, "The Black Lagoon");
    }

    #[test]
    fn folder_episode_filename_season_overrides_hint() {
        let parsed =
            parse_folder_episode(&PathBuf::from("S03E04 - Title.mkv"), "Some Show", Some(1));
        assert_eq!(parsed.season, 3);
        assert_eq!(parsed.episode, 4);
    }

    #[test]
    fn folder_title_strips_year() {
        let (title, year) = show_title_from_folder("Black Lagoon (2006)");
        assert_eq!(title, "Black Lagoon");
        assert_eq!(year, Some(2006));
    }

    #[test]
    fn unknown_quality_defaults() {
        let parsed = parse_folder_episode(&PathBuf::from("Show.S02E03.mkv"), "Show", None);
        assert_eq!(parsed.quality, "Unknown");
        assert_eq!(parsed.season, 2);
        assert_eq!(parsed.episode, 3);
    }
}
