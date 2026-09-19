use super::{NativeControlIntent, PlaybackSession};
use crate::{db::Database, server_manager::ServerManager, sync::SyncOperationManager};
use std::sync::Arc;

const PREPARATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Shared transport service for RPC, native controls, and daemon menu actions.
/// Owner admission remains synchronous and ordered; potentially slow source
/// preparation is detached and generation-fenced so later Pause/Stop can win.
#[derive(Clone)]
pub struct PlaybackCommandService {
    playback: PlaybackSession,
    server_manager: Arc<tokio::sync::RwLock<ServerManager>>,
    db: Arc<Database>,
    operations: Arc<SyncOperationManager>,
}

impl PlaybackCommandService {
    pub fn new(
        playback: PlaybackSession,
        server_manager: Arc<tokio::sync::RwLock<ServerManager>>,
        db: Arc<Database>,
        operations: Arc<SyncOperationManager>,
    ) -> Self {
        Self {
            playback,
            server_manager,
            db,
            operations,
        }
    }

    pub async fn rpc_control(
        &self,
        params: super::model::ControlParams,
        guard: Option<crate::sync::MutationGuard>,
    ) -> Result<super::model::SessionSnapshot, super::session::PlaybackError> {
        let playback = self.playback.clone();
        let snapshot =
            tokio::task::spawn_blocking(move || playback.control_with_guard(params, guard))
                .await
                .map_err(|_| task_failed())??;
        self.dispatch_effect(&snapshot);
        Ok(snapshot)
    }

    pub async fn rpc_preview(
        &self,
        params: super::model::PreviewTrackParams,
        guard: Option<crate::sync::MutationGuard>,
    ) -> Result<super::model::SessionSnapshot, super::session::PlaybackError> {
        let playback = self.playback.clone();
        let snapshot =
            tokio::task::spawn_blocking(move || playback.preview_with_guard(params, guard))
                .await
                .map_err(|_| task_failed())??;
        self.dispatch_effect(&snapshot);
        Ok(snapshot)
    }

    pub async fn native_control(
        &self,
        intent: NativeControlIntent,
    ) -> Result<super::model::SessionSnapshot, super::session::PlaybackError> {
        let Some(guard) = self.operations.try_admit_mutation() else {
            return Err(stopped());
        };
        let playback = self.playback.clone();
        let snapshot =
            tokio::task::spawn_blocking(move || playback.native_control(intent, Some(guard)))
                .await
                .map_err(|_| task_failed())??;
        self.dispatch_effect(&snapshot);
        Ok(snapshot)
    }

    pub async fn rpc_seek(
        &self,
        params: super::model::SeekParams,
        guard: Option<crate::sync::MutationGuard>,
    ) -> Result<super::model::SessionSnapshot, super::session::PlaybackError> {
        let playback = self.playback.clone();
        let snapshot = tokio::task::spawn_blocking(move || playback.seek_with_guard(params, guard))
            .await
            .map_err(|_| task_failed())??;
        self.dispatch_effect(&snapshot);
        Ok(snapshot)
    }

    pub fn playback(&self) -> &PlaybackSession {
        &self.playback
    }

    /// Run commit and dispatch as one blocking job. Dropping the RPC future
    /// cannot strand an already committed queue before its audio effect runs.
    #[cfg(test)]
    pub(crate) fn commit_album(
        &self,
        reservation: super::session::AlbumReservation,
        sources: Vec<super::model::TrackSource>,
    ) -> Result<super::model::SessionSnapshot, super::session::PlaybackError> {
        let snapshot = self.playback.commit_album(reservation, sources)?;
        self.dispatch_effect(&snapshot);
        Ok(snapshot)
    }

    pub(crate) fn commit_album_with_policy(
        &self,
        reservation: super::session::AlbumReservation,
        sources: Vec<super::model::TrackSource>,
        policy: super::loudness::AlbumLoudnessPolicy,
        representations: Vec<String>,
    ) -> Result<super::model::SessionSnapshot, super::session::PlaybackError> {
        let snapshot = self.playback.commit_album_with_policy(
            reservation,
            sources,
            policy,
            representations,
        )?;
        self.dispatch_effect(&snapshot);
        Ok(snapshot)
    }

