use crate::domain::models::{AlbumLoudnessEvidence, AlbumWithTracks};
use serde::{Deserialize, Serialize};

pub(crate) const ALBUM_LOUDNESS_POLICY_VERSION: u32 = 1;
pub(crate) const SAMPLE_PEAK_CEILING: f64 = 0.891_250_938_133_745_6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AlbumLoudnessReason {
    OpenSubsonicReplayGain,
    MetadataAbsent,
    MetadataRejected,
    InconsistentEvidence,
    IncompleteAlbum,
    UnsupportedRepresentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AlbumLoudnessPolicy {
    pub version: u32,
    pub(crate) scalar_bits: u32,
    pub gain_db_bits: Option<u64>,
    pub peak_bits: Option<u64>,
    pub reason: AlbumLoudnessReason,
}

impl AlbumLoudnessPolicy {
    pub const fn unity(reason: AlbumLoudnessReason) -> Self {
        Self {
            version: ALBUM_LOUDNESS_POLICY_VERSION,
            scalar_bits: 1.0f32.to_bits(),
            gain_db_bits: None,
            peak_bits: None,
            reason,
        }
    }

    pub fn scalar(self) -> f32 {
        f32::from_bits(self.scalar_bits)
    }

    pub(crate) fn validate(self) -> Result<(), &'static str> {
        let scalar = self.scalar();
        if self.version != ALBUM_LOUDNESS_POLICY_VERSION
            || !scalar.is_finite()
            || !(0.0 < scalar && scalar <= 31.623)
        {
            return Err("invalid album loudness policy");
        }
        match self.reason {
            AlbumLoudnessReason::OpenSubsonicReplayGain => {
                let (Some(gain_bits), Some(peak_bits)) = (self.gain_db_bits, self.peak_bits) else {
                    return Err("qualified loudness policy lacks evidence");
                };
                let gain = f64::from_bits(gain_bits);
                let peak = f64::from_bits(peak_bits);
                if !gain.is_finite()
                    || !peak.is_finite()
                    || !(-60.0..=30.0).contains(&gain)
                    || !(0.0 < peak && peak <= 64.0)
                {
                    return Err("qualified loudness evidence is invalid");
                }
                if effective_scalar(gain, peak).map(f32::to_bits) != Some(self.scalar_bits) {
                    return Err("qualified loudness policy scalar does not match its evidence");
                }
            }
            _ if self.scalar() != 1.0
                || self.gain_db_bits.is_some()
                || self.peak_bits.is_some() =>
            {
                return Err("unity loudness policy is incoherent");
            }
            _ => {}
        }
        Ok(())
    }
}

pub(crate) fn resolve_album_policy_for(
    album: &AlbumWithTracks,
    requested_album: &str,
) -> AlbumLoudnessPolicy {
    if album.album.id != requested_album
        || album.tracks.len() > super::album::MAX_ALBUM_OCCURRENCES
        || album.tracks.is_empty()
        || album.album.song_count != Some(album.tracks.len() as u32)
        || album
            .tracks
            .iter()
            .any(|track| track.album_id.as_deref() != Some(album.album.id.as_str()))
    {
        return AlbumLoudnessPolicy::unity(AlbumLoudnessReason::IncompleteAlbum);
    }

    let (mut min_gain, mut max_gain) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut min_peak, mut max_peak) = (f64::INFINITY, f64::NEG_INFINITY);
    for track in &album.tracks {
        if !qualified_original(track.suffix.as_deref(), track.content_type.as_deref()) {
            return AlbumLoudnessPolicy::unity(AlbumLoudnessReason::UnsupportedRepresentation);
        }
        let (gain, peak) = match track.album_loudness {
            AlbumLoudnessEvidence::Absent => {
                return AlbumLoudnessPolicy::unity(AlbumLoudnessReason::MetadataAbsent);
            }
            AlbumLoudnessEvidence::Rejected => {
                return AlbumLoudnessPolicy::unity(AlbumLoudnessReason::MetadataRejected);
            }
            AlbumLoudnessEvidence::OpenSubsonic { .. } => {
                track.album_loudness.values().expect("matched evidence")
            }
        };
        if !(-60.0..=30.0).contains(&gain) || !(0.0 < peak && peak <= 64.0) {
            return AlbumLoudnessPolicy::unity(AlbumLoudnessReason::MetadataRejected);
        }
        min_gain = min_gain.min(gain);
        max_gain = max_gain.max(gain);
        min_peak = min_peak.min(peak);
        max_peak = max_peak.max(peak);
    }
    // Allow only numerical subtraction error, not a broader metadata tolerance.
    let gain_roundoff = f64::EPSILON * min_gain.abs().max(max_gain.abs()).max(1.0) * 4.0;
    let peak_roundoff = f64::EPSILON * max_peak.max(1.0) * 4.0;
    if max_gain - min_gain > 0.01 + gain_roundoff
        || max_peak - min_peak > 1e-6f64.max(1e-4 * max_peak) + peak_roundoff
    {
        return AlbumLoudnessPolicy::unity(AlbumLoudnessReason::InconsistentEvidence);
    }

    let Some(scalar) = effective_scalar(min_gain, max_peak) else {
        return AlbumLoudnessPolicy::unity(AlbumLoudnessReason::MetadataRejected);
    };
    AlbumLoudnessPolicy {
        version: ALBUM_LOUDNESS_POLICY_VERSION,
        scalar_bits: scalar.to_bits(),
        gain_db_bits: Some(min_gain.to_bits()),
        peak_bits: Some(max_peak.to_bits()),
        reason: AlbumLoudnessReason::OpenSubsonicReplayGain,
    }
}

