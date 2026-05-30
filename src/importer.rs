use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;

use crate::db::ImportRecord;
use crate::naming::{destination_path, film_destination_path, music_destination_path};

// errno values shared by Linux and macOS.
const EXDEV: i32 = 18;
const EACCES: i32 = 13;
const EPERM: i32 = 1;

/// Extended-attribute name under which the source's original relative path is
/// recorded on copied/moved destination files.
const ORIGIN_XATTR: &str = "user.tvsorter.origin";

#[derive(Clone, Debug)]
pub struct ImportRequest {
    pub source_path: PathBuf,
    pub output_root: PathBuf,
    pub media_type: String,
    pub show_title: String,
    pub show_year: Option<i64>,
    pub season_number: i64,
    pub episode_number: i64,
    pub episode_title: String,
    pub quality: String,
    pub action: String,
    pub conflict_policy: String,
    pub provider: Option<String>,
    pub provider_show_id: Option<String>,
    /// Source path relative to its input root (folder structure included), e.g.
    /// "Spiderman/Saison 01/episode-01.mkv". Written to the destination file as
    /// the `user.tvsorter.origin` extended attribute after a copy/move.
    pub origin: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ImportResult {
    #[serde(skip)]
    pub request: ImportRequest,
    pub source_path: String,
    pub output_path: String,
    pub final_path: String,
    pub result: String,
    pub error: Option<String>,
}

// `request` is not serialized; expose just the fields the UI needs.
impl ImportResult {
    fn new(
        request: ImportRequest,
        output_path: PathBuf,
        final_path: PathBuf,
        result: &str,
    ) -> Self {
        Self {
            source_path: request.source_path.to_string_lossy().to_string(),
            output_path: output_path.to_string_lossy().to_string(),
            final_path: final_path.to_string_lossy().to_string(),
            result: result.to_string(),
            error: None,
            request,
        }
    }

    fn with_error(
        request: ImportRequest,
        output_path: PathBuf,
        final_path: PathBuf,
        result: &str,
        error: String,
    ) -> Self {
        let mut value = Self::new(request, output_path, final_path, result);
        value.error = Some(error);
        value
    }
}

/// Callbacks are invoked synchronously on the worker thread that runs the
/// import, so they need no Send/Sync bounds beyond their own captures.
pub type ProgressFn<'a> = &'a dyn Fn(u64, u64);
pub type CancelFn<'a> = &'a dyn Fn() -> bool;

enum CopyOutcome {
    Done,
    Cancelled,
}

/// Outcome of a copy attempt, with any partial output already cleaned up. Lets
/// the `copy` and `move` actions share identical cancel/failure handling.
enum CopyStep {
    Done,
    Cancelled,
    Failed(std::io::Error),
}

pub fn build_destination(request: &ImportRequest) -> PathBuf {
    if request.media_type == "music" {
        // For music: show_title=Artist, episode_title=Album, show_year=Year.
        music_destination_path(
            &request.output_root,
            &request.show_title,
            &request.episode_title,
            request.show_year,
            &request.source_path,
        )
    } else if request.media_type == "film" {
        film_destination_path(
            &request.output_root,
            &request.show_title,
            request.show_year,
            &request.quality,
            &request.source_path,
        )
    } else {
        destination_path(
            &request.output_root,
            &request.show_title,
            request.show_year,
            request.season_number,
            request.episode_number,
            &request.episode_title,
            &request.quality,
            &request.source_path,
        )
    }
}

#[derive(Debug)]
enum ConflictError {
    Exists(String),
}

fn apply_conflict_policy(path: &Path, policy: &str) -> Result<PathBuf, ConflictError> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    match policy {
        "skip" | "replace" => Ok(path.to_path_buf()),
        "fail" => Err(ConflictError::Exists(format!(
            "Destination already exists: {}",
            path.display()
        ))),
        "index" => indexed_path(path),
        other => Err(ConflictError::Exists(format!(
            "Unsupported conflict policy: {other}"
        ))),
    }
}

fn indexed_path(path: &Path) -> Result<PathBuf, ConflictError> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let suffix = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for index in 2..1000 {
        let candidate = path.with_file_name(format!("{stem} ({index}){suffix}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(ConflictError::Exists(format!(
        "No available indexed destination for: {}",
        path.display()
    )))
}

