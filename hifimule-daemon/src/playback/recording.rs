//! Private, conservative recording identity for automatic Radio occurrences.

/// Changing the resolver requires a new key namespace and a migration decision.
pub const RESOLVER_VERSION: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingProvenance {
    JellyfinRecording,
    OpenSubsonicSong,
}

impl RecordingProvenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::JellyfinRecording => "jellyfin.providerIds.MusicBrainzRecording",
            Self::OpenSubsonicSong => "openSubsonic.child.musicBrainzId.song",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecordingKey(String);

impl RecordingKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Only an explicit recording UUID is positive evidence. A version word in a
/// title is veto evidence; titles never create positive identity by themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingEvidence {
    pub provenance: RecordingProvenance,
    key: Option<RecordingKey>,
}

impl RecordingEvidence {
    pub fn from_recording_field(
        provenance: RecordingProvenance,
        raw: Option<&str>,
        title: &str,
    ) -> Self {
        Self::from_recording_fields(provenance, raw, title, None)
    }

    pub fn from_recording_fields(
        provenance: RecordingProvenance,
        raw: Option<&str>,
        title: &str,
        album: Option<&str>,
    ) -> Self {
        let uuid = raw
            .filter(|raw| raw.trim() == *raw)
            .and_then(|raw| uuid::Uuid::parse_str(raw).ok())
            .filter(|uuid| !uuid.is_nil());
        let key = uuid
            .zip(version_signature(title, album))
            .map(|(uuid, version)| {
                RecordingKey(format!("mbrec:{RESOLVER_VERSION}:{uuid}:{version}",))
            });
        Self { provenance, key }
    }

    pub fn key(&self) -> Option<&RecordingKey> {
        self.key.as_ref()
    }
}

fn version_signature(title: &str, album: Option<&str>) -> Option<&'static str> {
    let lower = format!("{} {}", title, album.unwrap_or_default()).to_ascii_lowercase();
    // An explicit version/performance label is a veto against an unlabelled
    // copy. Multiple labels are treated as uncertain rather than unioned.
    let markers = ["live", "cover", "remix", "edit", "acoustic", "demo"];
    let found: Vec<_> = markers
        .into_iter()
        .filter(|word| {
            lower
                .split(|ch: char| !ch.is_ascii_alphanumeric())
                .any(|part| part == *word)
        })
        .collect();
    // An unfamiliar explicit version is uncertainty, never the unmarked
    // recording. Provider tags can reuse a recording UUID incorrectly.
    let other_versions = [
        "instrumental",
        "alternate",
        "take",
        "version",
        "mix",
        "reprise",
        "unplugged",
        "orchestral",
        "stripped",
        "rerecorded",
        "radio",
        "extended",
        "club",
        "dub",
        "karaoke",
        "mono",
        "stereo",
        "remaster",
        "remastered",
    ];
    let has_other_version = |value: &str| {
        other_versions.iter().any(|word| {
            value
                .split(|ch: char| !ch.is_ascii_alphanumeric())
                .any(|part| part.eq_ignore_ascii_case(word))
        })
    };
    let title_qualifier = title.contains('(') || title.contains('[') || title.contains(" - ");
    if (title_qualifier && (found.is_empty() || has_other_version(title)))
        || album.is_some_and(has_other_version)
    {
        return None;
    }
    match found.as_slice() {
        [] => Some("plain"),
        ["live"] => Some("live"),
        ["cover"] => Some("cover"),
        ["remix"] => Some("remix"),
        ["edit"] => Some("edit"),
        ["acoustic"] => Some("acoustic"),
        ["demo"] => Some("demo"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "189002e7-3285-4e2e-92a3-7f6c30d407a2";

    #[test]
    fn accepts_only_one_recording_uuid_with_known_provenance() {
        let exact = RecordingEvidence::from_recording_field(
            RecordingProvenance::OpenSubsonicSong,
            Some(ID),
            "A Song",
        );
        assert_eq!(exact.key().unwrap().as_str(), format!("mbrec:1:{ID}:plain"));
        assert!(
            RecordingEvidence::from_recording_field(
                RecordingProvenance::OpenSubsonicSong,
                Some("bad-id"),
                "A Song",
            )
            .key()
            .is_none()
        );
        assert!(
            RecordingEvidence::from_recording_field(
                RecordingProvenance::OpenSubsonicSong,
                Some(&format!("{ID};{ID}")),
                "A Song",
            )
            .key()
            .is_none()
        );
    }

    #[test]
    fn explicit_performance_versions_veto_a_shared_id() {
        let studio = RecordingEvidence::from_recording_field(
            RecordingProvenance::JellyfinRecording,
            Some(ID),
            "A Song",
        );
        let live = RecordingEvidence::from_recording_field(
            RecordingProvenance::OpenSubsonicSong,
            Some(ID),
            "A Song (Live)",
        );
        assert_ne!(studio.key(), live.key());
        let live_album = RecordingEvidence::from_recording_fields(
            RecordingProvenance::JellyfinRecording,
            Some(ID),
            "A Song",
            Some("Live at the Hall"),
        );
        assert_ne!(studio.key(), live_album.key());
        assert!(
            RecordingEvidence::from_recording_fields(
                RecordingProvenance::JellyfinRecording,
                Some(ID),
                "Take",
                Some("Ordinary Album"),
            )
            .key()
            .is_some()
        );
        let contradictory = RecordingEvidence::from_recording_fields(
            RecordingProvenance::JellyfinRecording,
            Some(ID),
            "A Song (Live Remix)",
            None,
        );
        assert!(contradictory.key().is_none());
        for title in [
            "A Song (Instrumental)",
            "A Song (Alternate Take)",
            "A Song [Unknown Version]",
        ] {
            assert!(
                RecordingEvidence::from_recording_field(
                    RecordingProvenance::OpenSubsonicSong,
                    Some(ID),
                    title,
                )
                .key()
                .is_none()
            );
        }
    }
}
