use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::model::{Category, DownloadRecord, DownloadStatus};

#[derive(Debug)]
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.execute_batch(
            r"
            CREATE TABLE IF NOT EXISTS downloads (
                id TEXT PRIMARY KEY,
                url TEXT NOT NULL,
                mirrors TEXT NOT NULL DEFAULT '[]',
                final_url TEXT,
                destination TEXT NOT NULL,
                filename TEXT NOT NULL,
                category TEXT NOT NULL DEFAULT 'General',
                status TEXT NOT NULL,
                total_size INTEGER,
                downloaded INTEGER NOT NULL DEFAULT 0,
                speed_bps REAL NOT NULL DEFAULT 0,
                connections INTEGER NOT NULL DEFAULT 8,
                speed_limit INTEGER,
                headers TEXT NOT NULL DEFAULT '[]',
                checksum_algorithm TEXT,
                checksum_value TEXT,
                etag TEXT,
                last_modified TEXT,
                error TEXT,
                priority INTEGER NOT NULL DEFAULT 0,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                completed_at_ms INTEGER
            );
            CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status);
            CREATE INDEX IF NOT EXISTS idx_downloads_created ON downloads(created_at_ms);
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS categories (
                name TEXT PRIMARY KEY,
                directory TEXT NOT NULL,
                extensions TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS queues (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                concurrency INTEGER NOT NULL DEFAULT 4,
                enabled INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE IF NOT EXISTS schedules (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                days INTEGER NOT NULL DEFAULT 127,
                start_min INTEGER,
                stop_min INTEGER,
                action TEXT NOT NULL DEFAULT 'none'
            );
            INSERT OR IGNORE INTO categories(name,directory,extensions) VALUES
                ('General','',''),
                ('Music','','mp3 flac wav m4a ogg opus aac'),
                ('Video','','mp4 mkv webm avi mov m4v ts m3u8 mpd m4s'),
                ('Programs','','exe msi msix dmg pkg deb rpm appimage apk'),
                ('Documents','','pdf doc docx xls xlsx ppt pptx txt md epub'),
                ('Archives','','zip 7z rar tar gz bz2 xz zst');
            ",
        )?;
        let has_mirrors = {
            let mut statement = conn.prepare("PRAGMA table_info(downloads)")?;
            let mut rows = statement.query([])?;
            let mut found = false;
            while let Some(row) = rows.next()? {
                let name: String = row.get(1)?;
                if name == "mirrors" {
                    found = true;
                    break;
                }
            }
            found
        };
        if !has_mirrors {
            conn.execute(
                "ALTER TABLE downloads ADD COLUMN mirrors TEXT NOT NULL DEFAULT '[]'",
                [],
            )?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert(&self, record: &DownloadRecord) -> rusqlite::Result<()> {
        let headers = serde_json::to_string(&record.headers).unwrap_or_else(|_| "[]".into());
        let mirrors = serde_json::to_string(&record.mirrors).unwrap_or_else(|_| "[]".into());
        self.conn.lock().execute(
            r"INSERT INTO downloads (
                id,url,mirrors,final_url,destination,filename,category,status,total_size,downloaded,
                speed_bps,connections,speed_limit,headers,checksum_algorithm,checksum_value,
                etag,last_modified,error,priority,created_at_ms,updated_at_ms,completed_at_ms
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
            params![
                record.id.to_string(),
                record.url,
                mirrors,
                record.final_url,
                record.destination.to_string_lossy(),
                record.filename,
                record.category,
                record.status.as_str(),
                record.total_size.and_then(|value| i64::try_from(value).ok()),
                i64::try_from(record.downloaded).unwrap_or(i64::MAX),
                record.speed_bps,
                i64::from(record.connections),
                record.speed_limit.and_then(|v| i64::try_from(v).ok()),
                headers,
                record.checksum_algorithm,
                record.checksum_value,
                record.etag,
                record.last_modified,
                record.error,
                i64::from(record.priority),
                record.created_at_ms,
                record.updated_at_ms,
                record.completed_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn update(&self, record: &DownloadRecord) -> rusqlite::Result<()> {
        let headers = serde_json::to_string(&record.headers).unwrap_or_else(|_| "[]".into());
        let mirrors = serde_json::to_string(&record.mirrors).unwrap_or_else(|_| "[]".into());
        self.conn.lock().execute(
            r"UPDATE downloads SET
                url=?2, mirrors=?3, final_url=?4, destination=?5, filename=?6, category=?7, status=?8,
                total_size=?9, downloaded=?10, speed_bps=?11, connections=?12, speed_limit=?13,
                headers=?14, checksum_algorithm=?15, checksum_value=?16, etag=?17,
                last_modified=?18, error=?19, priority=?20, updated_at_ms=?21, completed_at_ms=?22
              WHERE id=?1",
            params![
                record.id.to_string(),
                record.url,
                mirrors,
                record.final_url,
                record.destination.to_string_lossy(),
                record.filename,
                record.category,
                record.status.as_str(),
                record.total_size.and_then(|v| i64::try_from(v).ok()),
                i64::try_from(record.downloaded).unwrap_or(i64::MAX),
                record.speed_bps,
                i64::from(record.connections),
                record.speed_limit.and_then(|v| i64::try_from(v).ok()),
                headers,
                record.checksum_algorithm,
                record.checksum_value,
                record.etag,
                record.last_modified,
                record.error,
                i64::from(record.priority),
                record.updated_at_ms,
                record.completed_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, id: Uuid) -> rusqlite::Result<Option<DownloadRecord>> {
        self.conn
            .lock()
            .query_row(
                "SELECT * FROM downloads WHERE id=?1",
                params![id.to_string()],
                map_row,
            )
            .optional()
    }

    pub fn list(&self, status: Option<DownloadStatus>) -> rusqlite::Result<Vec<DownloadRecord>> {
        let conn = self.conn.lock();
        let mut out = Vec::new();
        if let Some(status) = status {
            let mut statement = conn.prepare(
                "SELECT * FROM downloads WHERE status=?1 ORDER BY priority DESC, created_at_ms ASC",
            )?;
            let rows = statement.query_map(params![status.as_str()], map_row)?;
            for row in rows {
                out.push(row?);
            }
        } else {
            let mut statement =
                conn.prepare("SELECT * FROM downloads ORDER BY priority DESC, created_at_ms ASC")?;
            let rows = statement.query_map([], map_row)?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }

    pub fn next_queued(&self) -> rusqlite::Result<Option<DownloadRecord>> {
        self.conn
            .lock()
            .query_row(
                "SELECT * FROM downloads WHERE status='queued' ORDER BY priority DESC, created_at_ms ASC LIMIT 1",
                [],
                map_row,
            )
            .optional()
    }

    pub fn delete(&self, id: Uuid) -> rusqlite::Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM downloads WHERE id=?1", params![id.to_string()])?;
        Ok(())
    }

    /// Called on daemon startup: no worker survives a process restart, so
    /// active rows become paused with an explicit reason.
    pub fn mark_interrupted(&self) -> rusqlite::Result<usize> {
        let conn = self.conn.lock();
        let changed = conn.execute(
            "UPDATE downloads SET status='paused', error='interrupted by application exit', updated_at_ms=strftime('%s','now')*1000 WHERE status IN ('queued','probing','downloading','finalizing')",
            [],
        )?;
        Ok(changed)
    }

    pub fn count_active(&self) -> rusqlite::Result<usize> {
        let count: i64 = self.conn.lock().query_row(
            "SELECT COUNT(*) FROM downloads WHERE status IN ('probing','downloading','finalizing')",
            [],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    pub fn list_categories(&self) -> rusqlite::Result<Vec<Category>> {
        let conn = self.conn.lock();
        let mut statement =
            conn.prepare("SELECT name,directory,extensions FROM categories ORDER BY name")?;
        let rows = statement.query_map([], |row| {
            Ok(Category {
                name: row.get("name")?,
                directory: row.get::<_, String>("directory")?.into(),
                extensions: row
                    .get::<_, String>("extensions")?
                    .split_whitespace()
                    .map(str::to_ascii_lowercase)
                    .collect(),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn category_for(&self, extension: &str) -> rusqlite::Result<Option<Category>> {
        let extension = extension.trim_start_matches('.').to_ascii_lowercase();
        Ok(self
            .list_categories()?
            .into_iter()
            .find(|category| category.extensions.iter().any(|item| item == &extension)))
    }

    pub fn set_category_directory(&self, name: &str, directory: &str) -> rusqlite::Result<()> {
        self.conn.lock().execute(
            "UPDATE categories SET directory=?2 WHERE name=?1",
            params![name, directory],
        )?;
        Ok(())
    }

    pub fn list_schedules(&self) -> rusqlite::Result<Vec<crate::scheduler::Schedule>> {
        let conn = self.conn.lock();
        let mut statement = conn.prepare("SELECT * FROM schedules ORDER BY name")?;
        let rows = statement.query_map([], |row| {
            let id: String = row.get("id")?;
            let start: Option<i64> = row.get("start_min")?;
            let stop: Option<i64> = row.get("stop_min")?;
            Ok(crate::scheduler::Schedule {
                id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
                name: row.get("name")?,
                enabled: row.get::<_, i64>("enabled")? != 0,
                days: u8::try_from(row.get::<_, i64>("days")?).unwrap_or(127),
                start_min: start.and_then(|value| u16::try_from(value).ok()),
                stop_min: stop.and_then(|value| u16::try_from(value).ok()),
                action: crate::scheduler::PowerAction::parse(&row.get::<_, String>("action")?),
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn insert_schedule(&self, schedule: &crate::scheduler::Schedule) -> rusqlite::Result<()> {
        self.conn.lock().execute(
            "INSERT INTO schedules(id,name,enabled,days,start_min,stop_min,action) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                schedule.id.to_string(),
                schedule.name,
                i64::from(schedule.enabled),
                i64::from(schedule.days),
                schedule.start_min.map(i64::from),
                schedule.stop_min.map(i64::from),
                schedule.action.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn delete_schedule(&self, id: Uuid) -> rusqlite::Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM schedules WHERE id=?1", params![id.to_string()])?;
        Ok(())
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.lock().execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .lock()
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
    }
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DownloadRecord> {
    let id: String = row.get("id")?;
    let destination: String = row.get("destination")?;
    let headers_json: String = row.get("headers")?;
    let mirrors_json: String = row.get("mirrors")?;
    let total: Option<i64> = row.get("total_size")?;
    let downloaded: i64 = row.get("downloaded")?;
    let connections: i64 = row.get("connections")?;
    let speed_limit: Option<i64> = row.get("speed_limit")?;
    let priority: i64 = row.get("priority")?;
    let status: String = row.get("status")?;
    Ok(DownloadRecord {
        id: Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()),
        url: row.get("url")?,
        mirrors: serde_json::from_str(&mirrors_json).unwrap_or_default(),
        final_url: row.get("final_url")?,
        destination: PathBuf::from(destination),
        filename: row.get("filename")?,
        category: row.get("category")?,
        status: DownloadStatus::parse(&status),
        total_size: total.and_then(|v| u64::try_from(v).ok()),
        downloaded: u64::try_from(downloaded).unwrap_or(0),
        speed_bps: row.get("speed_bps")?,
        connections: u8::try_from(connections).unwrap_or(8),
        speed_limit: speed_limit.and_then(|v| u64::try_from(v).ok()),
        headers: serde_json::from_str(&headers_json).unwrap_or_default(),
        checksum_algorithm: row.get("checksum_algorithm")?,
        checksum_value: row.get("checksum_value")?,
        etag: row.get("etag")?,
        last_modified: row.get("last_modified")?,
        error: row.get("error")?,
        priority: i32::try_from(priority).unwrap_or(0),
        created_at_ms: row.get("created_at_ms")?,
        updated_at_ms: row.get("updated_at_ms")?,
        completed_at_ms: row.get("completed_at_ms")?,
    })
}

pub type SharedDatabase = Arc<Database>;

#[cfg(test)]
mod tests {
    use super::*;

    fn record(url: &str) -> DownloadRecord {
        let now = chrono::Utc::now().timestamp_millis();
        DownloadRecord {
            id: Uuid::new_v4(),
            url: url.into(),
            mirrors: vec!["https://mirror.example/file.bin".into()],
            final_url: None,
            destination: PathBuf::from("C:/downloads/file.bin"),
            filename: "file.bin".into(),
            category: "General".into(),
            status: DownloadStatus::Queued,
            total_size: Some(1234),
            downloaded: 0,
            speed_bps: 0.0,
            connections: 8,
            speed_limit: None,
            headers: vec![("Referer".into(), "https://example.test".into())],
            checksum_algorithm: None,
            checksum_value: None,
            etag: Some("\"x\"".into()),
            last_modified: None,
            error: None,
            priority: 0,
            created_at_ms: now,
            updated_at_ms: now,
            completed_at_ms: None,
        }
    }

    #[test]
    fn insert_get_and_update_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("state.db")).unwrap();
        let mut item = record("https://example.test/file.bin");
        db.insert(&item).unwrap();
        assert_eq!(db.get(item.id).unwrap().unwrap().url, item.url);
        item.downloaded = 100;
        item.status = DownloadStatus::Downloading;
        db.update(&item).unwrap();
        let loaded = db.get(item.id).unwrap().unwrap();
        assert_eq!(loaded.downloaded, 100);
        assert_eq!(loaded.status, DownloadStatus::Downloading);
        assert_eq!(loaded.headers.len(), 1);
        assert_eq!(loaded.mirrors.len(), 1);
    }

    #[test]
    fn queue_order_and_interruption() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("state.db")).unwrap();
        let mut low = record("https://example.test/low");
        low.priority = 1;
        let mut high = record("https://example.test/high");
        high.priority = 10;
        db.insert(&low).unwrap();
        db.insert(&high).unwrap();
        assert_eq!(db.next_queued().unwrap().unwrap().id, high.id);
        assert_eq!(db.mark_interrupted().unwrap(), 2);
        assert!(db.next_queued().unwrap().is_none());
    }

    #[test]
    fn migrates_legacy_database_without_mirrors_column() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                r"CREATE TABLE downloads (
                    id TEXT PRIMARY KEY, url TEXT NOT NULL, final_url TEXT,
                    destination TEXT NOT NULL, filename TEXT NOT NULL,
                    category TEXT NOT NULL DEFAULT 'General', status TEXT NOT NULL,
                    total_size INTEGER, downloaded INTEGER NOT NULL DEFAULT 0,
                    speed_bps REAL NOT NULL DEFAULT 0, connections INTEGER NOT NULL DEFAULT 8,
                    speed_limit INTEGER, headers TEXT NOT NULL DEFAULT '[]',
                    checksum_algorithm TEXT, checksum_value TEXT, etag TEXT,
                    last_modified TEXT, error TEXT, priority INTEGER NOT NULL DEFAULT 0,
                    created_at_ms INTEGER NOT NULL, updated_at_ms INTEGER NOT NULL,
                    completed_at_ms INTEGER
                );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO downloads(id,url,destination,filename,status,created_at_ms,updated_at_ms) VALUES('00000000-0000-0000-0000-000000000001','https://x.test/a','a','a','paused',0,0)",
                [],
            )
            .unwrap();
        }
        let db = Database::open(&path).unwrap();
        let record = db
            .get(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap())
            .unwrap()
            .unwrap();
        assert!(record.mirrors.is_empty());
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let id = {
            let db = Database::open(&path).unwrap();
            let item = record("https://example.test/persist");
            db.insert(&item).unwrap();
            item.id
        };
        let db = Database::open(&path).unwrap();
        assert!(db.get(id).unwrap().is_some());
    }
}