pub fn preview_import(request: ImportRequest) -> ImportResult {
    let output_path = build_destination(&request);
    match apply_conflict_policy(&output_path, &request.conflict_policy) {
        Ok(final_path) => {
            let result = if output_path.exists() && final_path == output_path {
                "conflict"
            } else {
                "preview"
            };
            ImportResult::new(request, output_path, final_path, result)
        }
        Err(ConflictError::Exists(message)) => {
            ImportResult::with_error(request, output_path.clone(), output_path, "failed", message)
        }
    }
}

pub fn execute_import(
    request: ImportRequest,
    progress: Option<ProgressFn<'_>>,
    copy_rate_limit_mbps: Option<f64>,
    cancel: Option<CancelFn<'_>>,
) -> ImportResult {
    let output_path = build_destination(&request);
    let final_path = match apply_conflict_policy(&output_path, &request.conflict_policy) {
        Ok(path) => path,
        Err(ConflictError::Exists(message)) => {
            return ImportResult::with_error(
                request,
                output_path.clone(),
                output_path,
                "failed",
                message,
            );
        }
    };

    if output_path.exists() && request.conflict_policy == "skip" {
        return ImportResult::new(request, output_path.clone(), output_path, "skipped");
    }

    if request.action == "test" {
        return ImportResult::new(request, output_path, final_path, "preview");
    }

    if cancelled(cancel) {
        return cancelled_result(request, output_path, final_path);
    }

    if let Some(parent) = final_path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            return io_failure(request, output_path, final_path, &err);
        }
    }

    if final_path.exists() && request.conflict_policy == "replace" {
        let _ = fs::remove_file(&final_path);
    }

    let action = request.action.clone();
    match action.as_str() {
        "hardlink" => {
            if cancelled(cancel) {
                return cancelled_result(request, output_path, final_path);
            }
            if let Err(err) = fs::hard_link(&request.source_path, &final_path) {
                if err.raw_os_error() == Some(EXDEV) {
                    return ImportResult::with_error(
                        request,
                        output_path,
                        final_path,
                        "failed",
                        "Hardlink failed because source and destination are on different filesystems."
                            .to_string(),
                    );
                }
                return io_failure(request, output_path, final_path, &err);
            }
            if let Some(progress) = progress {
                progress(1, 1);
            }
        }
        "copy" => match copy_step(
            &request.source_path,
            &final_path,
            progress,
            copy_rate_limit_mbps,
            cancel,
        ) {
            CopyStep::Done => {}
            CopyStep::Cancelled => return cancelled_result(request, output_path, final_path),
            CopyStep::Failed(err) => return io_failure(request, output_path, final_path, &err),
        },
        "move" => {
            // Try rename first (same filesystem, atomic and instant).
            match fs::rename(&request.source_path, &final_path) {
                Ok(()) => {
                    if let Some(progress) = progress {
                        progress(1, 1);
                    }
                }
                // Cross-device: copy then delete the source.
                Err(rename_err) if rename_err.raw_os_error() == Some(EXDEV) => {
                    match copy_step(
                        &request.source_path,
                        &final_path,
                        progress,
                        copy_rate_limit_mbps,
                        cancel,
                    ) {
                        CopyStep::Done => {
                            let _ = fs::remove_file(&request.source_path);
                        }
                        CopyStep::Cancelled => {
                            return cancelled_result(request, output_path, final_path)
                        }
                        CopyStep::Failed(err) => {
                            return io_failure(request, output_path, final_path, &err)
                        }
                    }
                }
                Err(rename_err) => {
                    return io_failure(request, output_path, final_path, &rename_err)
                }
            }
        }
        other => {
            return ImportResult::with_error(
                request,
                output_path,
                final_path,
                "failed",
                format!("Unsupported action: {other}"),
            );
        }
    }

    // Record the source's original relative path on copied/moved files. Hardlinks
    // share the source inode (and its xattrs) so they are intentionally excluded;
    // the "test" action never reaches here. xattr failures must not fail the
    // import — they are logged and ignored.
    if action == "copy" || action == "move" {
        write_origin_xattr(&final_path, &request.origin);
    }

    ImportResult::new(request, output_path, final_path, "imported")
}

