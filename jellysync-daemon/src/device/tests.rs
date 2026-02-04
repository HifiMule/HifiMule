use super::*;
use std::fs;
use tempfile::tempdir;

#[tokio::test]
async fn test_probe_no_manifest() {
    let dir = tempdir().unwrap();
    let res = DeviceProber::probe(dir.path()).await.unwrap();
    assert!(res.is_none());
}

#[tokio::test]
async fn test_probe_with_manifest() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join(".jellysync.json");
    let manifest_json = r#"{"id": "test-device-123", "name": "My iPod"}"#;
    fs::write(manifest_path, manifest_json).unwrap();

    let res = DeviceProber::probe(dir.path()).await.unwrap();
    assert!(res.is_some());
    let manifest = res.unwrap();
    assert_eq!(manifest.id, "test-device-123");
    assert_eq!(manifest.name, Some("My iPod".to_string()));
}

#[tokio::test]
async fn test_probe_invalid_manifest() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join(".jellysync.json");
    let manifest_json = r#"{"id": "test-device-123", "invalid": true"#; // Missing closing brace
    fs::write(manifest_path, manifest_json).unwrap();

    let res = DeviceProber::probe(dir.path()).await;
    assert!(res.is_err());
}
