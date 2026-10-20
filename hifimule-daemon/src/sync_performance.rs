//! Validated per-synchronization performance settings. No secrets enter diagnostics.
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SyncPerformance {
    pub copy_buffer_bytes: usize,
    pub local_encoding_workers: usize,
    pub staging_max_tracks: usize,
    pub staging_max_bytes: u64,
    pub checkpoint_track_updates: usize,
    pub checkpoint_interval: Duration,
}

impl Default for SyncPerformance {
    fn default() -> Self {
        Self {
            copy_buffer_bytes: 1024 * 1024,
            local_encoding_workers: 0,
            staging_max_tracks: 8,
            staging_max_bytes: 2048 * 1024 * 1024,
            checkpoint_track_updates: 1000,
            checkpoint_interval: Duration::from_secs(600),
        }
    }
}

impl SyncPerformance {
    #[cfg(test)]
    fn from_json(value: Option<&serde_json::Value>) -> Self {
        let raw = value.map(|v| serde_json::value::to_raw_value(v).unwrap());
        Self::from_raw(raw.as_deref())
    }

    pub(crate) fn from_raw(value: Option<&RawValue>) -> Self {
        let defaults = Self::default();
        let Some(value) = value else {
            return defaults;
        };
        let Ok(object) = serde_json::from_str::<HashMap<String, Box<RawValue>>>(value.get()) else {
            crate::daemon_log!(
                "[Sync] Invalid sync_performance: expected an object; using defaults"
            );
            return defaults;
        };
        let field = |name: &str, min: u64, max: u64, fallback: u64| {
            let Some(value) = object.get(name) else {
                return fallback;
            };
            match value
                .get()
                .parse::<u64>()
                .ok()
                .filter(|v| (min..=max).contains(v))
            {
                Some(value) => value,
                None => {
                    crate::daemon_log!(
                        "[Sync] Invalid sync_performance.{name}: expected integer {min}–{max}; using default"
                    );
                    fallback
                }
            }
        };
        let kib = field("copy_buffer_kib", 8, 16384, 1024);
        let mib = field("staging_max_mib", 2048, 32768, 2048);
        Self {
            local_encoding_workers: field("local_encoding_workers", 0, 64, 0) as usize,
            copy_buffer_bytes: kib
                .checked_mul(1024)
                .and_then(|v| usize::try_from(v).ok())
                .unwrap_or(defaults.copy_buffer_bytes),
            staging_max_tracks: field("staging_max_tracks", 1, 32, 8) as usize,
            staging_max_bytes: mib
                .checked_mul(1024 * 1024)
                .unwrap_or(defaults.staging_max_bytes),
            checkpoint_track_updates: field("checkpoint_track_updates", 1, 5000, 1000) as usize,
            checkpoint_interval: Duration::from_secs(field(
                "checkpoint_interval_seconds",
                1,
                900,
                600,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn local_workers_validate_independently() {
        for valid in [0, 1, 64] {
            assert_eq!(
                SyncPerformance::from_json(Some(&json!({"local_encoding_workers":valid})))
                    .local_encoding_workers,
                valid
            );
        }
        for invalid in [
            Value::Null,
            json!(1.5),
            json!(-1),
            json!(65),
            json!("4"),
            json!(true),
            json!(1e100),
        ] {
            let value = json!({"local_encoding_workers":invalid,"copy_buffer_kib":64});
            let actual = SyncPerformance::from_json(Some(&value));
            assert_eq!(actual.local_encoding_workers, 0);
            assert_eq!(actual.copy_buffer_bytes, 65536);
        }
    }

    #[test]
    fn unbounded_json_numbers_fall_back_independently() {
        let raw = RawValue::from_string(r#"{"copy_buffer_kib":1e400,"staging_max_tracks":3,"checkpoint_track_updates":184467440737095516160000}"#.into()).unwrap();
        let settings = SyncPerformance::from_raw(Some(&raw));
        assert_eq!(settings.copy_buffer_bytes, 1048576);
        assert_eq!(settings.staging_max_tracks, 3);
        assert_eq!(settings.checkpoint_track_updates, 1000);
    }

    #[test]
    fn defaults_absence_and_invalid_sections() {
        for value in [
            None,
            Some(json!({})),
            Some(Value::Null),
            Some(json!([])),
            Some(json!("secret")),
        ] {
            assert_eq!(
                SyncPerformance::from_json(value.as_ref()),
                SyncPerformance::default()
            );
        }
    }

    #[test]
    fn bounds_and_invalid_fields_are_independent() {
        let fields = [
            ("copy_buffer_kib", 8, 16384),
            ("staging_max_tracks", 1, 32),
            ("staging_max_mib", 2048, 32768),
            ("checkpoint_track_updates", 1, 5000),
            ("checkpoint_interval_seconds", 1, 900),
        ];
        for (name, min, max) in fields {
            for bad in [
                Value::Null,
                json!(false),
                json!("12"),
                json!(1.5),
                json!(-1),
                json!(0),
                json!(min - 1),
                json!(max + 1),
                json!(u64::MAX),
                json!(1e100),
            ] {
                let mut input = json!({"copy_buffer_kib": 64, "staging_max_tracks": 8});
                input[name] = bad;
                let actual = SyncPerformance::from_json(Some(&input));
                input.as_object_mut().unwrap().remove(name);
                assert_eq!(actual, SyncPerformance::from_json(Some(&input)), "{name}");
            }
            for bound in [min, max] {
                let mut input = json!({});
                input[name] = json!(bound);
                let actual = SyncPerformance::from_json(Some(&input));
                match name {
                    "copy_buffer_kib" => {
                        assert_eq!(actual.copy_buffer_bytes, bound as usize * 1024)
                    }
                    "staging_max_tracks" => assert_eq!(actual.staging_max_tracks, bound as usize),
                    "staging_max_mib" => assert_eq!(actual.staging_max_bytes, bound * 1024 * 1024),
                    "checkpoint_track_updates" => {
                        assert_eq!(actual.checkpoint_track_updates, bound as usize)
                    }
                    _ => assert_eq!(actual.checkpoint_interval.as_secs(), bound),
                }
            }
        }
    }
}
