use std::fs;
use std::path::{Component, Path, PathBuf};

pub const VIDEO_EXTENSIONS: &[&str] = &[
    "avi", "m2ts", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "ts", "webm", "wmv",
];

#[derive(Debug, thiserror::Error)]
pub enum FsError {
    #[error("Path is outside the configured root")]
    OutsideRoot,
    #[error("Not a directory: {0}")]
    NotADirectory(String),
    #[error("{0}")]
    Io(String),
}

#[derive(Clone, Debug)]
pub struct BrowserEntry {
    pub name: String,
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub is_dir: bool,
    pub size: Option<i64>,
    pub is_video: bool,
}

/// Lexically normalize an absolute-ish path, resolving `.` and `..` without
/// touching the filesystem. Used as a fallback when `canonicalize` fails
/// (e.g. the path does not yet exist).
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    result
}

pub fn canonical_or_normalized(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| lexical_normalize(path))
}

pub fn is_relative_to(path: &Path, root: &Path) -> bool {
    let path = canonical_or_normalized(path);
    let root = canonical_or_normalized(root);
    path.starts_with(&root)
}

pub fn resolve_under_root(root: &Path, relative_path: &str) -> Result<PathBuf, FsError> {
    let root_c = canonical_or_normalized(root);
    let joined = root.join(relative_path);
    let target = canonical_or_normalized(&joined);
    if !target.starts_with(&root_c) {
        return Err(FsError::OutsideRoot);
    }
    Ok(target)
}

pub fn is_video_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    match path.extension() {
        Some(ext) => {
            let ext = ext.to_string_lossy().to_lowercase();
            VIDEO_EXTENSIONS.contains(&ext.as_str())
        }
        None => false,
    }
}

pub fn list_directory(root: &Path, relative_path: &str) -> Result<Vec<BrowserEntry>, FsError> {
    let directory = resolve_under_root(root, relative_path)?;
    if !directory.is_dir() {
        return Err(FsError::NotADirectory(directory.display().to_string()));
    }
    let root_c = canonical_or_normalized(root);
    let read = fs::read_dir(&directory).map_err(|e| FsError::Io(e.to_string()))?;
    let mut children: Vec<PathBuf> = read.filter_map(|entry| entry.ok().map(|e| e.path())).collect();
    children.sort_by(|a, b| {
        let a_dir = a.is_dir();
        let b_dir = b.is_dir();
        // Folders first, then case-insensitive name.
        (!a_dir)
            .cmp(&!b_dir)
            .then_with(|| name_lower(a).cmp(&name_lower(b)))
    });

    let mut entries = Vec::new();
    for child in children {
        let metadata = match fs::metadata(&child) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        let child_c = canonical_or_normalized(&child);
        let relative = match child_c.strip_prefix(&root_c) {
            Ok(rel) => rel.to_string_lossy().to_string(),
            Err(_) => continue,
        };
        let is_dir = metadata.is_dir();
        entries.push(BrowserEntry {
            name: child
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            relative_path: relative,
            absolute_path: child.clone(),
            is_dir,
            size: if is_dir {
                None
            } else {
                Some(metadata.len() as i64)
            },
            is_video: is_video_file(&child),
        });
    }
    Ok(entries)
}

fn name_lower(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn collect_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(_) => return,
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn expand_files(
    root: &Path,
    relative_paths: &[String],
    video_only: bool,
) -> Result<Vec<PathBuf>, FsError> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for relative_path in relative_paths {
        let target = resolve_under_root(root, relative_path)?;
        let mut candidates: Vec<PathBuf> = Vec::new();
        if target.is_dir() {
            collect_files_recursive(&target, &mut candidates);
        } else {
            candidates.push(target);
        }
        for candidate in candidates {
            if candidate.is_file() && (!video_only || is_video_file(&candidate)) {
                let resolved = canonical_or_normalized(&candidate);
                if seen.insert(resolved.clone()) {
                    files.push(resolved);
                }
            }
        }
    }
    files.sort();
    Ok(files)
}

pub fn expand_video_files(root: &Path, relative_paths: &[String]) -> Result<Vec<PathBuf>, FsError> {
    expand_files(root, relative_paths, true)
}

pub fn expand_source_files(root: &Path, relative_paths: &[String]) -> Result<Vec<PathBuf>, FsError> {
    expand_files(root, relative_paths, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};

    #[test]
    fn rejects_path_traversal() {
        let dir = std::env::temp_dir().join(format!("tvsorter-fs-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("sub")).unwrap();
        File::create(dir.join("sub/a.mkv")).unwrap();
        assert!(resolve_under_root(&dir, "../etc").is_err());
        assert!(resolve_under_root(&dir, "sub").is_ok());
        let videos = expand_video_files(&dir, &["sub".to_string()]).unwrap();
        assert_eq!(videos.len(), 1);
        fs::remove_dir_all(&dir).ok();
    }
}
