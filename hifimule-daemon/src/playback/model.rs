use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_PAGE_SIZE: usize = 100;
pub const MAX_PAGE_SIZE: usize = 200;
pub const MAX_INSERT_BATCH: usize = 200;
pub const MAX_ID_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrackSource {
    pub server_id: String,
    pub track_id: String,
}

impl TrackSource {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.server_id.is_empty() || self.track_id.is_empty() {
            return Err("source identities must not be empty");
        }
        if self.server_id.len() > MAX_ID_BYTES || self.track_id.len() > MAX_ID_BYTES {
            return Err("source identity exceeds 1024 UTF-8 bytes");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub occurrence_id: String,
    pub ordinal: u64,
    pub source: TrackSource,
    pub availability: SourceAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceAvailability {
    Unknown,
    NotConfigured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransportState {
    Idle,
    Paused,
    Buffering,
    Playing,
    Stopping,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    /// Private admission disposition, excluded from the wire contract.
    #[serde(skip)]
    pub(crate) resume_audio: bool,
    /// Owner-captured epoch fencing asynchronous Resume preparation.
    #[serde(skip)]
    pub(crate) resume_epoch: u64,
    /// Private seek effect disposition, excluded from the wire contract.
    #[serde(skip)]
    pub(crate) seek_audio: bool,
    /// Owner-captured epoch fencing asynchronous seek preparation.
    #[serde(skip)]
    pub(crate) seek_epoch: u64,
    pub schema_version: u32,
    pub instance_id: String,
    pub session_id: String,
    pub queue_revision: String,
    pub state_sequence: String,
    pub generation_id: String,
    pub state: TransportState,
    pub current: Option<Occurrence>,
    pub position_ms: u64,
    pub checkpointed_position_ms: u64,
    pub persistence: Status,
    pub restoration: Status,
    pub total_occurrence_count: u64,
    pub occurrences: Vec<Occurrence>,
    pub next_cursor: Option<String>,
    pub playback: PlaybackState,
    pub output: OutputState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputState {
    pub revision: String,
    pub selected: Option<super::devices::OutputDescriptor>,
    pub pending: Option<super::devices::OutputDescriptor>,
    pub active: Option<super::devices::OutputDescriptor>,
    pub status: String,
    pub error: Option<PlaybackFailure>,
}

impl Default for OutputState {
    fn default() -> Self {
        Self {
            revision: "0".into(),
            selected: None,
            pending: None,
            active: None,
            status: "unselected".into(),
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListOutputsParams {
    pub schema_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectOutputParams {
    pub schema_version: u32,
    pub instance_id: String,
    pub session_id: String,
    pub command_id: String,
    pub expected_output_revision: String,
    pub expected_generation_id: String,
    pub output_id: String,
    #[serde(default)]
    pub replace_invalid_config: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputList {
    pub instance_id: String,
    pub output_revision: String,
    pub outputs: Vec<super::devices::OutputDescriptor>,
    pub output: OutputState,
    pub error: Option<PlaybackFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackStatus {
    Idle,
    Loading,
    Active,
    Paused,
    Stopped,
    Completed,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackFailure {
    pub code: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackTrackMetadata {
    pub source: TrackSource,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackState {
    pub status: PlaybackStatus,
    pub can_go_next: bool,
    pub metadata: Option<PlaybackTrackMetadata>,
    pub duration_ms: Option<u64>,
    pub representation: Option<String>,
    pub seek: SeekCapability,
    pub pending_seek: Option<PendingSeek>,
    pub seek_outcome: Option<SeekOutcome>,
    pub error: Option<PlaybackFailure>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Idle,
            can_go_next: false,
            metadata: None,
            duration_ms: None,
            representation: None,
            seek: SeekCapability::unavailable("seek.unresolved"),
            pending_seek: None,
            seek_outcome: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeekCapability {
    pub available: bool,
    pub reason: Option<String>,
    pub mechanism: Option<String>,
    pub decoded_landing_tolerance_ms: Option<u64>,
}

impl SeekCapability {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            available: false,
            reason: Some(reason.into()),
            mechanism: None,
            decoded_landing_tolerance_ms: None,
        }
    }

    pub fn jellyfin_pcm_wav() -> Self {
        Self {
            available: true,
            reason: None,
            mechanism: Some("ffmpeg-post-open-media-time-seek".into()),
            decoded_landing_tolerance_ms: Some(50),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingSeek {
    pub operation_id: String,
    pub requested_position_ms: u64,
    pub prior_committed_position_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeekOutcome {
    pub operation_id: String,
    pub requested_position_ms: u64,
    pub actual_position_ms: Option<u64>,
    pub status: String,
    pub error: Option<PlaybackFailure>,
}

#[derive(Debug, Clone)]
pub enum PlaybackEvent {
    Resolved {
        metadata: PlaybackTrackMetadata,
        duration_ms: Option<u64>,
        representation: String,
        seek: SeekCapability,
    },
    SeekQualified {
        capability: SeekCapability,
        duration_ms: Option<u64>,
    },
    Active,
    Buffering,
    Completed {
        position_ms: u64,
    },
    SeekCommitted {
        operation_id: String,
        requested_position_ms: u64,
        actual_position_ms: u64,
    },
    SeekFailed {
        operation_id: String,
        code: String,
        retryable: bool,
    },
    // The owner distinguishes preparation failure from a later pipeline failure.
    SeekPipelineFailed {
        operation_id: String,
        code: String,
        retryable: bool,
    },
    Failed {
        code: String,
        retryable: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplySessionParams {
    pub schema_version: u32,
    pub instance_id: String,
    pub session_id: String,
    pub command_id: String,
    pub expected_queue_revision: String,
    pub operation: SessionOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlbumSource {
    pub server_id: String,
    pub album_id: String,
}

impl AlbumSource {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.server_id.is_empty() || self.album_id.is_empty() {
            return Err("album source identities must not be empty");
        }
        if self.server_id.len() > MAX_ID_BYTES || self.album_id.len() > MAX_ID_BYTES {
            return Err("album source identity exceeds 1024 UTF-8 bytes");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayAlbumParams {
    pub schema_version: u32,
    pub instance_id: String,
    pub session_id: String,
    pub command_id: String,
    pub expected_queue_revision: String,
    pub expected_generation_id: String,
    pub source: AlbumSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SessionOperation {
    ReplaceQueue {
        sources: Vec<TrackSource>,
    },
    AppendQueue {
        sources: Vec<TrackSource>,
    },
    SelectCurrent {
        occurrence_id: String,
    },
    Clear,
    PlayTrack {
        source: TrackSource,
    },
    #[serde(skip_deserializing)]
    PlayAlbum {
        sources: Vec<TrackSource>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControlParams {
    pub schema_version: u32,
    pub instance_id: String,
    pub session_id: String,
    pub command_id: String,
    pub expected_generation_id: String,
    pub occurrence_id: String,
    pub action: ControlAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeekParams {
    pub schema_version: u32,
    pub instance_id: String,
    pub session_id: String,
    pub command_id: String,
    pub expected_generation_id: String,
    pub occurrence_id: String,
    pub position_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControlAction {
    Pause,
    Resume,
    Stop,
    Next,
    Retry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListOccurrencesParams {
    pub schema_version: u32,
    pub session_id: String,
    pub expected_queue_revision: String,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OccurrencePage {
    pub occurrences: Vec<Occurrence>,
    pub next_cursor: Option<String>,
    pub total_occurrence_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    #[serde(skip)]
    pub(crate) start_audio: bool,
    pub session_id: String,
    pub queue_revision: String,
    pub state_sequence: String,
    pub generation_id: String,
    pub assigned_occurrences: Vec<Occurrence>,
    pub current_metadata: SessionMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadata {
    pub instance_id: String,
    pub session_id: String,
    pub queue_revision: String,
    pub state_sequence: String,
    pub generation_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackHealth {
    pub restoration: Status,
    pub persistence: Status,
}

#[derive(Debug, Clone)]
pub struct PersistedSession {
    pub session_id: String,
    pub queue_revision: u64,
    pub checkpoint_sequence: u64,
    pub state: TransportState,
    pub current_occurrence_id: Option<String>,
    pub position_ms: u64,
}

#[cfg(test)]
mod seek_contract_tests {
    use super::*;

    fn request(position: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "schemaVersion": 1,
            "instanceId": "instance",
            "sessionId": "session",
            "commandId": "12345678-1234-1234-1234-123456789abc",
            "expectedGenerationId": "generation",
            "occurrenceId": "occurrence",
            "positionMs": position
        })
    }

    #[test]
    fn strict_seek_wire_rejects_fraction_negative_and_unknown_fields() {
        assert!(serde_json::from_value::<SeekParams>(request(serde_json::json!(1.5))).is_err());
        assert!(serde_json::from_value::<SeekParams>(request(serde_json::json!(-1))).is_err());
        let mut unknown = request(serde_json::json!(0));
        unknown
            .as_object_mut()
            .unwrap()
            .insert("target".into(), serde_json::json!(0));
        assert!(serde_json::from_value::<SeekParams>(unknown).is_err());
    }

    #[test]
    fn strict_seek_wire_accepts_zero_and_safe_integer_limit() {
        assert_eq!(
            serde_json::from_value::<SeekParams>(request(serde_json::json!(0)))
                .unwrap()
                .position_ms,
            0
        );
        assert_eq!(
            serde_json::from_value::<SeekParams>(request(serde_json::json!(
                9_007_199_254_740_991u64
            )))
            .unwrap()
            .position_ms,
            9_007_199_254_740_991
        );
    }

    #[test]
    fn strict_album_wire_requires_portable_source_and_rejects_unknown_fields() {
        let request = serde_json::json!({
            "schemaVersion": 1, "instanceId": "instance", "sessionId": "session",
            "commandId": "12345678-1234-1234-1234-123456789abc",
            "expectedQueueRevision": "0", "expectedGenerationId": "generation",
            "source": { "serverId": "portable-server", "albumId": "album" }
        });
        let parsed = serde_json::from_value::<PlayAlbumParams>(request.clone()).unwrap();
        assert!(parsed.source.validate().is_ok());
        let mut unknown = request;
        unknown
            .as_object_mut()
            .unwrap()
            .insert("tracks".into(), serde_json::json!([]));
        assert!(serde_json::from_value::<PlayAlbumParams>(unknown).is_err());
        assert!(
            AlbumSource {
                server_id: String::new(),
                album_id: "album".into()
            }
            .validate()
            .is_err()
        );
    }
}
