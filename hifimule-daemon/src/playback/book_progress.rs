//! Player-private, verified whole-book timing. No upstream identity reaches RPC.

use crate::domain::models::ProviderIdentity;
use crate::playback::model::{PlaybackMode, PlaybackStatus, SessionSnapshot, TransportState};
use crate::providers::BookProgress;
use crate::providers::BookTiming;
use std::collections::HashSet;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BookOccurrenceRecord {
    pub session_id: String,
    pub occurrence_id: String,
    pub server_id: String,
    pub track_id: String,
    pub identity: ProviderIdentity,
    pub audio_file_id: String,
    pub part_offset_ms: u64,
    pub duration_ms: u64,
    pub whole_ms: u64,
    pub mapping_valid: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BookReportSample {
    pub main: bool,
    pub playing: bool,
    pub active: bool,
    pub qualified: bool,
    pub pending_seek: bool,
    pub position_ms: u64,
}

impl BookReportSample {
    pub fn progress(self, record: &BookOccurrenceRecord) -> Option<BookProgress> {
        if !self.main
            || !self.playing
            || !self.active
            || !self.qualified
            || self.pending_seek
            || !record.mapping_valid
        {
            return None;
        }
        let whole_ms = record.part_offset_ms.checked_add(self.position_ms)?;
        if whole_ms > record.duration_ms {
            return None;
        }
        Some(BookProgress {
            current_ms: whole_ms,
            duration_ms: record.duration_ms,
            is_finished: false,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BookPart {
    pub track_id: String,
    pub file_id: String,
    pub duration_ms: u64,
}

impl BookPart {
    pub fn new(track_id: &str, file_id: &str, duration_ms: u64) -> Self {
        Self {
            track_id: track_id.into(),
            file_id: file_id.into(),
            duration_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BookMap {
    parts: Vec<BookPart>,
    starts: Vec<u64>,
    duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BookResumeDecision {
    Beginning,
    At { part_index: usize, local_ms: u64 },
    Unavailable,
}

pub(crate) fn decide_resume(
    timing: &BookTiming,
    remote: Option<BookProgress>,
) -> BookResumeDecision {
    let Some(map) = BookMap::new(
        timing
            .parts
            .iter()
            .map(|part| BookPart::new(&part.track_id, &part.audio_file_id, part.duration_ms))
            .collect(),
    ) else {
        return BookResumeDecision::Unavailable;
    };
    let Some(progress) = remote else {
        return BookResumeDecision::Beginning;
    };
    if progress.duration_ms.abs_diff(map.duration_ms()) > 1_000
        || progress.current_ms > map.duration_ms()
    {
        return BookResumeDecision::Unavailable;
    }
    if progress.is_finished {
        return BookResumeDecision::Beginning;
    }
    if progress.current_ms >= map.duration_ms() {
        return BookResumeDecision::Unavailable;
    }
    let Some((track_id, local_ms)) = map.at(progress.current_ms) else {
        return BookResumeDecision::Unavailable;
    };
    let Some(part_index) = timing
        .parts
        .iter()
        .position(|part| part.track_id == track_id)
    else {
        return BookResumeDecision::Unavailable;
    };
    BookResumeDecision::At {
        part_index,
        local_ms,
    }
}

impl BookMap {
    pub fn new(parts: Vec<BookPart>) -> Option<Self> {
        if parts.is_empty() {
            return None;
        }
        let mut seen = HashSet::new();
        let mut seen_track = HashSet::new();
        let mut starts = Vec::with_capacity(parts.len());
        let mut duration_ms = 0u64;
        for part in &parts {
            if part.track_id.is_empty()
                || part.file_id.is_empty()
                || part.duration_ms == 0
                || !seen.insert(&part.file_id)
                || !seen_track.insert(&part.track_id)
            {
                return None;
            }
            starts.push(duration_ms);
            duration_ms = duration_ms.checked_add(part.duration_ms)?;
        }
        Some(Self {
            parts,
            starts,
            duration_ms,
        })
    }

    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }

    /// An exact boundary belongs to the next real file, except the book end.
    pub fn at(&self, whole_ms: u64) -> Option<(&str, u64)> {
        if whole_ms > self.duration_ms {
            return None;
        }
        let index = if whole_ms == self.duration_ms {
            self.parts.len() - 1
        } else {
            self.starts.partition_point(|start| *start <= whole_ms) - 1
        };
        Some((&self.parts[index].track_id, whole_ms - self.starts[index]))
    }

    pub fn whole(&self, file_id: &str, local_ms: u64) -> Option<u64> {
        let index = self.parts.iter().position(|part| part.file_id == file_id)?;
        if local_ms > self.parts[index].duration_ms {
            return None;
        }
        self.starts[index].checked_add(local_ms)
    }

    pub fn part(&self, track_id: &str) -> Option<(&BookPart, u64)> {
        self.parts
            .iter()
            .position(|part| part.track_id == track_id)
            .map(|index| (&self.parts[index], self.starts[index]))
    }

    pub fn final_file(&self, file_id: &str) -> bool {
        self.parts
            .last()
            .is_some_and(|part| part.file_id == file_id)
    }
}

fn matches_current(snapshot: &SessionSnapshot, record: &BookOccurrenceRecord) -> bool {
    snapshot.session_id == record.session_id
        && snapshot.current.as_ref().is_some_and(|current| {
            current.occurrence_id == record.occurrence_id
                && current.source.server_id == record.server_id
                && current.source.track_id == record.track_id
        })
}

fn verified_map(timing: &BookTiming, record: &BookOccurrenceRecord) -> Option<BookMap> {
    if timing.identity != record.identity {
        return None;
    }
    let map = BookMap::new(
        timing
            .parts
            .iter()
            .map(|part| BookPart::new(&part.track_id, &part.audio_file_id, part.duration_ms))
            .collect(),
    )?;
    let (part, offset) = map.part(&record.track_id)?;
    (part.file_id == record.audio_file_id
        && offset == record.part_offset_ms
        && map.duration_ms() == record.duration_ms)
        .then_some(map)
}

fn same_binding(left: &BookOccurrenceRecord, right: &BookOccurrenceRecord) -> bool {
    left.session_id == right.session_id
        && left.occurrence_id == right.occurrence_id
        && left.server_id == right.server_id
        && left.track_id == right.track_id
        && left.identity == right.identity
        && left.audio_file_id == right.audio_file_id
        && left.part_offset_ms == right.part_offset_ms
        && left.duration_ms == right.duration_ms
        && right.mapping_valid
}

/// One serialized, bounded reporter. It has no queued writes and never polls
/// the upstream progress endpoint; only admitted player occurrences can write.
pub(crate) async fn run_reporter(
    playback: super::PlaybackSession,
    db: Arc<crate::db::Database>,
    manager: Arc<tokio::sync::RwLock<crate::server_manager::ServerManager>>,
    shutdown: Arc<AtomicBool>,
) {
    let mut last_record: Option<BookOccurrenceRecord> = None;
    let mut last_sent: Option<(String, u64, bool, Instant)> = None;
    let mut retry_after = Instant::now();
    while !shutdown.load(Ordering::Acquire) {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if shutdown.load(Ordering::Acquire) {
            break;
        }
        let owner = playback.clone();
        let snapshot = match tokio::task::spawn_blocking(move || owner.snapshot()).await {
            Ok(Ok(snapshot)) => snapshot,
            _ => continue,
        };
        if snapshot.mode != PlaybackMode::Main || snapshot.preview.is_some() {
            continue;
        }
        let Some(current) = snapshot.current.as_ref() else {
            continue;
        };
        let record = match db.load_book_continuity(&snapshot.session_id, &current.occurrence_id) {
            Ok(Some(record)) => record,
            Ok(None) => continue,
            Err(_) => continue,
        };
        if !matches_current(&snapshot, &record) || !record.mapping_valid {
            continue;
        }
        if last_record
            .as_ref()
            .is_some_and(|old| old.occurrence_id != record.occurrence_id)
        {
            retry_after = Instant::now();
        }
        last_record = Some(record.clone());
        if Instant::now() < retry_after {
            continue;
        }
        let sample = BookReportSample {
            main: true,
            playing: snapshot.state == TransportState::Playing,
            active: snapshot.playback.status == PlaybackStatus::Active,
            qualified: snapshot
                .playback
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.source == current.source),
            pending_seek: snapshot.playback.pending_seek.is_some(),
            position_ms: snapshot.position_ms,
        };
        let mut progress = sample.progress(&record);
        let mut terminal = false;
        if snapshot.playback.status == PlaybackStatus::Completed
            && snapshot.playback.pending_seek.is_none()
            && let Ok(Some((outcome, position_ms))) =
                db.book_attempt_outcome(&record.session_id, &record.occurrence_id)
            && outcome == "naturalCompletion"
        {
            progress = record
                .part_offset_ms
                .checked_add(position_ms)
                .filter(|whole| *whole <= record.duration_ms)
                .map(|current_ms| BookProgress {
                    current_ms,
                    duration_ms: record.duration_ms,
                    is_finished: false,
                });
            terminal = true;
        } else if snapshot.playback.status == PlaybackStatus::Stopped
            && snapshot.playback.pending_seek.is_none()
            && last_sent
                .as_ref()
                .is_some_and(|(occurrence, _, _, _)| occurrence == &record.occurrence_id)
        {
            progress = record
                .part_offset_ms
                .checked_add(snapshot.position_ms)
                .filter(|whole| *whole <= record.duration_ms)
                .map(|current_ms| BookProgress {
                    current_ms,
                    duration_ms: record.duration_ms,
                    is_finished: false,
                });
            terminal = true;
        }
        let Some(mut progress) = progress else {
            continue;
        };
        let due = terminal
            || last_sent
                .as_ref()
                .is_none_or(|(occurrence, position, _, when)| {
                    occurrence != &record.occurrence_id
                        || when.elapsed() >= Duration::from_secs(15)
                        || position.abs_diff(progress.current_ms) >= 15_000
                });
        if !due {
            continue;
        }
        let provider = match crate::server_manager::get_provider_by_server_id(
            &manager,
            &db,
            &record.server_id,
        )
        .await
        {
            Ok(provider) => provider,
            Err(_) => {
                retry_after = Instant::now() + Duration::from_secs(30);
                continue;
            }
        };
        let timing = match provider.book_timing_for_track(&record.track_id).await {
            Ok(Some(timing)) => timing,
            Err(crate::providers::ProviderError::StaleConfiguration(_))
            | Err(crate::providers::ProviderError::NotFound { .. }) => {
                let _ = db.invalidate_book_continuity(&record);
                continue;
            }
            _ => {
                retry_after = Instant::now() + Duration::from_secs(30);
                continue;
            }
        };
        let Some(map) = verified_map(&timing, &record) else {
            let _ = db.invalidate_book_continuity(&record);
            continue;
        };
        if map
            .part(&record.track_id)
            .is_none_or(|(part, _)| snapshot.position_ms > part.duration_ms)
        {
            let _ = db.invalidate_book_continuity(&record);
            continue;
        }
        if terminal
            && snapshot.playback.status == PlaybackStatus::Completed
            && map.final_file(&record.audio_file_id)
            && map.part(&record.track_id).is_some_and(|(part, _)| {
                progress.current_ms >= map.duration_ms().saturating_sub(1_000)
                    && progress.current_ms <= map.duration_ms()
                    && part.duration_ms > 0
            })
        {
            progress.current_ms = map.duration_ms();
            progress.is_finished = true;
        }
        if last_sent
            .as_ref()
            .is_some_and(|(occurrence, position, finished, _)| {
                occurrence == &record.occurrence_id
                    && *position == progress.current_ms
                    && *finished == progress.is_finished
            })
        {
            continue;
        }
        // The owner and persisted binding must still name the same occurrence
        // after asynchronous detail resolution, immediately before mutation.
        let owner = playback.clone();
        let still_current = tokio::task::spawn_blocking(move || owner.snapshot())
            .await
            .ok()
            .and_then(Result::ok)
            .is_some_and(|latest| {
                matches_current(&latest, &record)
                    && latest.generation_id == snapshot.generation_id
                    && latest.seek_epoch == snapshot.seek_epoch
                    && latest.mode == PlaybackMode::Main
                    && latest.preview.is_none()
                    && latest.playback.pending_seek.is_none()
            });
        if !still_current
            || !db
                .load_book_continuity(&record.session_id, &record.occurrence_id)
                .ok()
                .flatten()
                .as_ref()
                .is_some_and(|live| same_binding(&record, live))
        {
            continue;
        }
        match provider.write_book_progress(&timing, progress).await {
            Ok(()) => {
                let owner = playback.clone();
                if tokio::task::spawn_blocking(move || owner.snapshot())
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .is_some_and(|latest| matches_current(&latest, &record))
                {
                    last_sent = Some((
                        record.occurrence_id.clone(),
                        progress.current_ms,
                        progress.is_finished,
                        Instant::now(),
                    ));
                }
            }
            Err(crate::providers::ProviderError::StaleConfiguration(_))
            | Err(crate::providers::ProviderError::NotFound { .. }) => {
                let _ = db.invalidate_book_continuity(&record);
                retry_after = Instant::now() + Duration::from_secs(60);
            }
            Err(_) => retry_after = Instant::now() + Duration::from_secs(30),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_boundaries_to_real_files_and_round_trips() {
        let map = BookMap::new(vec![
            BookPart::new("one", "file-one", 60_250),
            BookPart::new("two", "file-two", 39_750),
        ])
        .unwrap();
        assert_eq!(map.at(0), Some(("one", 0)));
        assert_eq!(map.at(60_249), Some(("one", 60_249)));
        assert_eq!(map.at(60_250), Some(("two", 0)));
        assert_eq!(map.at(100_000), Some(("two", 39_750)));
        assert_eq!(map.whole("file-two", 1_500), Some(61_750));
        assert_eq!(map.whole("missing", 1_500), None);
        assert_eq!(map.at(100_001), None);
    }

    #[test]
    fn resume_classifies_boundaries_finished_and_invalid_remote_time() {
        let timing = BookTiming {
            identity: ProviderIdentity {
                library_id: "library".into(),
                library_item_id: "item".into(),
                media_id: "media".into(),
            },
            parts: vec![
                crate::providers::BookPartTiming {
                    track_id: "first".into(),
                    audio_file_id: "file-one".into(),
                    duration_ms: 60_000,
                },
                crate::providers::BookPartTiming {
                    track_id: "second".into(),
                    audio_file_id: "file-two".into(),
                    duration_ms: 40_000,
                },
            ],
        };
        assert_eq!(decide_resume(&timing, None), BookResumeDecision::Beginning);
        assert_eq!(
            decide_resume(
                &timing,
                Some(BookProgress {
                    current_ms: 60_000,
                    duration_ms: 100_000,
                    is_finished: false
                })
            ),
            BookResumeDecision::At {
                part_index: 1,
                local_ms: 0
            }
        );
        assert_eq!(
            decide_resume(
                &timing,
                Some(BookProgress {
                    current_ms: 99_000,
                    duration_ms: 100_000,
                    is_finished: true
                })
            ),
            BookResumeDecision::Beginning
        );
        assert_eq!(
            decide_resume(
                &timing,
                Some(BookProgress {
                    current_ms: 100_001,
                    duration_ms: 100_000,
                    is_finished: false
                })
            ),
            BookResumeDecision::Unavailable
        );
        assert_eq!(
            decide_resume(
                &timing,
                Some(BookProgress {
                    current_ms: 10_000,
                    duration_ms: 80_000,
                    is_finished: false
                })
            ),
            BookResumeDecision::Unavailable
        );
    }

    #[test]
    fn rejects_unprovable_durations_and_duplicate_file_identity() {
        assert!(BookMap::new(vec![BookPart::new("a", "x", 0)]).is_none());
        assert!(
            BookMap::new(vec![
                BookPart::new("a", "x", 1_000),
                BookPart::new("b", "x", 2_000),
            ])
            .is_none()
        );
        assert!(BookMap::new(Vec::new()).is_none());
    }

    #[test]
    fn reports_only_qualified_committed_main_playback() {
        let record = BookOccurrenceRecord {
            session_id: "session".into(),
            occurrence_id: "occurrence".into(),
            server_id: "server".into(),
            track_id: "track".into(),
            identity: ProviderIdentity {
                library_id: "library".into(),
                library_item_id: "item".into(),
                media_id: "media".into(),
            },
            audio_file_id: "file".into(),
            part_offset_ms: 10_000,
            duration_ms: 60_000,
            whole_ms: 10_000,
            mapping_valid: true,
        };
        let sample = BookReportSample {
            main: true,
            playing: true,
            active: true,
            qualified: true,
            pending_seek: false,
            position_ms: 2_500,
        };
        assert_eq!(sample.progress(&record).unwrap().current_ms, 12_500);
        for excluded in [
            BookReportSample {
                main: false,
                ..sample
            },
            BookReportSample {
                playing: false,
                ..sample
            },
            BookReportSample {
                active: false,
                ..sample
            },
            BookReportSample {
                qualified: false,
                ..sample
            },
            BookReportSample {
                pending_seek: true,
                ..sample
            },
            BookReportSample {
                position_ms: 60_000,
                ..sample
            },
        ] {
            assert!(excluded.progress(&record).is_none());
        }
    }
}
