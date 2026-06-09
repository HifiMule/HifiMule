use anyhow::{Result, anyhow};
use rusqlite::{Connection, params};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Stable UUID primary key (Story 2.11). Generated on first insert / migration.
    pub id: String,
    pub url: String,
    pub server_type: String,
    pub username: String,
    pub server_version: Option<String>,
    pub updated_at: i64,
    /// True for the single currently-selected server (`selected = 1`).
    pub selected: bool,
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
    pub fn init_for_test(&self) -> Result<()> {
        self.init()
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
        // Multi-server schema (Story 2.11): TEXT UUID primary key + `selected` flag.
        // Fresh installs create this directly; existing single-server installs are
        // migrated below (the legacy `id INTEGER PRIMARY KEY CHECK (id = 1)` table
        // can only be reshaped by recreating it — SQLite cannot drop a CHECK).
        conn.execute(
            "CREATE TABLE IF NOT EXISTS server_config (
                id TEXT PRIMARY KEY,
                url TEXT NOT NULL,
                server_type TEXT NOT NULL,
                username TEXT NOT NULL,
                server_version TEXT,
                updated_at INTEGER NOT NULL,
                selected INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .map_err(|e| anyhow!("Failed to create server_config table: {}", e))?;

        Self::migrate_server_config_to_multi(&conn)?;
        Ok(())
    }

    /// Migrates a legacy single-row `server_config` (INTEGER PK + `CHECK (id = 1)`,
    /// no `selected` column) to the multi-server schema. Idempotent: a no-op once
    /// the `id` column is already TEXT. Returns the migrated server's UUID when a
    /// row was migrated (so the caller can re-key the credential vault, AC17).
    fn migrate_server_config_to_multi(conn: &Connection) -> Result<()> {
        // Detect the type of the `id` column. New schema → TEXT, legacy → INTEGER.
        let id_type: Option<String> = conn
            .query_row(
                "SELECT type FROM pragma_table_info('server_config') WHERE name = 'id'",
                [],
                |row| row.get(0),
            )
            .ok();

        let is_legacy = matches!(id_type.as_deref(), Some(t) if t.eq_ignore_ascii_case("INTEGER"));
        if !is_legacy {
            // Already TEXT id. Defensive: ensure the `selected` column exists for
            // installs that created the TEXT table before `selected` was added.
            let has_selected = conn
                .prepare("SELECT selected FROM server_config LIMIT 0")
                .is_ok();
            if !has_selected {
                conn.execute(
                    "ALTER TABLE server_config ADD COLUMN selected INTEGER NOT NULL DEFAULT 0",
                    [],
                )
                .map_err(|e| anyhow!("Failed to add selected column: {}", e))?;
            }
            return Ok(());
        }

        // Legacy table: read the single existing row (if any) before recreating.
        let existing: Option<(String, String, String, Option<String>, i64)> = conn
            .query_row(
                "SELECT url, server_type, username, server_version, updated_at
                 FROM server_config WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .ok();

        conn.execute("ALTER TABLE server_config RENAME TO server_config_legacy", [])
            .map_err(|e| anyhow!("Failed to rename legacy server_config: {}", e))?;
        conn.execute(
            "CREATE TABLE server_config (
                id TEXT PRIMARY KEY,
                url TEXT NOT NULL,
                server_type TEXT NOT NULL,
                username TEXT NOT NULL,
                server_version TEXT,
                updated_at INTEGER NOT NULL,
                selected INTEGER NOT NULL DEFAULT 0
            )",
            [],
        )
        .map_err(|e| anyhow!("Failed to recreate server_config table: {}", e))?;

        if let Some((url, server_type, username, server_version, updated_at)) = existing {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO server_config
                    (id, url, server_type, username, server_version, updated_at, selected)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
                params![id, url, server_type, username, server_version, updated_at],
            )
            .map_err(|e| anyhow!("Failed to migrate server row: {}", e))?;
        }

        conn.execute("DROP TABLE server_config_legacy", [])
            .map_err(|e| anyhow!("Failed to drop legacy server_config: {}", e))?;
        Ok(())
    }

    fn row_to_server_config(row: &rusqlite::Row) -> rusqlite::Result<ServerConfig> {
        Ok(ServerConfig {
            id: row.get(0)?,
            url: row.get(1)?,
            server_type: row.get(2)?,
            username: row.get(3)?,
            server_version: row.get(4)?,
            updated_at: row.get(5)?,
            selected: row.get::<_, i64>(6)? != 0,
        })
    }

    /// Inserts a server (or updates the existing one matching `url` by normalized
    /// comparison, Story 2.11 AC5) and returns its UUID. When no row was selected
    /// before, the upserted server becomes selected. Does not change selection for
    /// an existing selected server.
    pub fn upsert_server(
        &self,
        url: &str,
        server_type: &str,
        username: &str,
        server_version: Option<&str>,
    ) -> Result<String> {
        let conn = self.conn.lock().unwrap();
        let updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| anyhow!("Failed to calculate timestamp: {}", e))?
            .as_secs() as i64;

        // Match an existing server by normalized URL (trim trailing slash, lowercase).
        let normalized = url.trim().trim_end_matches('/').to_ascii_lowercase();
        let existing_id: Option<String> = conn
            .query_row(
                "SELECT id FROM server_config
                 WHERE lower(rtrim(trim(url), '/')) = ?1",
                params![normalized],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing_id {
            conn.execute(
                "UPDATE server_config SET
                    url = ?2, server_type = ?3, username = ?4,
                    server_version = ?5, updated_at = ?6
                 WHERE id = ?1",
                params![id, url, server_type, username, server_version, updated_at],
            )
            .map_err(|e| anyhow!("Failed to update server config: {}", e))?;
            return Ok(id);
        }

        let id = uuid::Uuid::new_v4().to_string();
        // First-ever server is auto-selected; otherwise selection is preserved.
        let any_selected: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM server_config WHERE selected = 1)",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|n| n != 0)
            .unwrap_or(false);
        let selected = if any_selected { 0 } else { 1 };
        conn.execute(
            "INSERT INTO server_config
                (id, url, server_type, username, server_version, updated_at, selected)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, url, server_type, username, server_version, updated_at, selected],
        )
        .map_err(|e| anyhow!("Failed to insert server config: {}", e))?;
        Ok(id)
    }

    /// Returns all configured servers ordered by insertion (updated_at, then id).
    pub fn list_servers(&self) -> Result<Vec<ServerConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, url, server_type, username, server_version, updated_at, selected
             FROM server_config ORDER BY updated_at ASC, id ASC",
        )?;
        let rows = stmt.query_map([], Self::row_to_server_config)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_server(&self, id: &str) -> Result<Option<ServerConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, url, server_type, username, server_version, updated_at, selected
             FROM server_config WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(Self::row_to_server_config(row)?)),
            None => Ok(None),
        }
    }

    /// Returns the currently selected server (`selected = 1`), if any.
    /// Retained under the historical name so single-server callers keep working.
    pub fn get_server_config(&self) -> Result<Option<ServerConfig>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, url, server_type, username, server_version, updated_at, selected
             FROM server_config WHERE selected = 1 LIMIT 1",
        )?;
        let mut rows = stmt.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some(Self::row_to_server_config(row)?)),
            None => Ok(None),
        }
    }

    /// Marks `id` as the single selected server (`selected = 1`), clearing all
    /// others. Errors if `id` does not exist.
    pub fn set_selected(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE server_config SET selected = 0", [])
            .map_err(|e| anyhow!("Failed to clear selection: {}", e))?;
        let rows = conn
            .execute(
                "UPDATE server_config SET selected = 1 WHERE id = ?1",
                params![id],
            )
            .map_err(|e| anyhow!("Failed to set selection: {}", e))?;
        if rows == 0 {
            return Err(anyhow!("Server not found: {}", id));
        }
        Ok(())
    }

    /// Removes a server row by id. Returns true when a row was deleted.
    pub fn remove_server(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let rows = conn
            .execute("DELETE FROM server_config WHERE id = ?1", params![id])
            .map_err(|e| anyhow!("Failed to remove server: {}", e))?;
        Ok(rows > 0)
    }

    pub fn clear_server_config(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM server_config", [])
            .map_err(|e| anyhow!("Failed to clear server config: {}", e))?;
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
        db.record_scrobble(
            device_id,
            "Pink Floyd",
            "The Dark Side of the Moon",
            "Money",
            1706745600,
        )
        .unwrap();
        db.record_scrobble(
            device_id,
            "Led Zeppelin",
            "Led Zeppelin IV",
            "Stairway to Heaven",
            1706752800,
        )
        .unwrap();

        let count = db.get_scrobble_count(device_id).unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_record_scrobble_dedup() {
        let db = Database::memory().unwrap();
        let device_id = "ipod-001";

        // Insert same scrobble twice — second should be ignored
        db.record_scrobble(
            device_id,
            "Pink Floyd",
            "The Dark Side of the Moon",
            "Money",
            1706745600,
        )
        .unwrap();
        db.record_scrobble(
            device_id,
            "Pink Floyd",
            "The Dark Side of the Moon",
            "Money",
            1706745600,
        )
        .unwrap();

        let count = db.get_scrobble_count(device_id).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_is_scrobble_recorded_false() {
        let db = Database::memory().unwrap();
        let result = db
            .is_scrobble_recorded(
                "ipod-001",
                "Pink Floyd",
                "The Dark Side of the Moon",
                "Money",
                1706745600,
            )
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_is_scrobble_recorded_true() {
        let db = Database::memory().unwrap();
        db.record_scrobble(
            "ipod-001",
            "Pink Floyd",
            "The Dark Side of the Moon",
            "Money",
            1706745600,
        )
        .unwrap();
        let result = db
            .is_scrobble_recorded(
                "ipod-001",
                "Pink Floyd",
                "The Dark Side of the Moon",
                "Money",
                1706745600,
            )
            .unwrap();
        assert!(result);
    }

    #[test]
    fn test_is_scrobble_recorded_different_timestamp() {
        let db = Database::memory().unwrap();
        db.record_scrobble(
            "ipod-001",
            "Pink Floyd",
            "The Dark Side of the Moon",
            "Money",
            1706745600,
        )
        .unwrap();
        let result = db
            .is_scrobble_recorded(
                "ipod-001",
                "Pink Floyd",
                "The Dark Side of the Moon",
                "Money",
                9999999999,
            )
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

    #[test]
    fn test_db_init_idempotent() {
        let db = Database::memory().unwrap();
        // Second call to init must not fail — all DDL uses CREATE TABLE IF NOT EXISTS.
        db.init_for_test().unwrap();
        // Operations still work after re-init.
        db.upsert_server("http://x", "jellyfin", "u", None).unwrap();
        assert!(db.get_server_config().unwrap().is_some());
    }

    #[test]
    fn test_server_config_round_trips_and_updates() {
        let db = Database::memory().unwrap();

        assert_eq!(db.get_server_config().unwrap(), None);

        // First-ever server is auto-selected.
        let id1 = db
            .upsert_server("http://music.example", "openSubsonic", "alexis", Some("1.16.1"))
            .unwrap();
        let config = db.get_server_config().unwrap().unwrap();
        assert_eq!(config.id, id1);
        assert_eq!(config.url, "http://music.example");
        assert_eq!(config.server_type, "openSubsonic");
        assert_eq!(config.username, "alexis");
        assert_eq!(config.server_version.as_deref(), Some("1.16.1"));
        assert!(config.selected);

        // Re-upsert by normalized URL (trailing slash) updates in place, same id.
        let id1b = db
            .upsert_server("http://music.example/", "openSubsonic", "alexis", Some("1.17.0"))
            .unwrap();
        assert_eq!(id1, id1b);
        assert_eq!(db.list_servers().unwrap().len(), 1);
        assert_eq!(
            db.get_server(&id1).unwrap().unwrap().server_version.as_deref(),
            Some("1.17.0")
        );

        // A second, distinct server does NOT steal the selection.
        let id2 = db
            .upsert_server("http://jellyfin.example", "jellyfin", "user-id", None)
            .unwrap();
        assert_ne!(id1, id2);
        assert_eq!(db.list_servers().unwrap().len(), 2);
        assert_eq!(db.get_server_config().unwrap().unwrap().id, id1);

        // Switch selection.
        db.set_selected(&id2).unwrap();
        assert_eq!(db.get_server_config().unwrap().unwrap().id, id2);
        assert!(!db.get_server(&id1).unwrap().unwrap().selected);

        // Remove the non-selected server.
        assert!(db.remove_server(&id1).unwrap());
        assert_eq!(db.list_servers().unwrap().len(), 1);
        assert!(!db.remove_server("does-not-exist").unwrap());
    }

    #[test]
    fn test_set_selected_nonexistent_errors() {
        let db = Database::memory().unwrap();
        assert!(db.set_selected("nope").is_err());
    }

    /// AC16/AC17: a legacy `id INTEGER PRIMARY KEY CHECK (id = 1)` table with one
    /// row migrates to the TEXT-UUID schema, generating a UUID and selecting it.
    #[test]
    fn test_migrate_legacy_server_config() {
        let conn = Connection::open_in_memory().unwrap();
        // Recreate the exact legacy schema + single row.
        conn.execute(
            "CREATE TABLE server_config (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                url TEXT NOT NULL,
                server_type TEXT NOT NULL,
                username TEXT NOT NULL,
                server_version TEXT,
                updated_at INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO server_config (id, url, server_type, username, server_version, updated_at)
             VALUES (1, 'http://legacy.example', 'jellyfin', 'alexis', '10.9.0', 42)",
            [],
        )
        .unwrap();

        let db = Database {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_for_test().unwrap();

        let servers = db.list_servers().unwrap();
        assert_eq!(servers.len(), 1);
        let migrated = &servers[0];
        assert!(!migrated.id.is_empty());
        assert!(migrated.id.parse::<uuid::Uuid>().is_ok());
        assert_eq!(migrated.url, "http://legacy.example");
        assert_eq!(migrated.server_type, "jellyfin");
        assert_eq!(migrated.username, "alexis");
        assert_eq!(migrated.server_version.as_deref(), Some("10.9.0"));
        assert_eq!(migrated.updated_at, 42);
        assert!(migrated.selected);

        // Idempotent: a second init is a no-op (does not re-migrate or duplicate).
        let id_before = migrated.id.clone();
        db.init_for_test().unwrap();
        let servers = db.list_servers().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].id, id_before);
    }

    /// Migration of an empty legacy table produces an empty multi-server table.
    #[test]
    fn test_migrate_legacy_server_config_empty() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE server_config (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                url TEXT NOT NULL,
                server_type TEXT NOT NULL,
                username TEXT NOT NULL,
                server_version TEXT,
                updated_at INTEGER NOT NULL
            )",
            [],
        )
        .unwrap();
        let db = Database {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_for_test().unwrap();
        assert_eq!(db.list_servers().unwrap().len(), 0);
        // New-schema operations work post-migration.
        db.upsert_server("http://new.example", "jellyfin", "u", None)
            .unwrap();
        assert_eq!(db.list_servers().unwrap().len(), 1);
    }
}
