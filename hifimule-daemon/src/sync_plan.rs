//! Ephemeral, owned transfer metadata. Connections and decoded pages are bounded;
//! the last owner removes the database, including abandoned preparations.
use crate::{device::DeviceManifest, sync::*};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::VecDeque,
    hash::{Hash, Hasher},
    marker::PhantomData,
    sync::Arc,
};

pub const PAGE_SIZE: usize = 500;

pub struct PreparedPlan {
    pub id: String,
    pub plan: SyncPlan,
    pub fingerprint: String,
    pub force: bool,
    pub created_at: std::time::Instant,
    pub _expiry: ExpiryGuard,
}

pub struct ExpiryGuard(tokio::task::AbortHandle);
impl ExpiryGuard {
    pub fn new(handle: tokio::task::AbortHandle) -> Self {
        Self(handle)
    }
}
impl Drop for ExpiryGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub fn target_fingerprint(target: &SyncTarget, config: &str) -> Result<String> {
    Ok(checksum(&format!(
        "{}|{}|{}|{}",
        target.path.display(),
        Arc::as_ptr(&target.io) as *const () as usize,
        serde_json::to_string(&target.manifest)?,
        config
    )))
}

/// Retain configuration only. Identity/path projections live in the owned plan.
pub fn compact_target(target: &mut SyncTarget) {
    target.manifest.synced_items = Vec::new();
    target.manifest.pending_item_ids = Vec::new();
    target.manifest.basket_items = Vec::new();
    target.manifest.playlists = Vec::new();
}

#[derive(Clone)]
pub struct PlanList<T> {
    path: Arc<tempfile::TempPath>,
    kind: &'static str,
    count: usize,
    _item: PhantomData<T>,
}

fn connection(path: &std::path::Path) -> Result<Connection> {
    let db = Connection::open(path).context("Cannot open temporary sync plan")?;
    db.execute_batch("PRAGMA cache_size=-4096; PRAGMA mmap_size=0; PRAGMA temp_store=FILE;")?;
    Ok(db)
}

fn checksum(data: &str) -> String {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hash);
    format!("{:016x}", hash.finish())
}

fn record_checksum(
    kind: &str,
    seq: i64,
    id: Option<&str>,
    server: Option<&str>,
    priority: bool,
    data: &str,
) -> String {
    checksum(
        &serde_json::to_string(&(kind, seq, id, server, priority, data))
            .expect("serializable record fields"),
    )
}

impl<T: DeserializeOwned> PlanList<T> {
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub fn iter(&self) -> Result<PlanCursor<T>> {
        self.cursor(None, false)
    }
    pub fn cursor(&self, server: Option<Option<String>>, priority: bool) -> Result<PlanCursor<T>> {
        Ok(PlanCursor {
            db: connection(&self.path)?,
            kind: self.kind,
            server,
            priority,
            after_priority: -1,
            after_seq: -1,
            page: VecDeque::new(),
            done: false,
            _path: Arc::clone(&self.path),
        })
    }
    pub fn page(&self, offset: usize) -> Result<Vec<T>> {
        if offset >= self.count {
            return Ok(Vec::new());
        }
        let mut cursor = self.iter()?;
        // Explicit detail pages seek once; sequential execution uses indexed keysets.
        if offset > 0 {
            let seq: i64 = cursor.db.query_row(
                "SELECT seq FROM operations WHERE kind=?1 ORDER BY seq LIMIT 1 OFFSET ?2",
                params![self.kind, offset as i64],
                |row| row.get(0),
            )?;
            cursor.after_seq = seq - 1;
        }
        cursor.load_page()?;
        Ok(cursor.page.into_iter().collect())
    }
}

pub struct PlanCursor<T> {
    db: Connection,
    kind: &'static str,
    server: Option<Option<String>>,
    priority: bool,
    after_priority: i64,
    after_seq: i64,
    page: VecDeque<T>,
    done: bool,
    _path: Arc<tempfile::TempPath>,
}

