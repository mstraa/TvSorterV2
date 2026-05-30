use std::collections::HashMap;
use std::path::PathBuf;

use crate::db::Database;
use crate::filesystem::{collect_files, is_audio_file, is_video_file};

/// Discover existing media files under each configured output root and
/// reconcile the library table, marking removed files as missing.
pub fn rescan_outputs(db: &Database, roots: &HashMap<String, PathBuf>) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for (media_type, root) in roots {
        counts.insert(media_type.clone(), 0usize);
        if root.as_os_str().is_empty() || !root.exists() {
            continue;
        }
        // Music output roots hold audio files; everything else is video.
        let keep: &dyn Fn(&std::path::Path) -> bool = if media_type == "music" {
            &is_audio_file
        } else {
            &is_video_file
        };
        let mut files = Vec::new();
        collect_files(root, keep, &mut files);
        let canonical: Vec<PathBuf> = files
            .iter()
            .map(|p| crate::filesystem::canonical_or_normalized(p))
            .collect();
        db.upsert_discovered_files(media_type, &canonical);
        counts.insert(media_type.clone(), files.len());
    }
    db.mark_missing_outside(roots);
    counts
}
