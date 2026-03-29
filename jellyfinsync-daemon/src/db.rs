use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMapping {
    pub id: String,
    pub name: Option<String>,
    pub jellyfin_user_id: Option<String>,
    pub sync_rules: Option<String>,
    pub last_seen_at: Option<String>,
    pub auto_sync_on_connect: bool,
    pub transcoding_profile_id: Option<String>,
}

pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn new(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path).map_err(|e| anyhow!("Failed to open database: {}", e))?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| anyhow!("Failed to open in-memory database: {}", e))?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS devices (
                id TEXT PRIMARY KEY,
                name TEXT,
                jellyfin_user_id TEXT,
                sync_rules TEXT,
                last_seen_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                auto_sync_on_connect BOOLEAN DEFAULT 0
            )",
            [],
        )
        .map_err(|e| anyhow!("Failed to create devices table: {}", e))?;

        // Migration: add auto_sync_on_connect column if missing (existing databases)
        let has_column: bool = conn
            .prepare("SELECT auto_sync_on_connect FROM devices LIMIT 0")
            .is_ok();
        if !has_column {
            conn.execute(
                "ALTER TABLE devices ADD COLUMN auto_sync_on_connect BOOLEAN DEFAULT 0",
                [],
            )
            .map_err(|e| anyhow!("Failed to add auto_sync_on_connect column: {}", e))?;
        }

        // Migration: add transcoding_profile_id column if missing
        let has_transcoding_col: bool = conn
            .prepare("SELECT transcoding_profile_id FROM devices LIMIT 0")
            .is_ok();
        if !has_transcoding_col {
            conn.execute(
                "ALTER TABLE devices ADD COLUMN transcoding_profile_id TEXT",
                [],
            )
            .map_err(|e| anyhow!("Failed to add transcoding_profile_id column: {}", e))?;
        }
        conn.execute(
            "CREATE TABLE IF NOT EXISTS scrobble_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id TEXT NOT NULL,
                artist TEXT NOT NULL,
                album TEXT NOT NULL,
                title TEXT NOT NULL,
                timestamp_unix INTEGER NOT NULL,
                submitted_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )
        .map_err(|e| anyhow!("Failed to create scrobble_history table: {}", e))?;
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_scrobble_unique
             ON scrobble_history(device_id, artist, album, title, timestamp_unix)",
            [],
        )
        .map_err(|e| anyhow!("Failed to create scrobble unique index: {}", e))?;
        Ok(())
    }

    pub fn get_device_mapping(&self, id: &str) -> Result<Option<DeviceMapping>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, jellyfin_user_id, sync_rules, last_seen_at, auto_sync_on_connect, transcoding_profile_id FROM devices WHERE id = ?",
        )?;

        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(DeviceMapping {
                id: row.get(0)?,
                name: row.get(1)?,
                jellyfin_user_id: row.get(2)?,
                sync_rules: row.get(3)?,
                last_seen_at: row.get(4)?,
                auto_sync_on_connect: row.get::<_, bool>(5).unwrap_or(false),
                transcoding_profile_id: row.get(6)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn record_scrobble(
        &self,
        device_id: &str,
        artist: &str,
        album: &str,
        title: &str,
        timestamp_unix: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO scrobble_history (device_id, artist, album, title, timestamp_unix)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![device_id, artist, album, title, timestamp_unix],
        )
        .map_err(|e| anyhow!("Failed to record scrobble: {}", e))?;
        Ok(())
    }

    pub fn is_scrobble_recorded(
        &self,
        device_id: &str,
        artist: &str,
        album: &str,
        title: &str,
        timestamp_unix: i64,
    ) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM scrobble_history
                 WHERE device_id=?1 AND artist=?2 AND album=?3 AND title=?4 AND timestamp_unix=?5",
                params![device_id, artist, album, title, timestamp_unix],
                |row| row.get(0),
            )
            .map_err(|e| anyhow!("Failed to check scrobble record: {}", e))?;
        Ok(count > 0)
    }

    #[cfg(test)]
    pub fn drop_scrobble_table_for_test(&self) {
        let conn = self.conn.lock().unwrap();
        conn.execute("DROP TABLE scrobble_history", []).unwrap();
    }

    pub fn get_scrobble_count(&self, device_id: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM scrobble_history WHERE device_id = ?1",
                params![device_id],
                |row| row.get(0),
            )
            .map_err(|e| anyhow!("Failed to get scrobble count: {}", e))?;
        Ok(count)
    }

    pub fn upsert_device_mapping(
        &self,
        id: &str,
        name: Option<&str>,
        user_id: Option<&str>,
        rules: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO devices (id, name, jellyfin_user_id, sync_rules, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                jellyfin_user_id = excluded.jellyfin_user_id,
                sync_rules = excluded.sync_rules,
                last_seen_at = CURRENT_TIMESTAMP",
            params![id, name, user_id, rules],
        )
        .map_err(|e| anyhow!("Failed to upsert device mapping: {}", e))?;
        Ok(())
    }

    pub fn set_auto_sync_on_connect(&self, id: &str, enabled: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .execute(
                "UPDATE devices SET auto_sync_on_connect = ?1 WHERE id = ?2",
                params![enabled, id],
            )
            .map_err(|e| anyhow!("Failed to update auto_sync_on_connect: {}", e))?;
        if rows == 0 {
            return Err(anyhow!("Device not found: {}", id));
        }
        Ok(())
    }

    pub fn set_transcoding_profile(&self, device_id: &str, profile_id: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE devices SET transcoding_profile_id = ? WHERE id = ?",
            params![profile_id, device_id],
        )
        .map_err(|e| anyhow!("Failed to set transcoding profile: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_init() {
        let db = Database::memory().unwrap();
        // Just checking it doesn't crash and table exists
        db.get_device_mapping("test").unwrap();
    }

    #[test]
    fn test_record_scrobble_and_get_count() {
        let db = Database::memory().unwrap();
        let device_id = "ipod-001";

        // Insert two scrobbles
        db.record_scrobble(device_id, "Pink Floyd", "The Dark Side of the Moon", "Money", 1706745600)
            .unwrap();
        db.record_scrobble(device_id, "Led Zeppelin", "Led Zeppelin IV", "Stairway to Heaven", 1706752800)
            .unwrap();

        let count = db.get_scrobble_count(device_id).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_record_scrobble_dedup() {
        let db = Database::memory().unwrap();
        let device_id = "ipod-001";

        // Insert same scrobble twice — second should be ignored
        db.record_scrobble(device_id, "Pink Floyd", "The Dark Side of the Moon", "Money", 1706745600)
            .unwrap();
        db.record_scrobble(device_id, "Pink Floyd", "The Dark Side of the Moon", "Money", 1706745600)
            .unwrap();

        let count = db.get_scrobble_count(device_id).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_is_scrobble_recorded_false() {
        let db = Database::memory().unwrap();
        let result = db
            .is_scrobble_recorded("ipod-001", "Pink Floyd", "The Dark Side of the Moon", "Money", 1706745600)
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_is_scrobble_recorded_true() {
        let db = Database::memory().unwrap();
        db.record_scrobble("ipod-001", "Pink Floyd", "The Dark Side of the Moon", "Money", 1706745600)
            .unwrap();
        let result = db
            .is_scrobble_recorded("ipod-001", "Pink Floyd", "The Dark Side of the Moon", "Money", 1706745600)
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_is_scrobble_recorded_different_timestamp() {
        let db = Database::memory().unwrap();
        db.record_scrobble("ipod-001", "Pink Floyd", "The Dark Side of the Moon", "Money", 1706745600)
            .unwrap();
        let result = db
            .is_scrobble_recorded("ipod-001", "Pink Floyd", "The Dark Side of the Moon", "Money", 9999999999)
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_upsert_and_get() {
        let db = Database::memory().unwrap();
        let id = "device-123";

        // Initial insert
        db.upsert_device_mapping(id, Some("My Device"), Some("user-456"), Some("{}"))
            .unwrap();

        let mapping = db.get_device_mapping(id).unwrap().unwrap();
        assert_eq!(mapping.id, id);
        assert_eq!(mapping.name, Some("My Device".to_string()));
        assert_eq!(mapping.jellyfin_user_id, Some("user-456".to_string()));

        // Update
        db.upsert_device_mapping(id, Some("Updated Name"), None, None)
            .unwrap();

        let mapping = db.get_device_mapping(id).unwrap().unwrap();
        assert_eq!(mapping.name, Some("Updated Name".to_string()));
        assert_eq!(mapping.jellyfin_user_id, None);
    }

    #[test]
    fn test_auto_sync_on_connect_defaults_false() {
        let db = Database::memory().unwrap();
        let id = "device-auto-1";
        db.upsert_device_mapping(id, Some("Test"), None, None)
            .unwrap();
        let mapping = db.get_device_mapping(id).unwrap().unwrap();
        assert!(!mapping.auto_sync_on_connect);
    }

    #[test]
    fn test_set_auto_sync_on_connect() {
        let db = Database::memory().unwrap();
        let id = "device-auto-2";
        db.upsert_device_mapping(id, Some("Test"), None, None)
            .unwrap();

        // Enable auto-sync
        db.set_auto_sync_on_connect(id, true).unwrap();
        let mapping = db.get_device_mapping(id).unwrap().unwrap();
        assert!(mapping.auto_sync_on_connect);

        // Disable auto-sync
        db.set_auto_sync_on_connect(id, false).unwrap();
        let mapping = db.get_device_mapping(id).unwrap().unwrap();
        assert!(!mapping.auto_sync_on_connect);
    }

    #[test]
    fn test_set_auto_sync_on_connect_nonexistent_device() {
        let db = Database::memory().unwrap();
        let result = db.set_auto_sync_on_connect("nonexistent", true);
        assert!(result.is_err());
    }
}