fn effective_scalar(gain_db: f64, peak: f64) -> Option<f32> {
    let requested = 10f64.powf(gain_db / 20.0);
    let effective = requested.min(SAMPLE_PEAK_CEILING / peak);
    if !effective.is_finite() || effective <= 0.0 {
        return None;
    }
    let mut scalar = effective as f32;
    if f64::from(scalar) > effective {
        scalar = f32::from_bits(scalar.to_bits() - 1);
    }
    Some(scalar)
}

pub(crate) fn qualified_original(suffix: Option<&str>, content_type: Option<&str>) -> bool {
    let Some(suffix) = suffix.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let suffix = suffix.trim_start_matches('.').to_ascii_lowercase();
    let expected_types: &[&str] = match suffix.as_str() {
        "wav" => &["audio/wav", "audio/x-wav", "audio/wave"],
        "flac" => &["audio/flac", "audio/x-flac"],
        "m4a" => &["audio/mp4", "audio/x-m4a", "audio/aac", "audio/m4a"],
        "mp3" => &["audio/mpeg", "audio/mp3"],
        _ => return false,
    };
    content_type.is_none_or(|value| {
        let normalized = value
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        expected_types.contains(&normalized.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{Album, Song};

    fn resolve_album_policy(album: &AlbumWithTracks) -> AlbumLoudnessPolicy {
        resolve_album_policy_for(album, "album")
    }

    fn song(id: &str, gain: AlbumLoudnessEvidence, suffix: &str) -> Song {
        Song {
            id: id.into(),
            title: id.into(),
            artist_id: None,
            artist_name: None,
            album_id: Some("album".into()),
            album_title: Some("Album".into()),
            duration_seconds: 1,
            bitrate_kbps: None,
            track_number: None,
            disc_number: None,
            cover_art_id: None,
            date_added: None,
            last_played_at: None,
            play_count: None,
            is_favorite: None,
            content_type: None,
            suffix: Some(suffix.into()),
            size_bytes: None,
            album_loudness: gain,
        }
    }

    fn album(rows: Vec<Song>) -> AlbumWithTracks {
        AlbumWithTracks {
            album: Album {
                id: "album".into(),
                title: "Album".into(),
                artist_id: None,
                artist_name: None,
                year: None,
                song_count: Some(rows.len() as u32),
                duration_seconds: None,
                cover_art_id: None,
            },
            tracks: rows,
        }
    }

    #[test]
    fn resolves_one_order_independent_scalar_with_static_peak_protection() {
        let evidence_a = AlbumLoudnessEvidence::open_subsonic(6.0, 0.8);
        let evidence_b = AlbumLoudnessEvidence::open_subsonic(6.005, 0.800_04);
        let forward = resolve_album_policy(&album(vec![
            song("quiet", evidence_a, "flac"),
            song("loud", evidence_b, "mp3"),
        ]));
        let reverse = resolve_album_policy(&album(vec![
            song("loud", evidence_b, "mp3"),
            song("quiet", evidence_a, "flac"),
        ]));
        assert_eq!(forward, reverse);
        assert_eq!(forward.reason, AlbumLoudnessReason::OpenSubsonicReplayGain);
        let expected = (10f64.powf(6.0 / 20.0)).min(SAMPLE_PEAK_CEILING / 0.800_04);
        assert!((f64::from(forward.scalar()) - expected).abs() <= 1e-7);
        assert!(f64::from(forward.scalar()) * 0.800_04 <= SAMPLE_PEAK_CEILING);
    }

    #[test]
    fn freezes_unity_for_missing_rejected_partial_or_conflicting_evidence() {
        for evidence in [
            AlbumLoudnessEvidence::Absent,
            AlbumLoudnessEvidence::Rejected,
        ] {
            assert_eq!(
                resolve_album_policy(&album(vec![song("x", evidence, "flac")])).scalar(),
                1.0
            );
        }
        let conflicting = resolve_album_policy(&album(vec![
            song("a", AlbumLoudnessEvidence::open_subsonic(-3.0, 0.7), "flac"),
            song(
                "b",
                AlbumLoudnessEvidence::open_subsonic(-2.98, 0.7),
                "flac",
            ),
        ]));
        assert_eq!(
            conflicting.reason,
            AlbumLoudnessReason::InconsistentEvidence
        );
        assert_eq!(conflicting.scalar(), 1.0);
    }

    #[test]
    fn enforces_bounds_membership_count_and_supported_original_formats() {
        let valid = AlbumLoudnessEvidence::open_subsonic(0.0, 1.0);
        for evidence in [
            AlbumLoudnessEvidence::open_subsonic(-60.0, 64.0),
            AlbumLoudnessEvidence::open_subsonic(30.0, f64::MIN_POSITIVE),
        ] {
            assert_eq!(
                resolve_album_policy(&album(vec![song("edge", evidence, "wav")])).reason,
                AlbumLoudnessReason::OpenSubsonicReplayGain
            );
        }
        for evidence in [
            AlbumLoudnessEvidence::open_subsonic(-60.001, 1.0),
            AlbumLoudnessEvidence::open_subsonic(30.001, 1.0),
            AlbumLoudnessEvidence::open_subsonic(0.0, 0.0),
            AlbumLoudnessEvidence::open_subsonic(0.0, 64.001),
            AlbumLoudnessEvidence::open_subsonic(f64::NAN, 1.0),
            AlbumLoudnessEvidence::open_subsonic(0.0, f64::INFINITY),
        ] {
            assert_eq!(
                resolve_album_policy(&album(vec![song("x", evidence, "wav")])).scalar(),
                1.0
            );
        }
        let mut wrong_member = song("x", valid, "wav");
        wrong_member.album_id = Some("other".into());
        assert_eq!(
            resolve_album_policy(&album(vec![wrong_member])).reason,
            AlbumLoudnessReason::IncompleteAlbum
        );
        let unsupported = resolve_album_policy(&album(vec![song("x", valid, "opus")]));
        assert_eq!(
            unsupported.reason,
            AlbumLoudnessReason::UnsupportedRepresentation
        );
        let mut contradictory = song("x", valid, "flac");
        contradictory.content_type = Some("audio/mpeg".into());
        assert_eq!(
            resolve_album_policy(&album(vec![contradictory])).reason,
            AlbumLoudnessReason::UnsupportedRepresentation
        );
        let mut missing_count = album(vec![song("x", valid, "wav")]);
        missing_count.album.song_count = None;
        assert_eq!(
            resolve_album_policy(&missing_count).reason,
            AlbumLoudnessReason::IncompleteAlbum
        );
    }

    #[test]
    fn validation_tolerances_are_inclusive_at_the_documented_boundaries() {
        let accepted = resolve_album_policy(&album(vec![
            song("a", AlbumLoudnessEvidence::open_subsonic(-3.0, 1.0), "wav"),
            song(
                "b",
                AlbumLoudnessEvidence::open_subsonic(-2.99, 1.000_1),
                "wav",
            ),
        ]));
        assert_eq!(accepted.reason, AlbumLoudnessReason::OpenSubsonicReplayGain);
        let rejected = resolve_album_policy(&album(vec![
            song("a", AlbumLoudnessEvidence::open_subsonic(-3.0, 1.0), "wav"),
            song(
                "b",
                AlbumLoudnessEvidence::open_subsonic(-2.989, 1.0),
                "wav",
            ),
        ]));
        assert_eq!(rejected.reason, AlbumLoudnessReason::InconsistentEvidence);
    }

    #[test]
    fn rejects_wrong_requested_identity_and_oversized_evidence() {
        let row = song("x", AlbumLoudnessEvidence::open_subsonic(6.0, 0.5), "flac");
        assert_eq!(
            resolve_album_policy_for(&album(vec![row.clone()]), "other").reason,
            AlbumLoudnessReason::IncompleteAlbum
        );
        let oversized = album(vec![row; super::super::album::MAX_ALBUM_OCCURRENCES + 1]);
        assert_eq!(
            resolve_album_policy(&oversized).reason,
            AlbumLoudnessReason::IncompleteAlbum
        );
    }

    #[test]
    fn decimal_gain_boundary_accepts_roundoff_but_rejects_real_excess() {
        for base in [-59.0, -1.01, 0.0, 1.0, 29.0] {
            for (delta, accepted) in [(0.009999999, true), (0.01, true), (0.010000001, false)] {
                let result = resolve_album_policy(&album(vec![
                    song("a", AlbumLoudnessEvidence::open_subsonic(base, 0.5), "flac"),
                    song(
                        "b",
                        AlbumLoudnessEvidence::open_subsonic(base + delta, 0.5),
                        "flac",
                    ),
                ]));
                assert_eq!(
                    result.reason == AlbumLoudnessReason::OpenSubsonicReplayGain,
                    accepted,
                    "base={base}, delta={delta}"
                );
            }
        }
    }

    #[test]
    fn persisted_policy_validation_rejects_a_tampered_scalar() {
        let mut policy = resolve_album_policy(&album(vec![song(
            "x",
            AlbumLoudnessEvidence::open_subsonic(-3.0, 0.9),
            "flac",
        )]));
        assert!(policy.validate().is_ok());
        policy.scalar_bits = 0.5f32.to_bits();
        assert!(policy.validate().is_err());
    }
}
