//! Bounded, coalesced publication storage for the D-Bus backend.
use crate::{MediaControlCapabilities, MediaMetadata, MediaPlayback};
use std::convert::TryInto;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OwnedMetadata {
    pub title: Option<String>,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub cover_url: Option<String>,
    pub duration: Option<i64>,
}

impl From<MediaMetadata<'_>> for OwnedMetadata {
    fn from(other: MediaMetadata) -> Self {
        OwnedMetadata {
            title: other.title.map(|s| s.to_string()),
            artist: other.artist.map(|s| s.to_string()),
            album: other.album.map(|s| s.to_string()),
            cover_url: other.cover_url.map(|s| s.to_string()),
            // TODO: This should probably not have an unwrap
            duration: other.duration.map(|d| d.as_micros().try_into().unwrap()),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub(crate) enum InternalEvent {
    ChangeMetadata(OwnedMetadata),
    ChangePlayback(MediaPlayback),
    ChangeVolume(f64),
    ChangeCapabilities(MediaControlCapabilities),
    Kill,
}

/// At most one value per property is retained regardless of publication rate.
/// Shutdown is a separate terminal flag and never waits behind publication.
#[derive(Default)]
pub(crate) struct PendingUpdates {
    metadata: Option<OwnedMetadata>,
    playback: Option<MediaPlayback>,
    volume: Option<f64>,
    capabilities: Option<MediaControlCapabilities>,
    stopped: bool,
}

impl PendingUpdates {
    pub(crate) fn push(&mut self, event: InternalEvent) -> bool {
        if self.stopped {
            return false;
        }
        match event {
            InternalEvent::ChangeMetadata(value) => self.metadata = Some(value),
            InternalEvent::ChangePlayback(value) => self.playback = Some(value),
            InternalEvent::ChangeVolume(value) => self.volume = Some(value),
            InternalEvent::ChangeCapabilities(value) => self.capabilities = Some(value),
            InternalEvent::Kill => {
                *self = Self {
                    stopped: true,
                    ..Self::default()
                }
            }
        }
        true
    }

    pub(crate) fn take(&mut self) -> Option<Vec<InternalEvent>> {
        if self.stopped {
            return None;
        }
        let mut events = Vec::with_capacity(4);
        if let Some(value) = self.capabilities.take() {
            events.push(InternalEvent::ChangeCapabilities(value));
        }
        if let Some(value) = self.metadata.take() {
            events.push(InternalEvent::ChangeMetadata(value));
        }
        if let Some(value) = self.playback.take() {
            events.push(InternalEvent::ChangePlayback(value));
        }
        if let Some(value) = self.volume.take() {
            events.push(InternalEvent::ChangeVolume(value));
        }
        Some(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MediaPosition;
    use std::time::Duration;

    #[test]
    fn publication_burst_retains_only_latest_values() {
        let mut pending = PendingUpdates::default();
        for i in 0..10_000 {
            pending.push(InternalEvent::ChangeMetadata(OwnedMetadata {
                title: Some(format!("track {i}")),
                ..Default::default()
            }));
            pending.push(InternalEvent::ChangePlayback(MediaPlayback::Playing {
                progress: Some(MediaPosition(Duration::from_millis(i))),
            }));
            pending.push(InternalEvent::ChangeVolume(i as f64));
            pending.push(InternalEvent::ChangeCapabilities(
                MediaControlCapabilities {
                    play: i % 2 == 0,
                    pause: i % 2 != 0,
                    ..Default::default()
                },
            ));
        }
        let events = pending.take().unwrap();
        assert_eq!(events.len(), 4);
        assert!(
            events.contains(&InternalEvent::ChangeMetadata(OwnedMetadata {
                title: Some("track 9999".into()),
                ..Default::default()
            }))
        );
        assert!(
            events.contains(&InternalEvent::ChangePlayback(MediaPlayback::Playing {
                progress: Some(MediaPosition(Duration::from_millis(9999))),
            }))
        );
        assert!(events.contains(&InternalEvent::ChangeVolume(9999.0)));
        assert!(events.contains(&InternalEvent::ChangeCapabilities(
            MediaControlCapabilities {
                pause: true,
                ..Default::default()
            }
        )));
        assert!(pending.take().unwrap().is_empty());
    }

    #[test]
    fn shutdown_discards_backlog_and_rejects_later_publication() {
        let mut pending = PendingUpdates::default();
        pending.push(InternalEvent::ChangeMetadata(OwnedMetadata {
            title: Some("stale".into()),
            ..Default::default()
        }));
        pending.push(InternalEvent::ChangePlayback(MediaPlayback::Playing {
            progress: None,
        }));
        pending.push(InternalEvent::Kill);
        assert!(pending.take().is_none());
        assert!(pending.metadata.is_none());
        assert!(pending.playback.is_none());
        assert!(!pending.push(InternalEvent::ChangePlayback(MediaPlayback::Stopped)));
        assert!(pending.take().is_none());
    }
}