impl<T: DeserializeOwned> PlanCursor<T> {
    fn load_page(&mut self) -> Result<()> {
        let sql = if self.priority {
            "SELECT data,checksum,kind,seq,id,server,priority FROM operations WHERE kind=?1 AND server IS ?2 AND (priority,seq) > (?3,?4) ORDER BY priority,seq LIMIT ?5"
        } else {
            "SELECT data,checksum,kind,seq,id,server,priority FROM operations WHERE kind=?1 AND seq > ?4 ORDER BY seq LIMIT ?5"
        };
        let mut stmt = self.db.prepare(sql)?;
        let server = self.server.as_ref().and_then(|s| s.as_deref());
        let rows = stmt.query_map(
            params![
                self.kind,
                server,
                self.after_priority,
                self.after_seq,
                PAGE_SIZE as i64
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            },
        )?;
        for row in rows {
            let (data, expected, kind, seq, id, server, priority) = row?;
            if !matches!(priority, 0 | 1)
                || record_checksum(
                    &kind,
                    seq,
                    id.as_deref(),
                    server.as_deref(),
                    priority == 1,
                    &data,
                ) != expected
            {
                bail!("Temporary sync plan record is corrupt");
            }
            self.page.push_back(
                serde_json::from_str(&data).context("Invalid temporary sync plan record")?,
            );
            self.after_priority = priority;
            self.after_seq = seq;
        }
        self.done = self.page.len() < PAGE_SIZE;
        Ok(())
    }
}

impl<T: DeserializeOwned> Iterator for PlanCursor<T> {
    type Item = Result<T>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.page.is_empty() && !self.done {
            if let Err(error) = self.load_page() {
                self.done = true;
                return Some(Err(error));
            }
        }
        self.page.pop_front().map(Ok)
    }
}

#[derive(Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidentIdentity {
    pub jellyfin_id: String,
    pub server_id: Option<String>,
    pub local_path: String,
    pub is_auto_fill: bool,
}

#[derive(Clone)]
pub struct SyncPlan {
    path: Arc<tempfile::TempPath>,
    pub adds: PlanList<SyncAddItem>,
    pub deletes: PlanList<SyncDeleteItem>,
    pub id_changes: PlanList<SyncIdChangeItem>,
    pub blocked: PlanList<SyncBlockedItem>,
    pub playlists: PlanList<PlaylistSyncItem>,
    pub residents: PlanList<ResidentIdentity>,
    pub provenance: PlanList<(String, bool)>,
    pub unchanged: usize,
    pub pity_fired_servers: Vec<String>,
    pub total_bytes: u64,
    pub destructive_cleanup_count: usize,
    pub change_reasons: Vec<SyncReasonSummary>,
    expected_rows: usize,
}

