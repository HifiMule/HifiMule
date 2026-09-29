//! Opt-in controlled-account effect check. Inputs and credentials never enter evidence.
use super::*;
use crate::playback::feedback::*;
use crate::providers::{CredentialKind, MediaProvider, ProviderCredentials};
use std::sync::atomic::AtomicBool;

#[derive(Deserialize)]
struct Target {
    provider: String,
    server_url: String,
    track_id: String,
    token: Option<String>,
    user_id: Option<String>,
    username: Option<String>,
    password: Option<String>,
}

#[tokio::test]
#[ignore = "Requires explicit controlled-account fixture and restores the original preference"]
async fn feedback_configured_server_effects_through_production_delivery() {
    let path =
        std::env::var("HIFIMULE_FEEDBACK_LIVE_TARGETS").expect("controlled fixture path required");
    let targets: Vec<Target> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).expect("valid fixture");
    assert!(!targets.is_empty());
    let mut observations = Vec::new();
    for target in targets {
        let provider: Box<dyn MediaProvider> = match target.provider.as_str() {
            "jellyfin" => Box::new(crate::providers::jellyfin::JellyfinProvider::new(
                crate::api::JellyfinClient::new(),
                target.server_url,
                target.token.unwrap(),
                target.user_id.unwrap(),
            )),
            "navidrome" => Box::new(
                crate::providers::subsonic::SubsonicProvider::from_stored_config(
                    ProviderCredentials {
                        server_url: target.server_url,
                        credential: CredentialKind::Password {
                            username: target.username.unwrap(),
                            password: target.password.unwrap(),
                        },
                    },
                    true,
                    None,
                )
                .unwrap(),
            ),
            _ => panic!("unverified fixture provider"),
        };
        let initial = provider
            .read_feedback(&target.track_id)
            .await
            .expect("initial source verification");
        let mut values = vec![
            Preference::Like,
            Preference::Like,
            Preference::Neutral,
            Preference::Neutral,
        ];
        if initial.capabilities.dislike {
            values.extend([Preference::Dislike, Preference::Like]);
        }
        let outcome: anyhow::Result<()> = async {
            let db = crate::db::Database::memory()?;
            db.init_playback()?;
            let frozen = FeedbackTarget {
                session_id: "controlled-session".into(),
                logical_session_id: "controlled-session".into(),
                occurrence_id: "controlled-occurrence".into(),
                source: crate::playback::model::TrackSource {
                    server_id: "controlled-source".into(),
                    track_id: target.track_id.clone(),
                },
            };
            for value in &values {
                let id = uuid::Uuid::new_v4().to_string();
                db.record_feedback(&frozen, &initial.account_scope, *value, &id)?;
                let row = db.claim_feedback()?.expect("claimed operation");
                deliver_feedback(&db, &row, provider.as_ref(), &AtomicBool::new(false)).await?;
                anyhow::ensure!(
                    db.feedback_operation(&id)?.unwrap().status
                        == FeedbackOperationStatus::Confirmed,
                    "delivery not confirmed"
                );
                anyhow::ensure!(
                    provider.read_feedback(&target.track_id).await?.value == *value,
                    "persisted effect mismatch"
                );
            }
            Ok(())
        }
        .await;
        // Restore even when the controlled sequence fails, before asserting its outcome.
        let restoration = provider.set_feedback(&target.track_id, initial.value).await;
        let restored = provider.read_feedback(&target.track_id).await;
        assert!(
            restoration.is_ok() && restored.as_ref().is_ok_and(|r| r.value == initial.value),
            "controlled preference restoration failed"
        );
        assert!(
            outcome.is_ok(),
            "production delivery effect check failed after restoration"
        );
        observations.push(serde_json::json!({
            "provider": target.provider, "capabilities": initial.capabilities,
            "initialPreference": initial.value, "confirmedSequence": values, "originalRestored": true,
            "path": "production provider adapter + durable journal + delivery worker function + independent readback",
        }));
    }
    if let Ok(path) = std::env::var("HIFIMULE_FEEDBACK_LIVE_EVIDENCE") {
        let evidence = serde_json::json!({"schemaVersion":1,"recordedAt":chrono::Utc::now().to_rfc3339(),
            "os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"installedApplication":false,
            "observations":observations,"outcome":"passed"});
        std::fs::write(path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    }
}