/// Write the source's original relative path to the destination file as the
/// `user.tvsorter.origin` extended attribute. Best-effort: any failure (e.g. a
/// filesystem that does not support user xattrs) is logged and ignored so it
/// never fails an otherwise-successful import.
#[cfg(unix)]
fn write_origin_xattr(path: &Path, origin: &str) {
    if origin.is_empty() {
        return;
    }
    if let Err(err) = xattr::set(path, ORIGIN_XATTR, origin.as_bytes()) {
        tracing::warn!(
            "failed to set {ORIGIN_XATTR} xattr on {}: {err}",
            path.display()
        );
    }
}

#[cfg(not(unix))]
fn write_origin_xattr(_path: &Path, _origin: &str) {}

fn cancelled(cancel: Option<CancelFn<'_>>) -> bool {
    cancel.map(|c| c()).unwrap_or(false)
}

/// Build the standard "cancelled" result for an aborted import.
fn cancelled_result(
    request: ImportRequest,
    output_path: PathBuf,
    final_path: PathBuf,
) -> ImportResult {
    ImportResult::with_error(
        request,
        output_path,
        final_path,
        "cancelled",
        "Import cancelled.".to_string(),
    )
}

/// Copy `source` to `final_path`, cleaning up any partial output on
/// cancellation or failure. Shared by the `copy` and `move` actions.
fn copy_step(
    source: &Path,
    final_path: &Path,
    progress: Option<ProgressFn<'_>>,
    copy_rate_limit_mbps: Option<f64>,
    cancel: Option<CancelFn<'_>>,
) -> CopyStep {
    match copy_with_progress(source, final_path, progress, copy_rate_limit_mbps, cancel) {
        Ok(CopyOutcome::Done) => CopyStep::Done,
        Ok(CopyOutcome::Cancelled) => {
            remove_partial(final_path);
            CopyStep::Cancelled
        }
        Err(err) => {
            remove_partial(final_path);
            CopyStep::Failed(err)
        }
    }
}