impl SyncPlan {
    pub fn from_delta(delta: SyncDelta, manifest: &DeviceManifest) -> Result<Self> {
        let total_bytes = delta.adds.iter().map(|a| a.size_bytes).sum::<u64>()
            + delta.id_changes.iter().map(|a| a.size_bytes).sum::<u64>();
        let destructive_cleanup_count = destructive_cleanup_count(&delta, manifest);
        let change_reasons = change_reason_summary(&delta);
        let path = Arc::new(
            tempfile::Builder::new()
                .prefix("hifimule-sync-plan-")
                .suffix(".sqlite")
                .tempfile()?
                .into_temp_path(),
        );
        let mut db = connection(&path)?;
        db.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE operations(kind TEXT NOT NULL,seq INTEGER NOT NULL,id TEXT,server TEXT,priority INTEGER NOT NULL CHECK(priority IN (0,1)),data TEXT NOT NULL,checksum TEXT NOT NULL,PRIMARY KEY(kind,seq)); CREATE INDEX provider_priority ON operations(kind,server,priority,seq); CREATE INDEX operation_identity ON operations(kind,id); CREATE TABLE integrity(rows INTEGER NOT NULL);")?;
        let tx = db.transaction()?;
        let mut rows = 0;
        let mut insert = |kind: &str,
                          seq: usize,
                          id: Option<&str>,
                          server: Option<&str>,
                          priority: bool,
                          value: &dyn erased_marker::Value|
         -> Result<()> {
            let data = value.json()?;
            tx.execute(
                "INSERT INTO operations VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    kind,
                    seq as i64,
                    id,
                    server,
                    priority,
                    data,
                    record_checksum(kind, seq as i64, id, server, priority, &data)
                ],
            )?;
            rows += 1;
            Ok(())
        };
        let list = |kind, count| PlanList {
            path: Arc::clone(&path),
            kind,
            count,
            _item: PhantomData,
        };
        let adds = list("add", delta.adds.len());
        for (seq, value) in delta.adds.into_iter().enumerate() {
            insert(
                "add",
                seq,
                Some(&value.jellyfin_id),
                value.server_id.as_deref(),
                value.is_auto_fill,
                &value,
            )?;
        }
        let deletes = PlanList {
            path: Arc::clone(&path),
            kind: "delete",
            count: delta.deletes.len(),
            _item: PhantomData,
        };
        for (seq, value) in delta.deletes.into_iter().enumerate() {
            insert("delete", seq, Some(&value.jellyfin_id), None, false, &value)?;
        }
        let id_changes = PlanList {
            path: Arc::clone(&path),
            kind: "id",
            count: delta.id_changes.len(),
            _item: PhantomData,
        };
        for (seq, value) in delta.id_changes.into_iter().enumerate() {
            insert(
                "id",
                seq,
                Some(&value.new_jellyfin_id),
                value.source_server_id.as_deref(),
                false,
                &value,
            )?;
        }
        let blocked = PlanList {
            path: Arc::clone(&path),
            kind: "blocked",
            count: delta.blocked.len(),
            _item: PhantomData,
        };
        for (seq, value) in delta.blocked.into_iter().enumerate() {
            insert("blocked", seq, None, None, false, &value)?;
        }
        let playlists = PlanList {
            path: Arc::clone(&path),
            kind: "playlist",
            count: delta.playlists.len(),
            _item: PhantomData,
        };
        let mut track_seq = 0;
        for (seq, mut value) in delta.playlists.into_iter().enumerate() {
            let owner = seq.to_string();
            for track in std::mem::take(&mut value.tracks) {
                insert(
                    "track",
                    track_seq,
                    Some(&track.jellyfin_id),
                    Some(&owner),
                    false,
                    &track,
                )?;
                track_seq += 1;
            }
            insert(
                "playlist",
                seq,
                Some(&value.jellyfin_id),
                None,
                false,
                &value,
            )?;
        }
        let provenance = PlanList {
            path: Arc::clone(&path),
            kind: "provenance",
            count: delta.provenance_updates.len(),
            _item: PhantomData,
        };
        for (seq, value) in delta.provenance_updates.into_iter().enumerate() {
            insert("provenance", seq, Some(&value.0), None, false, &value)?;
        }
        let residents = PlanList {
            path: Arc::clone(&path),
            kind: "resident",
            count: manifest.synced_items.len(),
            _item: PhantomData,
        };
        for (seq, value) in manifest.synced_items.iter().enumerate() {
            insert(
                "resident",
                seq,
                Some(&value.jellyfin_id),
                value.server_id.as_deref(),
                value.is_auto_fill,
                &ResidentIdentity {
                    jellyfin_id: value.jellyfin_id.clone(),
                    server_id: value.server_id.clone(),
                    local_path: value.local_path.clone(),
                    is_auto_fill: value.is_auto_fill,
                },
            )?;
        }
        drop(insert);
        tx.execute("INSERT INTO integrity VALUES (?1)", [rows as i64])?;
        tx.commit()?;
        drop(db);
        Ok(Self {
            path,
            adds,
            deletes,
            id_changes,
            blocked,
            playlists,
            provenance,
            residents,
            unchanged: delta.unchanged,
            pity_fired_servers: delta.pity_fired_servers,
            total_bytes,
            destructive_cleanup_count,
            change_reasons,
            expected_rows: rows,
        })
    }

    pub fn validate(&self) -> Result<()> {
        let db = connection(&self.path)?;
        let integrity: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if integrity != "ok" {
            bail!("Temporary sync plan integrity check failed: {integrity}");
        }
        let recorded: i64 = db.query_row("SELECT rows FROM integrity", [], |r| r.get(0))?;
        let actual: i64 = db.query_row("SELECT count(*) FROM operations", [], |r| r.get(0))?;
        if recorded != self.expected_rows as i64 || actual != recorded {
            bail!("Temporary sync plan is incomplete");
        }
        let mut stmt =
            db.prepare("SELECT data,checksum,kind,seq,id,server,priority FROM operations")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let data: String = row.get(0)?;
            let kind: String = row.get(2)?;
            let seq: i64 = row.get(3)?;
            let id: Option<String> = row.get(4)?;
            let server: Option<String> = row.get(5)?;
            let priority: i64 = row.get(6)?;
            if !matches!(priority, 0 | 1)
                || record_checksum(
                    &kind,
                    seq,
                    id.as_deref(),
                    server.as_deref(),
                    priority == 1,
                    &data,
                ) != row.get::<_, String>(1)?
            {
                bail!("Temporary sync plan record is corrupt");
            }
        }
        Ok(())
    }

    pub fn provider_groups(&self) -> Result<Vec<(Option<String>, usize, usize)>> {
        let db = connection(&self.path)?;
        let mut stmt = db.prepare("SELECT server,count(*),sum(CASE WHEN priority=0 THEN 1 ELSE 0 END) FROM operations WHERE kind='add' GROUP BY server ORDER BY min(seq)")?;
        Ok(stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get::<_, i64>(1)? as usize,
                    r.get::<_, i64>(2)? as usize,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn playlist_tracks(&self, ordinal: usize) -> Result<PlanCursor<PlaylistTrackInfo>> {
        PlanList {
            path: Arc::clone(&self.path),
            kind: "track",
            count: 0,
            _item: PhantomData,
        }
        .cursor(Some(Some(ordinal.to_string())), true)
    }

    pub fn get<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Result<Option<T>> {
        let db = connection(&self.path)?;
        let record: Option<(String,String,i64,Option<String>,i64)> = db.query_row("SELECT data,checksum,seq,server,priority FROM operations WHERE kind=?1 AND id=?2 ORDER BY seq LIMIT 1", params![kind,id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        record
            .map(|(data, expected, seq, server, priority)| {
                if !matches!(priority, 0 | 1)
                    || record_checksum(kind, seq, Some(id), server.as_deref(), priority == 1, &data)
                        != expected
                {
                    bail!("Temporary sync plan record is corrupt");
                }
                Ok(serde_json::from_str(&data)?)
            })
            .transpose()
    }
    pub fn pending_ids(&self) -> Result<Vec<String>> {
        let db = connection(&self.path)?;
        let mut stmt = db.prepare("SELECT id FROM operations WHERE kind IN ('add','id') ORDER BY CASE kind WHEN 'add' THEN 0 ELSE 1 END,seq")?;
        Ok(stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn summary(&self, id: &str) -> serde_json::Value {
        serde_json::json!({ "planId":id, "addsCount":self.adds.len(), "deletesCount":self.deletes.len(), "idChangesCount":self.id_changes.len(), "blockedCount":self.blocked.len(), "playlistsCount":self.playlists.len(), "unchanged":self.unchanged, "totalBytes":self.total_bytes, "destructiveCleanupCount":self.destructive_cleanup_count, "destructiveCleanupThreshold":DESTRUCTIVE_CLEANUP_THRESHOLD, "changeReasons":self.change_reasons })
    }
}

// Local object-safe serialization keeps insertion ownership explicit without a new dependency.
mod erased_marker {
    pub trait Value {
        fn json(&self) -> anyhow::Result<String>;
    }
    impl<T: serde::Serialize> Value for T {
        fn json(&self) -> anyhow::Result<String> {
            Ok(serde_json::to_string(self)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(count: usize) -> SyncDelta {
        serde_json::from_value(serde_json::json!({
            "adds":(0..count).map(|i|serde_json::json!({"jellyfinId":format!("track-{i:04}"),"name":format!("Song {i}"),"album":null,"artist":null,"sizeBytes":7,"etag":null,"isAutoFill":i%2==0,"serverId":if i%3==0 {"a"} else {"b"}})).collect::<Vec<_>>(),
            "deletes":[],"idChanges":[],"unchanged":0,
            "playlists":[{"jellyfinId":"mix","name":"Ordered mix","tracks":[{"jellyfinId":"track-0002","artist":"Two","runTimeSeconds":120},{"jellyfinId":"track-0001","artist":"One","runTimeSeconds":60},{"jellyfinId":"track-0002","artist":"Two","runTimeSeconds":120}]}],
            "provenanceUpdates":{"resident":true}
        })).unwrap()
    }

    #[test]
    fn provider_pages_preserve_manual_first_and_original_order_without_omissions() {
        let plan = SyncPlan::from_delta(delta(1001), &DeviceManifest::default()).unwrap();
        assert_eq!(plan.adds.len(), 1001);
        assert_eq!(plan.total_bytes, 7007);
        let mut cursor = plan.adds.cursor(Some(Some("a".into())), true).unwrap();
        let mut ids = Vec::new();
        while let Some(item) = cursor.next() {
            let item = item.unwrap();
            assert!(cursor.page.len() < PAGE_SIZE);
            ids.push(item.jellyfin_id);
        }
        assert_eq!(&ids[..3], ["track-0003", "track-0009", "track-0015"]);
        assert_eq!(&ids[167..170], ["track-0000", "track-0006", "track-0012"]);
        assert_eq!(ids.len(), 334);
        assert_eq!(plan.adds.page(500).unwrap().len(), 500);
        assert_eq!(plan.adds.page(1000).unwrap()[0].jellyfin_id, "track-1000");
    }

    #[test]
    fn ordered_playlist_tracks_round_trip_duplicates_separately_from_metadata() {
        let plan = SyncPlan::from_delta(delta(3), &DeviceManifest::default()).unwrap();
        assert!(
            plan.playlists
                .iter()
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .tracks
                .is_empty()
        );
        let tracks: Vec<_> = plan
            .playlist_tracks(0)
            .unwrap()
            .map(|t| t.unwrap().jellyfin_id)
            .collect();
        assert_eq!(tracks, ["track-0002", "track-0001", "track-0002"]);
        assert_eq!(
            plan.provenance.iter().unwrap().next().unwrap().unwrap(),
            ("resident".into(), true)
        );
    }

    #[test]
    fn truncated_or_corrupted_plan_is_rejected_before_execution() {
        let plan = SyncPlan::from_delta(delta(3), &DeviceManifest::default()).unwrap();
        plan.validate().unwrap();
        connection(&plan.path)
            .unwrap()
            .execute(
                "UPDATE operations SET priority=1 WHERE kind='add' AND seq=1",
                [],
            )
            .unwrap();
        assert!(plan.validate().unwrap_err().to_string().contains("corrupt"));
        let plan = SyncPlan::from_delta(delta(3), &DeviceManifest::default()).unwrap();
        connection(&plan.path)
            .unwrap()
            .execute("DELETE FROM operations WHERE kind='add' AND seq=1", [])
            .unwrap();
        assert!(
            plan.validate()
                .unwrap_err()
                .to_string()
                .contains("incomplete")
        );
    }

    #[test]
    fn last_plan_or_cursor_owner_removes_ephemeral_file() {
        let plan = SyncPlan::from_delta(delta(1), &DeviceManifest::default()).unwrap();
        let path = plan.path.to_path_buf();
        let cursor = plan.adds.iter().unwrap();
        drop(plan);
        assert!(path.exists());
        drop(cursor);
        assert!(!path.exists());
    }

    #[test]
    fn replacement_lookup_rejects_modified_metadata_during_transfer() {
        let plan = SyncPlan::from_delta(delta(1), &DeviceManifest::default()).unwrap();
        connection(&plan.path)
            .unwrap()
            .execute(
                "UPDATE operations SET data=replace(data,'Song 0','Altered song') WHERE kind='add'",
                [],
            )
            .unwrap();
        assert!(plan.get::<SyncAddItem>("add", "track-0000").is_err());
    }

    #[test]
    fn reviewed_same_native_playlist_ids_do_not_mix_their_track_rows() {
        let mut delta = delta(3);
        let mut second = delta.playlists[0].clone();
        second.name = "Other server playlist".into();
        second.tracks = vec![PlaylistTrackInfo {
            jellyfin_id: "other-server-track".into(),
            artist: None,
            run_time_seconds: 1,
        }];
        delta.playlists.push(second);
        let plan = SyncPlan::from_delta(delta, &Default::default()).unwrap();
        let tracks: Vec<_> = plan
            .playlist_tracks(0)
            .unwrap()
            .map(|t| t.unwrap().jellyfin_id)
            .collect();
        assert_eq!(tracks, ["track-0002", "track-0001", "track-0002"]);
        let second: Vec<_> = plan
            .playlist_tracks(1)
            .unwrap()
            .map(|t| t.unwrap().jellyfin_id)
            .collect();
        assert_eq!(second, ["other-server-track"]);
    }

    #[test]
    fn reviewed_keyset_provider_cursor_crosses_pages_without_reordering_or_omissions() {
        let mut delta = delta(3001);
        for item in &mut delta.adds {
            item.server_id = Some("single".into());
        }
        let expected: Vec<_> = delta
            .adds
            .iter()
            .filter(|item| !item.is_auto_fill)
            .chain(delta.adds.iter().filter(|item| item.is_auto_fill))
            .map(|item| item.jellyfin_id.clone())
            .collect();
        let plan = SyncPlan::from_delta(delta, &Default::default()).unwrap();
        let mut cursor = plan.adds.cursor(Some(Some("single".into())), true).unwrap();
        let mut actual = Vec::new();
        while let Some(item) = cursor.next() {
            actual.push(item.unwrap().jellyfin_id);
            assert!(cursor.page.len() < PAGE_SIZE);
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn reviewed_noncanonical_priority_is_rejected_even_if_boolean_checksum_would_match() {
        let plan = SyncPlan::from_delta(delta(3), &Default::default()).unwrap();
        let db = connection(&plan.path).unwrap();
        assert!(
            db.execute(
                "UPDATE operations SET priority=2 WHERE kind='add' AND seq=0",
                []
            )
            .is_err()
        );
        db.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE operations SET priority=2 WHERE kind='add' AND seq=0;").unwrap();
        assert!(plan.validate().is_err());
        assert!(plan.get::<SyncAddItem>("add", "track-0000").is_err());
        assert!(plan.adds.iter().unwrap().next().unwrap().is_err());
    }
}
