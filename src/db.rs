use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::Serialize;

use crate::filesystem::canonical_or_normalized;

const SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS input_roots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS imports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_path TEXT NOT NULL,
    source_size INTEGER,
    source_mtime REAL,
    source_device INTEGER,
    source_inode INTEGER,
    output_path TEXT NOT NULL,
    media_type TEXT NOT NULL CHECK (media_type IN ('tv', 'anime', 'film')),
    provider TEXT,
    provider_show_id TEXT,
    show_title TEXT NOT NULL,
    show_year INTEGER,
    season_number INTEGER NOT NULL,
    episode_number INTEGER NOT NULL,
    episode_title TEXT NOT NULL,
    quality TEXT NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('hardlink', 'copy', 'move', 'test')),
    conflict_policy TEXT NOT NULL CHECK (conflict_policy IN ('skip', 'replace', 'index', 'fail')),
    result TEXT NOT NULL,
    error TEXT,
    imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS library_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    media_type TEXT NOT NULL CHECK (media_type IN ('tv', 'anime', 'film')),
    output_path TEXT NOT NULL UNIQUE,
    size INTEGER,
    mtime REAL,
    present INTEGER NOT NULL DEFAULT 1,
    import_id INTEGER REFERENCES imports(id) ON DELETE SET NULL,
    discovered_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS provider_cache (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS source_status_overrides (
    source_path TEXT PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('none', 'imported', 'failed', 'skipped', 'preview', 'conflict')),
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
"#;

#[derive(Clone, Debug, Serialize)]
pub struct InputRoot {
    pub id: i64,
    pub path: String,
}

/// Record written when an import is attempted. Mirrors the `imports` table.
#[derive(Clone, Debug)]
pub struct ImportRecord {
    pub source_path: String,
    pub source_size: Option<i64>,
    pub source_mtime: Option<f64>,
    pub source_device: Option<i64>,
    pub source_inode: Option<i64>,
    pub output_path: String,
    pub media_type: String,
    pub provider: Option<String>,
    pub provider_show_id: Option<String>,
    pub show_title: String,
    pub show_year: Option<i64>,
    pub season_number: i64,
    pub episode_number: i64,
    pub episode_title: String,
    pub quality: String,
    pub action: String,
    pub conflict_policy: String,
    pub result: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ImportRow {
    pub id: i64,
    pub source_path: String,
    pub output_path: String,
    pub media_type: String,
    pub provider: Option<String>,
    pub provider_show_id: Option<String>,
    pub show_title: String,
    pub show_year: Option<i64>,
    pub season_number: i64,
    pub episode_number: i64,
    pub episode_title: String,
    pub quality: String,
    pub action: String,
    pub conflict_policy: String,
    pub result: String,
    pub error: Option<String>,
    pub imported_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LibraryFileRow {
    pub id: i64,
    pub media_type: String,
    pub output_path: String,
    pub size: Option<i64>,
    pub present: bool,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceStatusOverride {
    pub source_path: String,
    pub status: String,
}

#[derive(Clone)]
pub struct Database {
    #[allow(dead_code)]
    path: PathBuf,
    conn: Arc<Mutex<Connection>>,
}

type DbResult<T> = Result<T, rusqlite::Error>;

impl Database {
    pub fn open(path: &Path) -> DbResult<Self> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        migrate_imports_action_check(&conn)?;
        Ok(Self {
            path: path.to_path_buf(),
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    #[allow(dead_code)]
    pub fn database_path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- settings ----

    pub fn get_setting(&self, key: &str, default: &str) -> String {
        let conn = self.lock();
        conn.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .ok()
        .flatten()
        .unwrap_or_else(|| default.to_string())
    }

    pub fn set_setting(&self, key: &str, value: &str) {
        let conn = self.lock();
        let _ = conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        );
    }

    // ---- input roots ----

    pub fn replace_input_roots(&self, paths: &[String]) {
        let mut unique: Vec<String> = Vec::new();
        for path in paths {
            if !path.is_empty() && !unique.contains(path) {
                unique.push(path.clone());
            }
        }
        let mut conn = self.lock();
        let tx = match conn.transaction() {
            Ok(tx) => tx,
            Err(_) => return,
        };
        let _ = tx.execute("DELETE FROM input_roots", []);
        for path in &unique {
            let _ = tx.execute("INSERT INTO input_roots (path) VALUES (?1)", params![path]);
        }
        let _ = tx.commit();
    }

    pub fn list_input_roots(&self) -> Vec<InputRoot> {
        let conn = self.lock();
        let mut stmt = match conn.prepare("SELECT id, path FROM input_roots ORDER BY path") {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt
            .query_map([], |row| {
                Ok(InputRoot {
                    id: row.get(0)?,
                    path: row.get(1)?,
                })
            })
            .map(|iter| iter.filter_map(Result::ok).collect())
            .unwrap_or_default();
        rows
    }

    pub fn get_input_root(&self, root_id: i64) -> Option<InputRoot> {
        let conn = self.lock();
        conn.query_row(
            "SELECT id, path FROM input_roots WHERE id = ?1",
            params![root_id],
            |row| {
                Ok(InputRoot {
                    id: row.get(0)?,
                    path: row.get(1)?,
                })
            },
        )
        .optional()
        .ok()
        .flatten()
    }

    // ---- imports ----

    pub fn insert_import(&self, record: &ImportRecord) -> i64 {
        let conn = self.lock();
        let result = conn.execute(
            "INSERT INTO imports (
                source_path, source_size, source_mtime, source_device, source_inode,
                output_path, media_type, provider, provider_show_id, show_title, show_year,
                season_number, episode_number, episode_title, quality, action, conflict_policy,
                result, error
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
            )",
            params![
                record.source_path,
                record.source_size,
                record.source_mtime,
                record.source_device,
                record.source_inode,
                record.output_path,
                record.media_type,
                record.provider,
                record.provider_show_id,
                record.show_title,
                record.show_year,
                record.season_number,
                record.episode_number,
                record.episode_title,
                record.quality,
                record.action,
                record.conflict_policy,
                record.result,
                record.error,
            ],
        );
        if result.is_err() {
            return -1;
        }
        let import_id = conn.last_insert_rowid();
        if matches!(record.result.as_str(), "imported" | "preview" | "skipped") {
            upsert_library_file(&conn, record, Some(import_id));
        }
        import_id
    }

    pub fn list_imports(&self, limit: i64) -> Vec<ImportRow> {
        let conn = self.lock();
        let mut stmt = match conn
            .prepare("SELECT * FROM imports ORDER BY imported_at DESC, id DESC LIMIT ?1")
        {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(params![limit], map_import_row)
            .map(|iter| iter.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    pub fn latest_imports_for_sources(&self, source_paths: &[PathBuf]) -> HashMap<String, ImportRow> {
        if source_paths.is_empty() {
            return HashMap::new();
        }
        let normalized: Vec<String> = source_paths
            .iter()
            .map(|p| canonical_or_normalized(p).to_string_lossy().to_string())
            .collect();
        let placeholders = vec!["?"; normalized.len()].join(", ");
        let sql = format!(
            "SELECT * FROM imports WHERE id IN (
                SELECT MAX(id) FROM imports WHERE source_path IN ({placeholders}) GROUP BY source_path
            )"
        );
        let conn = self.lock();
        let mut stmt = match conn.prepare(&sql) {
            Ok(stmt) => stmt,
            Err(_) => return HashMap::new(),
        };
        let rows = stmt
            .query_map(params_from_iter(normalized.iter()), map_import_row)
            .map(|iter| iter.filter_map(Result::ok).collect::<Vec<_>>())
            .unwrap_or_default();
        rows.into_iter().map(|row| (row.source_path.clone(), row)).collect()
    }

    // ---- source status overrides ----

    pub fn source_status_overrides(
        &self,
        source_paths: &[PathBuf],
    ) -> HashMap<String, SourceStatusOverride> {
        if source_paths.is_empty() {
            return HashMap::new();
        }
        let normalized: Vec<String> = source_paths
            .iter()
            .map(|p| canonical_or_normalized(p).to_string_lossy().to_string())
            .collect();
        let placeholders = vec!["?"; normalized.len()].join(", ");
        let sql = format!(
            "SELECT source_path, status FROM source_status_overrides WHERE source_path IN ({placeholders})"
        );
        let conn = self.lock();
        let mut stmt = match conn.prepare(&sql) {
            Ok(stmt) => stmt,
            Err(_) => return HashMap::new(),
        };
        let rows = stmt
            .query_map(params_from_iter(normalized.iter()), |row| {
                Ok(SourceStatusOverride {
                    source_path: row.get(0)?,
                    status: row.get(1)?,
                })
            })
            .map(|iter| iter.filter_map(Result::ok).collect::<Vec<_>>())
            .unwrap_or_default();
        rows.into_iter().map(|row| (row.source_path.clone(), row)).collect()
    }

    pub fn set_source_status_overrides(&self, source_paths: &[PathBuf], status: Option<&str>) {
        let normalized: Vec<String> = source_paths
            .iter()
            .map(|p| canonical_or_normalized(p).to_string_lossy().to_string())
            .collect();
        if normalized.is_empty() {
            return;
        }
        let conn = self.lock();
        match status {
            None => {
                for path in &normalized {
                    let _ = conn.execute(
                        "DELETE FROM source_status_overrides WHERE source_path = ?1",
                        params![path],
                    );
                }
            }
            Some(status) => {
                for path in &normalized {
                    let _ = conn.execute(
                        "INSERT INTO source_status_overrides (source_path, status) VALUES (?1, ?2)
                         ON CONFLICT(source_path) DO UPDATE SET status = excluded.status, updated_at = CURRENT_TIMESTAMP",
                        params![path, status],
                    );
                }
            }
        }
    }

    // ---- library files ----

    pub fn list_library_files(&self) -> Vec<LibraryFileRow> {
        let conn = self.lock();
        let mut stmt = match conn.prepare(
            "SELECT id, media_type, output_path, size, present, updated_at FROM library_files ORDER BY media_type, output_path",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |row| {
            Ok(LibraryFileRow {
                id: row.get(0)?,
                media_type: row.get(1)?,
                output_path: row.get(2)?,
                size: row.get(3)?,
                present: row.get::<_, i64>(4)? != 0,
                updated_at: row.get(5)?,
            })
        })
        .map(|iter| iter.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    pub fn upsert_discovered_file(&self, media_type: &str, path: &Path) {
        let (size, mtime) = match std::fs::metadata(path) {
            Ok(meta) => (
                meta.len() as i64,
                meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs_f64()),
            ),
            Err(_) => return,
        };
        let conn = self.lock();
        let _ = conn.execute(
            "INSERT INTO library_files (media_type, output_path, size, mtime, present)
             VALUES (?1, ?2, ?3, ?4, 1)
             ON CONFLICT(output_path) DO UPDATE SET
                size = excluded.size, mtime = excluded.mtime, present = 1, updated_at = CURRENT_TIMESTAMP",
            params![media_type, path.to_string_lossy(), size, mtime],
        );
    }

    pub fn mark_missing_outside(&self, roots: &HashMap<String, PathBuf>) {
        let rows = self.list_library_files();
        let conn = self.lock();
        for row in rows {
            let path = PathBuf::from(&row.output_path);
            if let Some(root) = roots.get(&row.media_type) {
                if crate::filesystem::is_relative_to(&path, root) && !path.exists() {
                    let _ = conn.execute(
                        "UPDATE library_files SET present = 0, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
                        params![row.id],
                    );
                }
            }
        }
    }

    // ---- provider cache ----

    pub fn get_cache(&self, key: &str) -> Option<serde_json::Value> {
        let conn = self.lock();
        let text: Option<String> = conn
            .query_row(
                "SELECT value FROM provider_cache WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();
        text.and_then(|t| serde_json::from_str(&t).ok())
    }

    pub fn set_cache(&self, key: &str, value: &serde_json::Value) {
        let serialized = match serde_json::to_string(value) {
            Ok(s) => s,
            Err(_) => return,
        };
        let conn = self.lock();
        let _ = conn.execute(
            "INSERT INTO provider_cache (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, created_at = CURRENT_TIMESTAMP",
            params![key, serialized],
        );
    }
}

/// Older databases created the `imports` table with a CHECK constraint that
/// only allowed ('hardlink', 'copy', 'test'). Rebuild the table so the 'move'
/// action is accepted. No-op on fresh databases or ones already migrated.
fn migrate_imports_action_check(conn: &Connection) -> DbResult<()> {
    let sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'imports'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let needs_migration = match sql {
        Some(definition) => !definition.contains("'move'"),
        None => false,
    };
    if !needs_migration {
        return Ok(());
    }

    // SQLite can't ALTER a CHECK constraint, so recreate the table and copy rows.
    // library_files.import_id references imports(id); ids are preserved by the
    // copy, so disable FK enforcement during the swap to avoid spurious errors.
    conn.execute_batch(
        r#"
PRAGMA foreign_keys = OFF;
BEGIN;
ALTER TABLE imports RENAME TO imports_legacy;
CREATE TABLE imports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_path TEXT NOT NULL,
    source_size INTEGER,
    source_mtime REAL,
    source_device INTEGER,
    source_inode INTEGER,
    output_path TEXT NOT NULL,
    media_type TEXT NOT NULL CHECK (media_type IN ('tv', 'anime', 'film')),
    provider TEXT,
    provider_show_id TEXT,
    show_title TEXT NOT NULL,
    show_year INTEGER,
    season_number INTEGER NOT NULL,
    episode_number INTEGER NOT NULL,
    episode_title TEXT NOT NULL,
    quality TEXT NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('hardlink', 'copy', 'move', 'test')),
    conflict_policy TEXT NOT NULL CHECK (conflict_policy IN ('skip', 'replace', 'index', 'fail')),
    result TEXT NOT NULL,
    error TEXT,
    imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
INSERT INTO imports SELECT * FROM imports_legacy;
DROP TABLE imports_legacy;
COMMIT;
PRAGMA foreign_keys = ON;
"#,
    )?;
    Ok(())
}

fn map_import_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ImportRow> {
    Ok(ImportRow {
        id: row.get("id")?,
        source_path: row.get("source_path")?,
        output_path: row.get("output_path")?,
        media_type: row.get("media_type")?,
        provider: row.get("provider")?,
        provider_show_id: row.get("provider_show_id")?,
        show_title: row.get("show_title")?,
        show_year: row.get("show_year")?,
        season_number: row.get("season_number")?,
        episode_number: row.get("episode_number")?,
        episode_title: row.get("episode_title")?,
        quality: row.get("quality")?,
        action: row.get("action")?,
        conflict_policy: row.get("conflict_policy")?,
        result: row.get("result")?,
        error: row.get("error")?,
        imported_at: row.get("imported_at")?,
    })
}

fn upsert_library_file(conn: &Connection, record: &ImportRecord, import_id: Option<i64>) {
    let output_path = PathBuf::from(&record.output_path);
    let (size, mtime, present) = match std::fs::metadata(&output_path) {
        Ok(meta) => (
            Some(meta.len() as i64),
            meta.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64()),
            1,
        ),
        Err(_) => (None, None, 0),
    };
    let _ = conn.execute(
        "INSERT INTO library_files (media_type, output_path, size, mtime, present, import_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(output_path) DO UPDATE SET
            media_type = excluded.media_type,
            size = excluded.size,
            mtime = excluded.mtime,
            present = excluded.present,
            import_id = COALESCE(excluded.import_id, library_files.import_id),
            updated_at = CURRENT_TIMESTAMP",
        params![
            record.media_type,
            record.output_path,
            size,
            mtime,
            present,
            import_id
        ],
    );
}