    fn dispatch_effect(&self, snapshot: &super::model::SessionSnapshot) {
        if snapshot.back_audio {
            self.dispatch_back(snapshot);
            return;
        }
        if snapshot.seek_audio {
            self.dispatch_seek(snapshot);
            return;
        }
        if !snapshot.resume_audio || self.playback.control_epoch() != snapshot.resume_epoch {
            return;
        }
        if super::audio::global().resume_existing(
            &snapshot.generation_id,
            &self.playback,
            snapshot.resume_epoch,
        ) {
            return;
        }
        let Some(current) = snapshot.current.clone() else {
            return;
        };
        let manager = self.server_manager.clone();
        let db = self.db.clone();
        let playback = self.playback.clone();
        let generation = snapshot.generation_id.clone();
        let position_ms = snapshot.position_ms;
        let resume_epoch = snapshot.resume_epoch;
        let gain = f32::from_bits(snapshot.gain_bits);
        let admitted_suffix = snapshot.qualified_suffix.clone();
        tokio::spawn(async move {
            let deadline = std::time::Instant::now() + PREPARATION_TIMEOUT;
            let resolve = async {
                let provider = crate::server_manager::get_provider_by_server_id(
                    &manager,
                    &db,
                    &current.source.server_id,
                )
                .await?;
                provider.resolve_playback(&current.source.track_id).await
            };
            let resolved = tokio::select! {
                result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), resolve) => Some(result),
                _ = async {
                    while playback.generation_guard(&generation).is_some()
                        && playback.control_epoch() == resume_epoch
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    }
                } => None,
            };
            if playback.control_epoch() != resume_epoch {
                return;
            }
            let failure = match resolved {
                Some(Ok(Ok(description))) => super::audio::global()
                    .start_at_epoch_with_gain(
                        description,
                        current.source,
                        position_ms,
                        generation.clone(),
                        playback.clone(),
                        deadline,
                        resume_epoch,
                        gain,
                        admitted_suffix,
                    )
                    .await
                    .err(),
                Some(Ok(Err(error))) => Some(
                    super::audio::PlaybackPipelineError::from_provider_error(error),
                ),
                Some(Err(_)) => {
                    playback.publish_event_at_epoch(
                        generation,
                        super::model::PlaybackEvent::Failed {
                            code: "RESUME_UNAVAILABLE".into(),
                            retryable: true,
                        },
                        resume_epoch,
                    );
                    return;
                }
                None => return,
            };
            if let Some(error) = failure {
                if super::audio::log_pipeline_failure(&playback, &generation, &error) {
                    playback.publish_event_at_epoch(
                        generation,
                        super::model::PlaybackEvent::Failed {
                            code: if error.code().starts_with("OUTPUT_") {
                                error.code()
                            } else {
                                "RESUME_UNAVAILABLE"
                            }
                            .into(),
                            retryable: true,
                        },
                        resume_epoch,
                    );
                }
                return;
            }

            spawn_successor_preparation(playback, manager, db, generation);
        });
    }

    fn dispatch_back(&self, snapshot: &super::model::SessionSnapshot) {
        if self.playback.control_epoch() != snapshot.back_epoch {
            return;
        }
        let (Some(current), Some(pending)) = (
            snapshot.current.clone(),
            snapshot.playback.pending_back.clone(),
        ) else {
            return;
        };
        let manager = self.server_manager.clone();
        let db = self.db.clone();
        let playback = self.playback.clone();
        let generation = snapshot.generation_id.clone();
        let back_epoch = snapshot.back_epoch;
        let back_audible = snapshot.back_audible;
        let gain = f32::from_bits(snapshot.gain_bits);
        let admitted_suffix = snapshot.qualified_suffix.clone();
        tokio::spawn(async move {
            let deadline = std::time::Instant::now() + PREPARATION_TIMEOUT;
            let resolve = async {
                let provider = crate::server_manager::get_provider_by_server_id(
                    &manager,
                    &db,
                    &current.source.server_id,
                )
                .await?;
                provider.resolve_playback(&current.source.track_id).await
            };
            let resolved = tokio::select! {
                result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), resolve) => Some(result),
                _ = async {
                    while playback.generation_guard(&generation).is_some()
                        && playback.control_epoch() == back_epoch
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    }
                } => None,
            };
            if playback.control_epoch() != back_epoch {
                return;
            }
            let failure = match resolved {
                Some(Ok(Ok(description))) => super::audio::global()
                    .start_back_at_epoch_with_gain(
                        description,
                        current.source,
                        pending.operation_id.clone(),
                        generation.clone(),
                        playback.clone(),
                        deadline,
                        back_epoch,
                        gain,
                        admitted_suffix,
                    )
                    .await
                    .err(),
                Some(Ok(Err(error))) => Some(
                    super::audio::PlaybackPipelineError::from_provider_error(error),
                ),
                Some(Err(_)) => Some(super::audio::PlaybackPipelineError::seek_timeout()),
                None => return,
            };
            if let Some(error) = failure {
                let code = if error.code().starts_with("OUTPUT_") {
                    error.code()
                } else {
                    "BACK_SOURCE_UNAVAILABLE"
                };
                playback.publish_event_at_epoch(
                    generation,
                    super::model::PlaybackEvent::BackFailed {
                        operation_id: pending.operation_id,
                        code: code.into(),
                        retryable: true,
                    },
                    back_epoch,
                );
                return;
            }
            if !authorize_prepared_back(
                &playback,
                &generation,
                back_epoch,
                &pending.operation_id,
                back_audible,
                || super::audio::global().resume_existing(&generation, &playback, back_epoch),
            ) {
                return;
            }
            spawn_successor_preparation(playback, manager, db, generation);
        });
    }

    fn dispatch_seek(&self, snapshot: &super::model::SessionSnapshot) {
        if self.playback.control_epoch() != snapshot.seek_epoch {
            return;
        }
        let (Some(current), Some(pending)) = (
            snapshot.current.clone(),
            snapshot.playback.pending_seek.clone(),
        ) else {
            return;
        };
        let manager = self.server_manager.clone();
        let db = self.db.clone();
        let playback = self.playback.clone();
        let generation = snapshot.generation_id.clone();
        let seek_epoch = snapshot.seek_epoch;
        let gain = f32::from_bits(snapshot.gain_bits);
        let admitted_suffix = snapshot.qualified_suffix.clone();
        tokio::spawn(async move {
            let deadline = std::time::Instant::now() + PREPARATION_TIMEOUT;
            let resolve = async {
                let provider = crate::server_manager::get_provider_by_server_id(
                    &manager,
                    &db,
                    &current.source.server_id,
                )
                .await?;
                provider.resolve_playback(&current.source.track_id).await
            };
            let resolved = tokio::select! {
                result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), resolve) => Some(result),
                _ = async {
                    while playback.generation_guard(&generation).is_some()
                        && playback.control_epoch() == seek_epoch
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    }
                } => None,
            };
            if playback.control_epoch() != seek_epoch {
                return;
            }
            let failure = match resolved {
                Some(Ok(Ok(description))) => super::audio::global()
                    .start_seek_at_epoch_with_gain(
                        description,
                        current.source,
                        pending.requested_position_ms,
                        pending.operation_id.clone(),
                        generation.clone(),
                        playback.clone(),
                        deadline,
                        seek_epoch,
                        gain,
                        admitted_suffix,
                    )
                    .await
                    .err(),
                Some(Ok(Err(error))) => Some(
                    super::audio::PlaybackPipelineError::from_provider_error(error),
                ),
                Some(Err(_)) => Some(super::audio::PlaybackPipelineError::seek_timeout()),
                None => return,
            };
            if failure.is_none() {
                spawn_successor_preparation(playback.clone(), manager, db, generation.clone());
            }
            if let Some(error) = failure
                && super::audio::log_pipeline_failure(&playback, &generation, &error)
            {
                playback.publish_event_at_epoch(
                    generation,
                    super::model::PlaybackEvent::SeekFailed {
                        operation_id: pending.operation_id,
                        code: "SEEK_FAILED".into(),
                        retryable: true,
                    },
                    seek_epoch,
                );
            }
        });
    }
}

