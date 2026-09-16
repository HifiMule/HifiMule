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
    pub metadata: Option<PlaybackTrackMetadata>,
    pub duration_ms: Option<u64>,
    pub representation: Option<String>,
    pub error: Option<PlaybackFailure>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Idle,
            metadata: None,
            duration_ms: None,
            representation: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PlaybackEvent {
    Resolved {
        metadata: PlaybackTrackMetadata,
        duration_ms: Option<u64>,
        representation: String,
    },
    Active,
    Buffering,
    Completed {
        position_ms: u64,
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
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SessionOperation {
    ReplaceQueue { sources: Vec<TrackSource> },
    AppendQueue { sources: Vec<TrackSource> },
    SelectCurrent { occurrence_id: String },
    Clear,
    PlayTrack { source: TrackSource },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ControlAction {
    Pause,
    Resume,
    Stop,
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
