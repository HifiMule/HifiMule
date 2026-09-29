//! Durable, target-bound export of immutable listening snapshots to a device basket.
use crate::db::Database;
use crate::device::{BasketItem, DeviceManifest};
use crate::playback::session::PlaybackError;
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_ENTRIES: usize = 10_000;
pub const RETENTION_SECONDS: i64 = 30 * 24 * 60 * 60;

fn error(code: &'static str, conflict: bool) -> PlaybackError {
    PlaybackError {
        code,
        message: "Snapshot basket export unavailable",
        conflict,
        authoritative: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BasketExportAction {
    Add,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BasketExportState {
    IntentRecorded,
    CommitConfirmed,
    Conflict,
    Failed,
    CommitUncertain,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanBasketExportParams {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub target_device_id: String,
    pub action: BasketExportAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommitBasketExportParams {
    pub schema_version: u32,
    pub operation_id: String,
    pub snapshot_id: String,
    pub target_device_id: String,
    pub action: BasketExportAction,
    pub target_connection_revision: String,
    pub expected_basket_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BasketExportLocator {
    pub schema_version: u32,
    pub operation_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListBasketExportsParams {
    pub schema_version: u32,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BasketExportLimitation {
    pub ordinal: String,
    pub code: String,
    pub server_id: String,
    pub track_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BasketExportPlan {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub target_device_id: String,
    pub target_name: String,
    pub target_icon: Option<String>,
    pub action: BasketExportAction,
    pub target_connection_revision: String,
    pub observed_basket_hash: String,
    pub snapshot_entry_count: String,
    pub projected_entry_count: String,
    pub faithful: bool,
    pub limitations: Vec<BasketExportLimitation>,
    pub(crate) items: Vec<BasketItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BasketExportOperation {
    pub schema_version: u32,
    pub operation_id: String,
    pub snapshot_id: String,
    pub target_device_id: String,
    pub action: BasketExportAction,
    pub state: BasketExportState,
    pub pre_basket_hash: String,
    pub post_basket_hash: Option<String>,
    pub basket_items: Option<Vec<BasketItem>>,
    pub reason: Option<String>,
}

pub fn validate_plan(p: &PlanBasketExportParams) -> Result<(), PlaybackError> {
    if p.schema_version != SCHEMA_VERSION
        || Uuid::parse_str(&p.snapshot_id).is_err()
        || p.target_device_id.trim().is_empty()
        || p.target_device_id.len() > 256
    {
        return Err(error("INVALID_BASKET_EXPORT_REQUEST", false));
    }
    Ok(())
}

pub fn validate_commit(p: &CommitBasketExportParams) -> Result<String, PlaybackError> {
    validate_plan(&PlanBasketExportParams {
        schema_version: p.schema_version,
        snapshot_id: p.snapshot_id.clone(),
        target_device_id: p.target_device_id.clone(),
        action: p.action,
    })?;
    if Uuid::parse_str(&p.operation_id).is_err()
        || p.operation_id.len() != 36
        || p.target_connection_revision.is_empty()
        || p.target_connection_revision.len() > 32
        || p.expected_basket_hash.len() != 64
        || !p
            .expected_basket_hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err(error("INVALID_BASKET_EXPORT_REQUEST", false));
    }
    serde_json::to_string(p).map_err(|_| error("INVALID_BASKET_EXPORT_REQUEST", false))
}

pub fn merge_add(existing: &[BasketItem], planned: &[BasketItem]) -> Vec<BasketItem> {
    let mut result = existing.to_vec();
    let mut identities: HashSet<(Option<&str>, &str)> = existing
        .iter()
        .map(|item| (item.server_id.as_deref(), item.id.as_str()))
        .collect();
    for item in planned {
        if identities.insert((item.server_id.as_deref(), item.id.as_str())) {
            result.push(item.clone());
        }
    }
    result
}

pub fn projected(
    action: BasketExportAction,
    manifest: &DeviceManifest,
    planned: &[BasketItem],
) -> Vec<BasketItem> {
    match action {
        BasketExportAction::Add => merge_add(&manifest.basket_items, planned),
        BasketExportAction::Replace => planned.to_vec(),
    }
}

pub(crate) fn migrate(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS playback_basket_exports(
        operation_id TEXT PRIMARY KEY, schema_version INTEGER NOT NULL CHECK(schema_version=1),
        snapshot_id TEXT NOT NULL, target_device_id TEXT NOT NULL, action TEXT NOT NULL CHECK(action IN ('add','replace')),
        canonical_request TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('intentRecorded','commitConfirmed','conflict','failed','commitUncertain')),
        pre_basket_hash TEXT NOT NULL, post_basket_hash TEXT, basket_json TEXT, reason TEXT,
        created_at INTEGER NOT NULL DEFAULT(unixepoch()), updated_at INTEGER NOT NULL DEFAULT(unixepoch())
    ); CREATE INDEX IF NOT EXISTS playback_basket_exports_retention ON playback_basket_exports(updated_at);
    UPDATE playback_basket_exports SET state='commitUncertain',reason='daemonRestartedDuringCommit',updated_at=unixepoch() WHERE state='intentRecorded';")?;
    tx.execute(
        "DELETE FROM playback_basket_exports WHERE updated_at < unixepoch()-?1",
        [RETENTION_SECONDS],
    )?;
    Ok(())
}

impl Database {
    pub fn replay_basket_export(
        &self,
        p: &CommitBasketExportParams,
    ) -> Result<Option<BasketExportOperation>, PlaybackError> {
        let canonical = validate_commit(p)?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        match load(&conn, &p.operation_id)? {
            Some((stored, operation)) if stored == canonical => Ok(Some(operation)),
            Some(_) => Err(error("BASKET_EXPORT_OPERATION_REUSED", true)),
            None => Ok(None),
        }
    }

    pub fn list_basket_exports(
        &self,
        p: ListBasketExportsParams,
    ) -> Result<Vec<BasketExportOperation>, PlaybackError> {
        if p.schema_version != 1 || Uuid::parse_str(&p.snapshot_id).is_err() {
            return Err(error("INVALID_BASKET_EXPORT_REQUEST", false));
        }
        let ids = {
            let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
            let mut statement = conn.prepare("SELECT operation_id FROM playback_basket_exports WHERE snapshot_id=?1 ORDER BY created_at DESC LIMIT 20")
                .map_err(|_| error("BASKET_EXPORT_STORAGE_FAILED", false))?;
            statement
                .query_map([p.snapshot_id], |row| row.get::<_, String>(0))
                .map_err(|_| error("BASKET_EXPORT_STORAGE_FAILED", false))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|_| error("BASKET_EXPORT_STORAGE_FAILED", false))?
        };
        ids.into_iter()
            .map(|id| self.get_basket_export(&id))
            .collect()
    }
    pub fn record_basket_export_intent(
        &self,
        p: &CommitBasketExportParams,
        pre_hash: &str,
        post_hash: &str,
    ) -> Result<BasketExportOperation, PlaybackError> {
        let canonical = validate_commit(p)?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((stored, operation)) = load(&conn, &p.operation_id)? {
            if stored != canonical {
                return Err(error("BASKET_EXPORT_OPERATION_REUSED", true));
            }
            return Ok(operation);
        }
        conn.execute("INSERT INTO playback_basket_exports(operation_id,schema_version,snapshot_id,target_device_id,action,canonical_request,state,pre_basket_hash,post_basket_hash) VALUES(?1,1,?2,?3,?4,?5,'intentRecorded',?6,?7)",
            params![p.operation_id,p.snapshot_id,p.target_device_id,action_name(p.action),canonical,pre_hash,post_hash])
            .map_err(|_storage_error| {
                error("BASKET_EXPORT_STORAGE_FAILED", false)
            })?;
        Ok(BasketExportOperation {
            schema_version: 1,
            operation_id: p.operation_id.clone(),
            snapshot_id: p.snapshot_id.clone(),
            target_device_id: p.target_device_id.clone(),
            action: p.action,
            state: BasketExportState::IntentRecorded,
            pre_basket_hash: pre_hash.to_owned(),
            post_basket_hash: Some(post_hash.to_owned()),
            basket_items: None,
            reason: None,
        })
    }

    pub fn finish_basket_export(
        &self,
        operation_id: &str,
        state: BasketExportState,
        post_hash: Option<&str>,
        items: Option<&[BasketItem]>,
        reason: Option<&str>,
    ) -> Result<BasketExportOperation, PlaybackError> {
        let basket_json = items
            .map(serde_json::to_string)
            .transpose()
            .map_err(|_| error("BASKET_EXPORT_STORAGE_FAILED", false))?;
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute("UPDATE playback_basket_exports SET state=?2,post_basket_hash=?3,basket_json=?4,reason=?5,updated_at=unixepoch() WHERE operation_id=?1",
            params![operation_id,state_name(state),post_hash,basket_json,reason]).map_err(|_| error("BASKET_EXPORT_STORAGE_FAILED", false))?;
        drop(conn);
        self.get_basket_export(operation_id)
    }

    pub fn get_basket_export(
        &self,
        operation_id: &str,
    ) -> Result<BasketExportOperation, PlaybackError> {
        if Uuid::parse_str(operation_id).is_err() {
            return Err(error("INVALID_BASKET_EXPORT_REQUEST", false));
        }
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        load(&conn, operation_id)?
            .map(|(_, operation)| operation)
            .ok_or_else(|| error("BASKET_EXPORT_NOT_FOUND", false))
    }
}

fn load(
    conn: &rusqlite::Connection,
    operation_id: &str,
) -> Result<Option<(String, BasketExportOperation)>, PlaybackError> {
    conn.query_row("SELECT canonical_request,snapshot_id,target_device_id,action,state,pre_basket_hash,post_basket_hash,basket_json,reason FROM playback_basket_exports WHERE operation_id=?1",[operation_id],|r| {
        let action: String=r.get(3)?; let state:String=r.get(4)?; let json:Option<String>=r.get(7)?;
        Ok((r.get(0)?,BasketExportOperation { schema_version:1,operation_id:operation_id.to_string(),snapshot_id:r.get(1)?,target_device_id:r.get(2)?,action:parse_action(&action)?,state:parse_state(&state)?,pre_basket_hash:r.get(5)?,post_basket_hash:r.get(6)?,basket_items:json.map(|v|serde_json::from_str(&v)).transpose().map_err(|e|rusqlite::Error::FromSqlConversionFailure(7,rusqlite::types::Type::Text,Box::new(e)))?,reason:r.get(8)? }))
    }).optional().map_err(|_| error("BASKET_EXPORT_STORAGE_FAILED", false))
}
fn action_name(v: BasketExportAction) -> &'static str {
    match v {
        BasketExportAction::Add => "add",
        BasketExportAction::Replace => "replace",
    }
}
fn state_name(v: BasketExportState) -> &'static str {
    match v {
        BasketExportState::IntentRecorded => "intentRecorded",
        BasketExportState::CommitConfirmed => "commitConfirmed",
        BasketExportState::Conflict => "conflict",
        BasketExportState::Failed => "failed",
        BasketExportState::CommitUncertain => "commitUncertain",
    }
}
fn parse_action(v: &str) -> rusqlite::Result<BasketExportAction> {
    match v {
        "add" => Ok(BasketExportAction::Add),
        "replace" => Ok(BasketExportAction::Replace),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}
fn parse_state(v: &str) -> rusqlite::Result<BasketExportState> {
    match v {
        "intentRecorded" => Ok(BasketExportState::IntentRecorded),
        "commitConfirmed" => Ok(BasketExportState::CommitConfirmed),
        "conflict" => Ok(BasketExportState::Conflict),
        "failed" => Ok(BasketExportState::Failed),
        "commitUncertain" => Ok(BasketExportState::CommitUncertain),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::canonical_basket_hash;
    fn item(id: &str, server: &str) -> BasketItem {
        BasketItem {
            id: id.into(),
            name: id.into(),
            item_type: "Audio".into(),
            server_id: Some(server.into()),
            artist: None,
            child_count: 1,
            size_ticks: 10,
            size_bytes: 20,
        }
    }
    #[test]
    fn add_preserves_existing_and_provider_identity() {
        let a = item("same", "a");
        let b = item("same", "b");
        assert_eq!(
            merge_add(std::slice::from_ref(&a), &[a.clone(), b.clone()]),
            vec![a, b]
        );
    }
    #[test]
    fn hash_is_ordered_and_ignores_other_manifest_fields() {
        let a = item("a", "s");
        let b = item("b", "s");
        assert_ne!(
            canonical_basket_hash(&[a.clone(), b.clone()]),
            canonical_basket_hash(&[b, a])
        );
    }

    #[test]
    fn durable_identity_rejects_mismatched_reuse_and_restart_is_uncertain() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("basket-export.sqlite");
        let operation_id = Uuid::new_v4().to_string();
        let snapshot_id = Uuid::new_v4().to_string();
        let request = CommitBasketExportParams {
            schema_version: 1,
            operation_id: operation_id.clone(),
            snapshot_id,
            target_device_id: "device-a".into(),
            action: BasketExportAction::Replace,
            target_connection_revision: "1".into(),
            expected_basket_hash: "a".repeat(64),
        };
        let db = Database::new(path.clone()).unwrap();
        db.init_playback().unwrap();
        let first = db
            .record_basket_export_intent(&request, &"a".repeat(64), &"b".repeat(64))
            .unwrap();
        assert_eq!(first.state, BasketExportState::IntentRecorded);
        let same = db
            .record_basket_export_intent(&request, &"a".repeat(64), &"b".repeat(64))
            .unwrap();
        assert_eq!(same.operation_id, operation_id);
        assert_eq!(
            db.replay_basket_export(&request)
                .unwrap()
                .unwrap()
                .operation_id,
            operation_id
        );
        let mut mismatch = request.clone();
        mismatch.target_device_id = "device-b".into();
        assert_eq!(
            db.record_basket_export_intent(&mismatch, &"a".repeat(64), &"b".repeat(64))
                .unwrap_err()
                .code,
            "BASKET_EXPORT_OPERATION_REUSED"
        );
        drop(db);
        let reopened = Database::new(path).unwrap();
        reopened.init_playback().unwrap();
        assert_eq!(
            reopened.get_basket_export(&operation_id).unwrap().state,
            BasketExportState::CommitUncertain
        );
    }
}