pub(crate) fn authorize_prepared_back(
    playback: &PlaybackSession,
    generation: &str,
    back_epoch: u64,
    operation_id: &str,
    audible: bool,
    resume: impl FnOnce() -> bool,
) -> bool {
    if audible && !resume() {
        playback.publish_event_at_epoch(
            generation.to_owned(),
            super::model::PlaybackEvent::BackFailed {
                operation_id: operation_id.to_owned(),
                code: "BACK_SOURCE_UNAVAILABLE".into(),
                retryable: true,
            },
            back_epoch,
        );
        return false;
    }
    true
}

/// One sequential preparer per installed output pipeline. Capture the fence
/// before the owner candidate, cancel pending provider/HTTP work on revocation,
/// and do not allocate another source until the worker releases its old slot.
pub(crate) fn spawn_successor_preparation(
    playback: PlaybackSession,
    manager: Arc<tokio::sync::RwLock<ServerManager>>,
    db: Arc<Database>,
    generation: String,
) {
    let Some(output_epoch) = super::audio::global().start_successor_coordinator(&generation) else {
        return;
    };
    tokio::spawn(async move {
        let mut attempted = None;
        while super::audio::global().has_active_generation(&generation) {
            if let Some(ticket) = super::audio::global().successor_ticket(&generation) {
                if ticket.output_epoch != output_epoch {
                    break;
                }
                let control_epoch = playback.control_epoch();
                let key = (ticket.epoch, control_epoch);
                if attempted != Some(key) {
                    if let Some(candidate) =
                        playback.successor_candidate(&generation, control_epoch)
                    {
                        attempted = Some(key);
                        let deadline = std::time::Instant::now() + PREPARATION_TIMEOUT;
                        let resolve = async {
                            let provider = crate::server_manager::get_provider_by_server_id(
                                &manager,
                                &db,
                                &candidate.successor.source.server_id,
                            )
                            .await?;
                            provider
                                .resolve_playback(&candidate.successor.source.track_id)
                                .await
                        };
                        let resolved = tokio::select! {
                            result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), resolve) => Some(result),
                            _ = ticket.cancelled() => None,
                        };
                        if let Some(Ok(Ok(description))) = resolved {
                            let _ = super::audio::global()
                                .prepare_successor(
                                    candidate,
                                    &ticket,
                                    description,
                                    generation.clone(),
                                    deadline,
                                )
                                .await;
                        }
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    });
}

fn task_failed() -> super::session::PlaybackError {
    super::session::PlaybackError {
        code: "PLAYBACK_BUSY",
        message: "playback command task failed",
        conflict: true,
        authoritative: None,
    }
}

fn stopped() -> super::session::PlaybackError {
    super::session::PlaybackError {
        code: "DAEMON_STOPPED",
        message: "daemon shutdown rejected playback work",
        conflict: true,
        authoritative: None,
    }
}

#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;
