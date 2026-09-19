use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_PAGE_SIZE: usize = 100;
pub const MAX_PAGE_SIZE: usize = 200;
pub const MAX_INSERT_BATCH: usize = 200;
pub const MAX_REMOVE_BATCH: usize = 200;
pub const MAX_MANUAL_ACTIVE_OCCURRENCES: usize = 10_000;
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
    /// Private Back preparation disposition, excluded from the wire contract.
    #[serde(skip)]
    pub(crate) back_audio: bool,
    /// Owner-captured epoch fencing asynchronous Back preparation.
    #[serde(skip)]
    pub(crate) back_epoch: u64,
    /// Whether a completed Back preparation may become audible immediately.
    #[serde(skip)]
    pub(crate) back_audible: bool,
    /// Frozen occurrence gain, excluded from the public playback wire contract.
    #[serde(skip)]
    pub(crate) gain_bits: u32,
    #[serde(skip)]
    pub(crate) qualified_suffix: Option<String>,
    pub schema_version: u32,
    pub instance_id: String,
    pub session_id: String,
    pub queue_revision: String,
    pub state_sequence: String,
    pub generation_id: String,
    pub mode: PlaybackMode,
    pub queue_kind: QueueKind,
    pub preview: Option<PreviewSummary>,
    pub state: TransportState,
    pub current: Option<Occurrence>,
    pub main_current: Option<Occurrence>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackMode {
    Main,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QueueKind {
    Album,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSummary {
    pub audition_id: String,
    pub has_main_session: bool,
    pub saved_main_occurrence_id: Option<String>,
    pub saved_main_position_ms: u64,
    pub saved_main_intent: TransportState,
    pub resume_inhibited: bool,
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
    pub can_go_back: bool,
    pub back_unavailable_reason: Option<String>,
    pub metadata: Option<PlaybackTrackMetadata>,
    pub duration_ms: Option<u64>,
    pub representation: Option<String>,
    pub seek: SeekCapability,
    pub pending_seek: Option<PendingSeek>,
    pub seek_outcome: Option<SeekOutcome>,
    pub pending_back: Option<PendingBack>,
    pub back_outcome: Option<BackOutcome>,
    pub error: Option<PlaybackFailure>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Idle,
            can_go_next: false,
            can_go_back: false,
            back_unavailable_reason: Some("back.empty".into()),
            metadata: None,
            duration_ms: None,
            representation: None,
            seek: SeekCapability::unavailable("seek.unresolved"),
            pending_seek: None,
            seek_outcome: None,
            pending_back: None,
            back_outcome: None,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingBack {
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackOutcome {
    pub operation_id: String,
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
    HandoffPresented {
        token: super::continuity::HandoffToken,
        metadata: PlaybackTrackMetadata,
        duration_ms: u64,
        representation: String,
        predecessor_position_ms: u64,
        successor_offset_frames: u64,
        sample_rate: u32,
        seek: SeekCapability,
    },
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
    BackCommitted {
        operation_id: String,
    },
    BackFailed {
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewTrackParams {
    pub schema_version: u32,
    pub instance_id: String,
    pub session_id: String,
    pub command_id: String,
    pub expected_queue_revision: String,
    pub expected_generation_id: String,
    pub source: TrackSource,
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
    RemoveUpcoming {
        occurrence_ids: Vec<String>,
    },
    MoveUpcoming {
        occurrence_id: String,
        before_occurrence_id: Option<String>,
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
    Back,
    Pause,
    Resume,
    Stop,
    Next,
    Retry,
    ReturnToSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListOccurrencesParams {
    pub schema_version: u32,
    pub session_id: String,
    pub expected_queue_revision: String,
    #[serde(default)]
    pub section: OccurrenceSection,
    pub cursor: Option<String>,
    pub around_occurrence_id: Option<String>,
    pub expected_main_occurrence_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OccurrenceSection {
    #[default]
    All,
    Upcoming,
    History,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OccurrencePage {
    pub occurrences: Vec<Occurrence>,
    pub next_cursor: Option<String>,
    pub total_occurrence_count: u64,
    pub section: OccurrenceSection,
    pub section_count: u64,
    pub main_current_occurrence_id: Option<String>,
    pub preceding_occurrence_id: Option<String>,
    pub following_occurrence_ids: Vec<String>,
    pub end_of_section: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DescribeOccurrencesParams {
    pub schema_version: u32,
    pub session_id: String,
    pub expected_queue_revision: String,
    pub occurrence_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OccurrenceDisplay {
    pub occurrence_id: String,
    pub source: TrackSource,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub status: OccurrenceDisplayStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OccurrenceDisplayStatus {
    Available,
    SourceUnavailable,
    TrackUnavailable,
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
    pub queue_kind: QueueKind,
    pub(crate) current_gain_bits: u32,
    pub(crate) current_qualified_suffix: Option<String>,
    pub(crate) album_context: Option<FrozenAlbumContext>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedAudition {
    pub audition_id: String,
    pub parent_session_id: String,
    pub source: TrackSource,
    pub position_ms: u64,
    pub state: TransportState,
    pub saved_main_occurrence_id: Option<String>,
    pub saved_main_position_ms: u64,
    pub saved_main_intent: TransportState,
    pub resume_inhibited: bool,
    pub contiguous_heard_ms: u64,
    pub coverage_unknown: bool,
    pub seek_discontinuous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditionOutcome {
    pub outcome_id: u64,
    pub audition_id: String,
    pub parent_session_id: String,
    pub source: TrackSource,
    pub disposition: String,
    pub terminal_position_ms: u64,
    pub duration_ms: Option<u64>,
    pub failure_code: Option<String>,
    pub contiguous_heard_ms: u64,
    pub coverage_unknown: bool,
    pub seek_discontinuous: bool,
    pub fully_heard: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackAttempt {
    pub attempt_seq: u64,
    pub attempt_id: String,
    pub session_id: String,
    pub occurrence_id: String,
    pub source: TrackSource,
    pub disposition: Option<String>,
    pub failure_code: Option<String>,
    pub terminal_position_ms: Option<u64>,
    pub legacy: bool,
}

// Only legacy v3 migration creates this marker. It never qualifies playback:
// the preparation check requires an exact supported suffix match.
pub(crate) const UNVERIFIED_ALBUM_FORMAT: &str = "unverified";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FrozenAlbumContext {
    pub source: AlbumSource,
    pub member_count: u64,
    pub membership_digest: String,
    #[serde(default)]
    pub representations: Vec<String>,
    pub policy: super::loudness::AlbumLoudnessPolicy,
}

impl FrozenAlbumContext {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        self.source.validate()?;
        if self.member_count == 0 || self.member_count > super::album::MAX_ALBUM_OCCURRENCES as u64
        {
            return Err("invalid album membership count");
        }
        if self.membership_digest.len() != 64
            || !self
                .membership_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("invalid album membership digest");
        }
        if (self.policy.scalar() != 1.0 && self.representations.len() != self.member_count as usize)
            || (self.policy.scalar() == 1.0 && !self.representations.is_empty())
            || self.representations.iter().any(|suffix| {
                suffix != UNVERIFIED_ALBUM_FORMAT
                    && !matches!(suffix.as_str(), "wav" | "flac" | "m4a" | "mp3")
            })
        {
            return Err("missing or invalid admitted representations");
        }
        self.policy.validate()
    }

    pub(crate) fn suffix_for(&self, occurrence: &Occurrence) -> Option<String> {
        if occurrence.source.server_id == self.source.server_id
            && occurrence.ordinal < self.member_count
        {
            self.representations
                .get(occurrence.ordinal as usize)
                .cloned()
        } else {
            None
        }
    }

    pub(crate) fn scalar_for(&self, occurrence: &Occurrence) -> f32 {
        if occurrence.ordinal < self.member_count
            && occurrence.source.server_id == self.source.server_id
        {
            self.policy.scalar()
        } else {
            1.0
        }
    }
}

pub(crate) fn album_membership_hasher() -> blake3::Hasher {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"hifimule-album-membership-v1\0");
    hasher
}

pub(crate) fn hash_album_member(hasher: &mut blake3::Hasher, source: &TrackSource) {
    for value in [&source.server_id, &source.track_id] {
        hasher.update(&(value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
}

pub(crate) fn album_membership_digest<'a>(
    sources: impl IntoIterator<Item = &'a TrackSource>,
) -> String {
    let mut hasher = album_membership_hasher();
    for source in sources {
        hash_album_member(&mut hasher, source);
    }
    hasher.finalize().to_hex().to_string()
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

    fn preview_request() -> serde_json::Value {
        serde_json::json!({
            "schemaVersion": 1,
            "instanceId": "instance",
            "sessionId": "session",
            "commandId": "12345678-1234-1234-1234-123456789abc",
            "expectedQueueRevision": "7",
            "expectedGenerationId": "generation",
            "source": { "serverId": "portable-server", "trackId": "track" }
        })
    }

    #[test]
    fn strict_preview_wire_requires_portable_source_and_rejects_unknown_fields() {
        let parsed = serde_json::from_value::<PreviewTrackParams>(preview_request()).unwrap();
        assert!(parsed.source.validate().is_ok());
        assert_eq!(parsed.expected_queue_revision, "7");
        assert_eq!(parsed.expected_generation_id, "generation");

        let mut unknown = preview_request();
        unknown.as_object_mut().unwrap().insert(
            "url".into(),
            serde_json::json!("https://secret.invalid/audio"),
        );
        assert!(serde_json::from_value::<PreviewTrackParams>(unknown).is_err());

        let mut nested_unknown = preview_request();
        nested_unknown["source"]["albumId"] = serde_json::json!("album");
        assert!(serde_json::from_value::<PreviewTrackParams>(nested_unknown).is_err());
    }

    #[test]
    fn preview_mode_and_return_action_use_additive_camel_case_wire_values() {
        assert_eq!(
            serde_json::to_value(PlaybackMode::Preview).unwrap(),
            "preview"
        );
        assert_eq!(
            serde_json::to_value(ControlAction::ReturnToSession).unwrap(),
            "returnToSession"
        );
        assert_eq!(serde_json::to_value(ControlAction::Back).unwrap(), "back");
    }

    #[test]
    fn queue_edit_wire_is_occurrence_based_and_strict() {
        let remove = serde_json::json!({
            "type": "removeUpcoming",
            "occurrenceIds": ["first", "second"]
        });
        assert_eq!(
            serde_json::from_value::<SessionOperation>(remove).unwrap(),
            SessionOperation::RemoveUpcoming {
                occurrence_ids: vec!["first".into(), "second".into()]
            }
        );

        let move_to_end = serde_json::json!({
            "type": "moveUpcoming",
            "occurrenceId": "first",
            "beforeOccurrenceId": null
        });
        assert_eq!(
            serde_json::from_value::<SessionOperation>(move_to_end).unwrap(),
            SessionOperation::MoveUpcoming {
                occurrence_id: "first".into(),
                before_occurrence_id: None
            }
        );

        let unknown = serde_json::json!({
            "type": "removeUpcoming",
            "occurrenceIds": ["first"],
            "sourceId": "must-not-be-accepted"
        });
        assert!(serde_json::from_value::<SessionOperation>(unknown).is_err());
    }

    #[test]
    fn scoped_occurrence_read_contract_is_bounded_and_explicit() {
        let parsed = serde_json::from_value::<ListOccurrencesParams>(serde_json::json!({
            "schemaVersion": 1,
            "sessionId": "session",
            "expectedQueueRevision": "9",
            "section": "upcoming",
            "expectedMainOccurrenceId": "current",
            "aroundOccurrenceId": "target",
            "limit": 200
        }))
        .unwrap();
        assert_eq!(parsed.section, OccurrenceSection::Upcoming);
        assert_eq!(
            parsed.expected_main_occurrence_id.as_deref(),
            Some("current")
        );
        assert_eq!(parsed.around_occurrence_id.as_deref(), Some("target"));
        assert_eq!(MAX_MANUAL_ACTIVE_OCCURRENCES, 10_000);
        assert_eq!(MAX_REMOVE_BATCH, 200);
    }
}
