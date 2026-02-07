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
                last_seen_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )
        .map_err(|e| anyhow!("Failed to create devices table: {}", e))?;
        Ok(())
    }

    pub fn get_device_mapping(&self, id: &str) -> Result<Option<DeviceMapping>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, jellyfin_user_id, sync_rules, last_seen_at FROM devices WHERE id = ?",
        )?;

        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(DeviceMapping {
                id: row.get(0)?,
                name: row.get(1)?,
                jellyfin_user_id: row.get(2)?,
                sync_rules: row.get(3)?,
                last_seen_at: row.get(4)?,
            }))
        } else {
            Ok(None)
        }
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
}
