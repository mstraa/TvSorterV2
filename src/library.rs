use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::db::Database;
use crate::filesystem::is_video_file;

fn walk_videos(dir: &Path, out: &mut Vec<PathBuf>) {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(_) => return,
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_videos(&path, out);
        } else if is_video_file(&path) {
            out.push(path);
        }
    }
}

/// Discover existing media files under each configured output root and
/// reconcile the library table, marking removed files as missing.
pub fn rescan_outputs(db: &Database, roots: &HashMap<String, PathBuf>) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for (media_type, root) in roots {
        counts.insert(media_type.clone(), 0usize);
        if root.as_os_str().is_empty() || !root.exists() {
            continue;
        }
        let mut files = Vec::new();
        walk_videos(root, &mut files);
        for path in &files {
            let canonical = crate::filesystem::canonical_or_normalized(path);
            db.upsert_discovered_file(media_type, &canonical);
        }
        counts.insert(media_type.clone(), files.len());
    }
    db.mark_missing_outside(roots);
    counts
}
