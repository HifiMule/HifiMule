//! Endpoint identities contain no native handles and never restore by display name.
use super::config::OutputPreference;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[cfg(any(target_os = "linux", test))]
mod ack;
#[cfg(any(target_os = "windows", test))]
mod endpoint_signal;
#[cfg(target_os = "windows")]
pub(crate) mod wasapi_monitor;
pub mod worker;

pub const MAX_OUTPUTS: usize = 256;

#[cfg(target_os = "linux")]
mod pulse;
#[cfg(any(target_os = "linux", test))]
mod pulse_route;
#[cfg(target_os = "linux")]
pub(crate) mod pulse_stream;

pub fn discover() -> Discovery {
    #[cfg(target_os = "linux")]
    {
        pulse::discover()
    }
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        discover_cpal()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Discovery {
            outputs: vec![],
            error: Some("OUTPUT_SHARED_UNSUPPORTED"),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn discover_cpal() -> Discovery {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let default = host.default_output_device().and_then(|d| d.id().ok());
    let Ok(devices) = host.output_devices() else {
        return Discovery {
            outputs: vec![],
            error: Some("OUTPUT_UNAVAILABLE"),
        };
    };
    let mut result = Discovery {
        outputs: vec![],
        error: None,
    };
    for (index, device) in devices.take(MAX_OUTPUTS + 1).enumerate() {
        if index == MAX_OUTPUTS {
            result.error = Some("OUTPUT_ENUMERATION_TRUNCATED");
            break;
        }
        let (Ok(id), Ok(description)) = (device.id(), device.description()) else {
            result.error = Some("OUTPUT_DISCOVERY_PARTIAL");
            continue;
        };
        if id.id().len() > 4096 || description.name().len() > 4096 {
            result.error = Some("OUTPUT_DISCOVERY_PARTIAL");
            continue;
        }
        let backend = if cfg!(target_os = "macos") {
            "coreaudio"
        } else {
            "wasapi"
        };
        let preference = OutputPreference {
            backend: backend.into(),
            stable_id: id.id().into(),
            display_name: description.name().into(),
            identity_properties: BTreeMap::new(),
        };
        let detail = format!(
            "{} · {}",
            description
                .manufacturer()
                .unwrap_or(backend)
                .chars()
                .take(512)
                .collect::<String>(),
            description.interface_type()
        );
        result.outputs.push(OutputDescriptor {
            output_id: output_id(&preference),
            display_name: preference.display_name.clone(),
            detail,
            backend: backend.into(),
            available: true,
            is_default: default.as_ref() == Some(&id),
            identity_confidence: "stable".into(),
            is_virtual: description.device_type() == cpal::DeviceType::Virtual
                || description.interface_type() == cpal::InterfaceType::Virtual,
            preference: Some(preference),
        });
    }
    result
}

/// Enumerated devices are concrete on CoreAudio; the default getter creates an
/// auto-following AudioUnit and must never be used to reopen a saved preference.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn open_device(preference: &OutputPreference) -> Result<cpal::Device, &'static str> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let backend = if cfg!(target_os = "macos") {
        "coreaudio"
    } else {
        "wasapi"
    };
    if preference.backend != backend {
        return Err("OUTPUT_UNAVAILABLE");
    }
    let devices = cpal::default_host()
        .output_devices()
        .map_err(|_| "OUTPUT_UNAVAILABLE")?;
    let mut found = None;
    for device in devices.take(MAX_OUTPUTS + 1) {
        if device.id().is_ok_and(|id| id.id() == preference.stable_id) {
            if found.is_some() {
                return Err("OUTPUT_IDENTITY_AMBIGUOUS");
            }
            found = Some(device);
        }
    }
    found.ok_or("OUTPUT_UNAVAILABLE")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputDescriptor {
    pub output_id: String,
    pub display_name: String,
    pub detail: String,
    pub backend: String,
    pub available: bool,
    pub is_default: bool,
    pub identity_confidence: String,
    pub is_virtual: bool,
    #[serde(skip)]
    pub preference: Option<OutputPreference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub outputs: Vec<OutputDescriptor>,
    pub error: Option<&'static str>,
}

impl Discovery {
    /// Partial enumeration does not invalidate endpoints that were identified.
    /// Missing, duplicate or unsupported selections still fail resolve().
    pub fn can_resolve(&self) -> bool {
        matches!(
            self.error,
            None | Some(
                "OUTPUT_DISCOVERY_PARTIAL"
                    | "OUTPUT_ENUMERATION_TRUNCATED"
                    | "OUTPUT_IDENTITY_AMBIGUOUS"
            )
        )
    }
}

#[derive(Default)]
pub struct Labels {
    ordinals: BTreeMap<String, usize>,
    next: usize,
}

pub fn output_id(preference: &OutputPreference) -> String {
    let mut hash = blake3::Hasher::new();
    hash.update(preference.backend.as_bytes());
    hash.update(&[0]);
    hash.update(preference.stable_id.as_bytes());
    hash.finalize().to_hex().to_string()
}

impl Labels {
    pub fn reconcile(&mut self, mut discovery: Discovery) -> Discovery {
        if discovery.outputs.len() > MAX_OUTPUTS {
            discovery.outputs.truncate(MAX_OUTPUTS);
            discovery.error = Some("OUTPUT_ENUMERATION_TRUNCATED");
        }
        let current_ids: BTreeSet<_> = discovery
            .outputs
            .iter()
            .map(|o| o.output_id.clone())
            .collect();
        let mut counts = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut duplicate_ids = BTreeSet::new();
        for output in &discovery.outputs {
            *counts.entry(output.display_name.clone()).or_insert(0) += 1;
        }
        for output in &mut discovery.outputs {
            if !ids.insert(output.output_id.clone()) {
                discovery.error = Some("OUTPUT_IDENTITY_AMBIGUOUS");
                duplicate_ids.insert(output.output_id.clone());
            }
            if !self.ordinals.contains_key(&output.output_id)
                && self.ordinals.len() == MAX_OUTPUTS
                && let Some(evicted) = self
                    .ordinals
                    .iter()
                    .filter(|(id, _)| !current_ids.contains(*id))
                    .min_by_key(|(_, ordinal)| **ordinal)
                    .map(|(id, _)| id.clone())
            {
                self.ordinals.remove(&evicted);
            }
            let ordinal = *self
                .ordinals
                .entry(output.output_id.clone())
                .or_insert_with(|| {
                    self.next += 1;
                    self.next
                });
            if counts[&output.display_name] > 1 {
                output.detail = format!("{} · {ordinal}", output.detail);
            }
        }
        for output in &mut discovery.outputs {
            if duplicate_ids.contains(&output.output_id) {
                output.available = false;
                output.identity_confidence = "ambiguous".into();
            }
        }
        // Keep absent identities through ordinary reconnects, evicting only
        // when the bounded display cache fills. Restore never uses ordinals.
        discovery
    }
}

pub fn resolve<'a>(
    saved: &OutputPreference,
    outputs: &'a [OutputDescriptor],
) -> Result<&'a OutputDescriptor, &'static str> {
    let matches: Vec<_> = outputs
        .iter()
        .filter(|o| {
            o.preference
                .as_ref()
                .is_some_and(|p| p.backend == saved.backend && p.stable_id == saved.stable_id)
        })
        .collect();
    if matches.len() > 1 {
        return Err("OUTPUT_IDENTITY_AMBIGUOUS");
    }
    let output = matches.first().copied().ok_or("OUTPUT_UNAVAILABLE")?;
    if !output.available {
        return Err("OUTPUT_UNAVAILABLE");
    }
    let current = output.preference.as_ref().ok_or("OUTPUT_UNAVAILABLE")?;
    if saved
        .identity_properties
        .iter()
        .any(|(k, v)| current.identity_properties.get(k) != Some(v))
    {
        return Err("OUTPUT_IDENTITY_AMBIGUOUS");
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[ignore = "requires real native audio service; opens unstarted silent streams"]
    fn native_concrete_endpoints_open_silently_and_missing_identity_never_falls_back() {
        use cpal::traits::DeviceTrait;
        let inventory = discover();
        assert_eq!(inventory.error, None);
        assert!(
            !inventory.outputs.is_empty(),
            "native audio service exposes no output"
        );
        let mut opened = 0;
        for output in inventory.outputs.iter().filter(|output| output.available) {
            let preference = output.preference.as_ref().unwrap();
            let device = open_device(preference).unwrap();
            assert_eq!(device.id().unwrap().id(), preference.stable_id);
            let config = device.default_output_config().unwrap();
            #[cfg(target_os = "windows")]
            let _monitor = super::wasapi_monitor::SelectedEndpointMonitor::new(
                &preference.stable_id,
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )
            .unwrap();
            let stream = device
                .build_output_stream_raw(
                    config.config(),
                    config.sample_format(),
                    |data, _| data.bytes_mut().fill(0),
                    |_| {},
                    None,
                )
                .unwrap();
            // Deliberately never call play: this is an endpoint binding smoke,
            // not physical playback/no-reroute acceptance.
            drop(stream);
            let mut absent = preference.clone();
            absent.stable_id = format!("hifimule-absent-{}", uuid::Uuid::new_v4());
            assert!(matches!(open_device(&absent), Err("OUTPUT_UNAVAILABLE")));
            opened += 1;
        }
        assert!(opened > 0);
        println!(
            "Opened and released {opened} concrete shared outputs without starting playback; absent identities rejected"
        );
    }

    fn endpoint(id: &str, name: &str) -> OutputDescriptor {
        let preference = OutputPreference {
            backend: "coreaudio".into(),
            stable_id: id.into(),
            display_name: name.into(),
            identity_properties: BTreeMap::new(),
        };
        OutputDescriptor {
            output_id: output_id(&preference),
            display_name: name.into(),
            detail: "USB".into(),
            backend: "coreaudio".into(),
            available: true,
            is_default: false,
            identity_confidence: "stable".into(),
            is_virtual: false,
            preference: Some(preference),
        }
    }

    #[test]
    fn restoration_uses_identity_even_after_rename_and_never_falls_back() {
        let saved = endpoint("one", "Headphones").preference.unwrap();
        let mut replacement = endpoint("two", "Headphones");
        replacement.is_default = true;
        assert_eq!(resolve(&saved, &[replacement]), Err("OUTPUT_UNAVAILABLE"));
        let renamed = endpoint("one", "Renamed");
        assert_eq!(
            resolve(&saved, std::slice::from_ref(&renamed)),
            Ok(&renamed)
        );
    }

    #[test]
    fn duplicate_names_keep_labels_across_enumeration_reorder() {
        let mut labels = Labels::default();
        let a = endpoint("a", "Headphones");
        let b = endpoint("b", "Headphones");
        let first = labels.reconcile(Discovery {
            outputs: vec![a.clone(), b.clone()],
            error: None,
        });
        let second = labels.reconcile(Discovery {
            outputs: vec![b, a],
            error: None,
        });
        assert_ne!(first.outputs[0].detail, first.outputs[1].detail);
        assert_eq!(first.outputs[0].detail, second.outputs[1].detail);
        assert_eq!(first.outputs[1].detail, second.outputs[0].detail);
    }

    #[test]
    fn duplicate_output_labels_survive_temporary_disconnect() {
        let mut labels = Labels::default();
        let outputs = vec![endpoint("a", "USB"), endpoint("b", "USB")];
        let initial = labels.reconcile(Discovery {
            outputs: outputs.clone(),
            error: None,
        });
        labels.reconcile(Discovery {
            outputs: vec![outputs[0].clone()],
            error: None,
        });
        let restored = labels.reconcile(Discovery {
            outputs,
            error: None,
        });
        assert_eq!(initial.outputs[1].detail, restored.outputs[1].detail);
    }

    #[test]
    fn conflicting_identity_properties_and_duplicate_ids_are_unavailable() {
        let endpoint = endpoint("a", "Headphones");
        let mut saved = endpoint.preference.clone().unwrap();
        saved
            .identity_properties
            .insert("device.serial".into(), "old".into());
        assert_eq!(
            resolve(&saved, std::slice::from_ref(&endpoint)),
            Err("OUTPUT_IDENTITY_AMBIGUOUS")
        );
        let inventory = Labels::default().reconcile(Discovery {
            outputs: vec![endpoint.clone(), endpoint],
            error: None,
        });
        assert_eq!(inventory.error, Some("OUTPUT_IDENTITY_AMBIGUOUS"));
        assert!(inventory.outputs.iter().all(|o| !o.available));
    }

    #[test]
    fn duplicate_ids_do_not_invalidate_an_unrelated_stable_output() {
        let healthy = endpoint("healthy", "Headphones");
        let duplicate = endpoint("duplicate", "Other device");
        let inventory = Labels::default().reconcile(Discovery {
            outputs: vec![duplicate.clone(), healthy.clone(), duplicate],
            error: None,
        });
        assert!(inventory.can_resolve());
        assert!(resolve(healthy.preference.as_ref().unwrap(), &inventory.outputs).is_ok());
        assert!(!inventory.outputs[0].available);
        assert!(!inventory.outputs[2].available);
        assert!(
            !Discovery {
                outputs: vec![healthy],
                error: Some("OUTPUT_DISCOVERY_TIMEOUT")
            }
            .can_resolve()
        );
    }

    #[test]
    fn inventory_and_label_cache_are_bounded() {
        let mut labels = Labels::default();
        let inventory = labels.reconcile(Discovery {
            outputs: (0..257).map(|i| endpoint(&i.to_string(), "USB")).collect(),
            error: None,
        });
        assert_eq!(inventory.outputs.len(), MAX_OUTPUTS);
        assert_eq!(inventory.error, Some("OUTPUT_ENUMERATION_TRUNCATED"));
        labels.reconcile(Discovery {
            outputs: vec![],
            error: None,
        });
        assert_eq!(labels.ordinals.len(), MAX_OUTPUTS);
        for n in 0..1000 {
            labels.reconcile(Discovery {
                outputs: vec![endpoint(&format!("new-{n}"), "USB")],
                error: None,
            });
            assert!(labels.ordinals.len() <= MAX_OUTPUTS);
        }
    }
}