fn copy_with_progress(
    source: &Path,
    destination: &Path,
    progress: Option<ProgressFn<'_>>,
    copy_rate_limit_mbps: Option<f64>,
    cancel: Option<CancelFn<'_>>,
) -> std::io::Result<CopyOutcome> {
    let total = fs::metadata(source)?.len();
    let mut copied: u64 = 0;
    let started_at = Instant::now();
    let chunk_size = 256 * 1024;
    let bytes_per_second = match copy_rate_limit_mbps {
        Some(limit) if limit > 0.0 => Some(limit * 1024.0 * 1024.0),
        _ => None,
    };

    let mut reader = fs::File::open(source)?;
    let mut writer = fs::File::create(destination)?;
    let mut buffer = vec![0u8; chunk_size];

    loop {
        if cancelled(cancel) {
            return Ok(CopyOutcome::Cancelled);
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        copied += read as u64;
        if let Some(progress) = progress {
            progress(copied, total);
        }
        if let Some(bps) = bytes_per_second {
            let expected = copied as f64 / bps;
            let actual = started_at.elapsed().as_secs_f64();
            if expected > actual {
                if let CopyOutcome::Cancelled = sleep_until_next_chunk(expected - actual, cancel) {
                    return Ok(CopyOutcome::Cancelled);
                }
            }
        }
    }
    writer.flush()?;
    drop(writer);
    copy_stat(source, destination);
    if let Some(progress) = progress {
        progress(total, total);
    }
    Ok(CopyOutcome::Done)
}

fn sleep_until_next_chunk(duration: f64, cancel: Option<CancelFn<'_>>) -> CopyOutcome {
    let deadline = Instant::now() + std::time::Duration::from_secs_f64(duration);
    loop {
        if cancelled(cancel) {
            return CopyOutcome::Cancelled;
        }
        let now = Instant::now();
        if now >= deadline {
            return CopyOutcome::Done;
        }
        let remaining = deadline - now;
        std::thread::sleep(remaining.min(std::time::Duration::from_millis(100)));
    }
}

/// Equivalent of Python's shutil.copystat: copy permissions and modification
/// time from source to destination.
fn copy_stat(source: &Path, destination: &Path) {
    if let Ok(meta) = fs::metadata(source) {
        let _ = fs::set_permissions(destination, meta.permissions());
        if let Ok(mtime) = meta.modified() {
            let ft = filetime::FileTime::from_system_time(mtime);
            let _ = filetime::set_file_mtime(destination, ft);
        }
    }
}

fn remove_partial(path: &Path) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

fn io_failure(
    request: ImportRequest,
    output_path: PathBuf,
    final_path: PathBuf,
    err: &std::io::Error,
) -> ImportResult {
    let message = format_os_error(err, &final_path);
    ImportResult::with_error(request, output_path, final_path, "failed", message)
}

fn format_os_error(err: &std::io::Error, destination: &Path) -> String {
    match err.raw_os_error() {
        Some(code) if code == EACCES || code == EPERM => {
            let parent = destination
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            format!(
                "Permission denied while writing to {parent}. Grant the tvsorter service user write access to the output mount, or adjust the bind-mount ownership/permissions on the Proxmox host."
            )
        }
        _ => err.to_string(),
    }
}

/// Source file stat data captured *before* the import runs. A `move` deletes
/// the source, so statting it afterwards (in `result_to_record`) would lose
/// this data; the caller captures it up front via `stat_source` instead.
#[derive(Clone, Copy, Default)]
pub struct SourceStat {
    pub size: Option<i64>,
    pub mtime: Option<f64>,
    pub device: Option<i64>,
    pub inode: Option<i64>,
}

/// Read size/mtime/device/inode from the source path. Call this before
/// executing the import so the data survives a `move`.
pub fn stat_source(path: &Path) -> SourceStat {
    match fs::metadata(path) {
        Ok(meta) => {
            let (device, inode) = stat_dev_inode(&meta);
            SourceStat {
                size: Some(meta.len() as i64),
                mtime: crate::filesystem::mtime_secs(&meta),
                device,
                inode,
            }
        }
        Err(_) => SourceStat::default(),
    }
}

/// Build the database record for an import result. `source` holds the source
/// stat captured before the import ran (see `stat_source`).
pub fn result_to_record(result: &ImportResult, source: SourceStat) -> ImportRecord {
    let request = &result.request;
    ImportRecord {
        source_path: request.source_path.to_string_lossy().to_string(),
        source_size: source.size,
        source_mtime: source.mtime,
        source_device: source.device,
        source_inode: source.inode,
        output_path: result.final_path.clone(),
        media_type: request.media_type.clone(),
        provider: request.provider.clone(),
        provider_show_id: request.provider_show_id.clone(),
        show_title: request.show_title.clone(),
        show_year: request.show_year,
        season_number: request.season_number,
        episode_number: request.episode_number,
        episode_title: request.episode_title.clone(),
        quality: request.quality.clone(),
        action: request.action.clone(),
        conflict_policy: request.conflict_policy.clone(),
        result: result.result.clone(),
        error: result.error.clone(),
    }
}

#[cfg(unix)]
fn stat_dev_inode(meta: &fs::Metadata) -> (Option<i64>, Option<i64>) {
    use std::os::unix::fs::MetadataExt;
    (Some(meta.dev() as i64), Some(meta.ino() as i64))
}

#[cfg(not(unix))]
fn stat_dev_inode(_meta: &fs::Metadata) -> (Option<i64>, Option<i64>) {
    (None, None)
}

/// Number of progress "units" an import contributes: byte count for copies,
/// 1 for instant actions.
pub fn import_request_units(request: &ImportRequest) -> u64 {
    if request.action == "copy" || request.action == "move" {
        fs::metadata(&request.source_path)
            .map(|m| m.len().max(1))
            .unwrap_or(1)
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tvsorter-imp-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn request(source: PathBuf, output_root: PathBuf, action: &str, policy: &str) -> ImportRequest {
        let source_name = source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        ImportRequest {
            source_path: source,
            output_root,
            media_type: "tv".to_string(),
            show_title: "Fringe".to_string(),
            show_year: Some(2008),
            season_number: 1,
            episode_number: 1,
            episode_title: "Pilot".to_string(),
            quality: "1080p".to_string(),
            action: action.to_string(),
            conflict_policy: policy.to_string(),
            provider: None,
            provider_show_id: None,
            origin: source_name,
        }
    }

    #[test]
    fn copy_creates_destination() {
        let dir = temp_dir();
        let source = dir.join("src.mkv");
        let mut f = File::create(&source).unwrap();
        f.write_all(b"hello world").unwrap();
        let out = dir.join("out");
        let result = execute_import(
            request(source, out.clone(), "copy", "skip"),
            None,
            None,
            None,
        );
        assert_eq!(result.result, "imported");
        assert!(PathBuf::from(&result.final_path).exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    #[cfg(unix)]
    fn copy_sets_origin_xattr() {
        let dir = temp_dir();
        let source = dir.join("src.mkv");
        File::create(&source)
            .unwrap()
            .write_all(b"hello world")
            .unwrap();
        let out = dir.join("out");
        let mut req = request(source, out, "copy", "skip");
        req.origin = "Spiderman/Saison 01/src.mkv".to_string();
        let result = execute_import(req, None, None, None);
        assert_eq!(result.result, "imported");
        let dest = PathBuf::from(&result.final_path);

        // The temp filesystem may not support user xattrs; tolerate that by
        // only asserting when a probe write succeeds, but assert the value when
        // xattrs are supported.
        let supported = xattr::set(&dest, ORIGIN_XATTR, b"probe").is_ok();
        if supported {
            // Re-run is unnecessary; the import already wrote it, but our probe
            // overwrote the value — restore via another import to validate the
            // real code path end to end.
            let value = xattr::get(&dest, ORIGIN_XATTR).unwrap();
            // Probe value present confirms xattr round-trips; now verify the
            // importer's own write by re-importing to a fresh destination.
            assert!(value.is_some());
            let dir2 = temp_dir();
            let source2 = dir2.join("track.mkv");
            File::create(&source2).unwrap().write_all(b"x").unwrap();
            let mut req2 = request(source2, dir2.join("out"), "copy", "skip");
            req2.origin = "Album/01.mkv".to_string();
            let r2 = execute_import(req2, None, None, None);
            let dest2 = PathBuf::from(&r2.final_path);
            let got = xattr::get(&dest2, ORIGIN_XATTR).unwrap();
            assert_eq!(got.as_deref(), Some(b"Album/01.mkv".as_ref()));
            fs::remove_dir_all(&dir2).ok();
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn skip_existing_destination() {
        let dir = temp_dir();
        let source = dir.join("src.mkv");
        File::create(&source).unwrap().write_all(b"x").unwrap();
        let out = dir.join("out");
        let first = execute_import(
            request(source.clone(), out.clone(), "copy", "skip"),
            None,
            None,
            None,
        );
        assert_eq!(first.result, "imported");
        let second = execute_import(request(source, out, "copy", "skip"), None, None, None);
        assert_eq!(second.result, "skipped");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn index_keeps_both() {
        let dir = temp_dir();
        let source = dir.join("src.mkv");
        File::create(&source).unwrap().write_all(b"x").unwrap();
        let out = dir.join("out");
        execute_import(
            request(source.clone(), out.clone(), "copy", "skip"),
            None,
            None,
            None,
        );
        let indexed = execute_import(request(source, out, "copy", "index"), None, None, None);
        assert_eq!(indexed.result, "imported");
        assert!(indexed.final_path.contains("(2)"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_action_is_preview() {
        let dir = temp_dir();
        let source = dir.join("src.mkv");
        File::create(&source).unwrap().write_all(b"x").unwrap();
        let result = execute_import(
            request(source, dir.join("out"), "test", "skip"),
            None,
            None,
            None,
        );
        assert_eq!(result.result, "preview");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn move_removes_source() {
        let dir = temp_dir();
        let source = dir.join("src.mkv");
        File::create(&source)
            .unwrap()
            .write_all(b"hello world")
            .unwrap();
        let out = dir.join("out");
        let result = execute_import(
            request(source.clone(), out, "move", "skip"),
            None,
            None,
            None,
        );
        assert_eq!(result.result, "imported");
        assert!(PathBuf::from(&result.final_path).exists());
        assert!(!source.exists(), "source should be gone after a move");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn move_record_keeps_source_stat_captured_before_run() {
        let dir = temp_dir();
        let source = dir.join("src.mkv");
        File::create(&source)
            .unwrap()
            .write_all(b"hello world")
            .unwrap();
        // Stat must be captured before the move deletes the source; statting
        // after would yield all-None.
        let stat = stat_source(&source);
        let result = execute_import(
            request(source.clone(), dir.join("out"), "move", "skip"),
            None,
            None,
            None,
        );
        assert!(!source.exists());
        let record = result_to_record(&result, stat);
        assert_eq!(record.source_size, Some(11));
        assert!(record.source_mtime.is_some());
        fs::remove_dir_all(&dir).ok();
    }
}
