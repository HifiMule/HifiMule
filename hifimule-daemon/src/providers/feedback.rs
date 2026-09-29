//! Verified preference semantics. Operational state belongs to playback, not browse metadata.
use super::ProviderError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(test)]
mod live_tests;

pub const MAX_FEEDBACK_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Preference {
    Neutral,
    Like,
    Dislike,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackCapabilities {
    pub like: bool,
    pub dislike: bool,
    pub clear: bool,
}

impl FeedbackCapabilities {
    pub fn favorites() -> Self {
        Self {
            like: true,
            dislike: false,
            clear: true,
        }
    }
    pub fn ratings() -> Self {
        Self {
            like: true,
            dislike: true,
            clear: true,
        }
    }
    pub fn supports(self, value: Preference) -> bool {
        match value {
            Preference::Neutral => self.clear,
            Preference::Like => self.like,
            Preference::Dislike => self.dislike,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderFeedback {
    pub capabilities: FeedbackCapabilities,
    pub value: Preference,
    /// Hashed authenticated identity, used only inside the daemon to fence account changes.
    pub account_scope: String,
}

pub fn unsupported() -> ProviderError {
    ProviderError::UnsupportedCapability("feedback is not verified for this server/user".into())
}

pub fn malformed() -> ProviderError {
    ProviderError::Deserialization("authoritative feedback unavailable".into())
}

pub fn account_scope(user: &str) -> String {
    blake3::hash(user.as_bytes()).to_hex().to_string()
}

pub fn same_jellyfin_id(a: &str, b: &str) -> bool {
    match (uuid::Uuid::parse_str(a), uuid::Uuid::parse_str(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

pub fn jellyfin_value(raw: &Value, track: &str) -> Result<Preference, ProviderError> {
    if !raw
        .get("ItemId")
        .and_then(Value::as_str)
        .is_some_and(|id| same_jellyfin_id(id, track))
        || !raw.get("IsFavorite").is_some_and(Value::is_boolean)
    {
        return Err(malformed());
    }
    match raw.get("Likes") {
        None | Some(Value::Null) => Ok(Preference::Neutral),
        Some(Value::Bool(true)) => Ok(Preference::Like),
        Some(Value::Bool(false)) => Ok(Preference::Dislike),
        _ => Err(malformed()),
    }
}

pub fn navidrome_value(raw: &Value, track: &str) -> Result<Preference, ProviderError> {
    if raw.get("id").and_then(Value::as_str) != Some(track) {
        return Err(malformed());
    }
    match raw.get("starred") {
        None => Ok(Preference::Neutral),
        Some(Value::String(stamp)) if chrono::DateTime::parse_from_rfc3339(stamp).is_ok() => {
            Ok(Preference::Like)
        }
        _ => Err(malformed()),
    }
}

/// Deadline and byte limit apply to the entire response, including streaming bodies.
pub async fn request_json(request: reqwest::RequestBuilder) -> Result<Value, ProviderError> {
    tokio::time::timeout(std::time::Duration::from_secs(8), async {
        let mut response = request.send().await.map_err(|_| ProviderError::Http {
            status: None,
            message: "feedback transport failed".into(),
        })?;
        let status = response.status().as_u16();
        if !response.status().is_success() {
            return Err(ProviderError::Http {
                status: Some(status),
                message: "feedback request rejected".into(),
            });
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| malformed())? {
            if bytes.len().saturating_add(chunk.len()) > MAX_FEEDBACK_RESPONSE_BYTES {
                return Err(malformed());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| malformed())
    })
    .await
    .map_err(|_| ProviderError::Http {
        status: None,
        message: "feedback request timed out".into(),
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{CredentialKind, MediaProvider, ProviderCredentials};
    use mockito::{Matcher, Server};

    #[test]
    fn feedback_missing_is_neutral_only_inside_a_valid_authoritative_item() {
        assert_eq!(
            jellyfin_value(
                &serde_json::json!({"ItemId":"track","IsFavorite":false}),
                "track"
            )
            .unwrap(),
            Preference::Neutral
        );
        assert_eq!(
            jellyfin_value(
                &serde_json::json!({"ItemId":"track","IsFavorite":true,"Likes":false}),
                "track"
            )
            .unwrap(),
            Preference::Dislike
        );
        assert!(jellyfin_value(&serde_json::json!({}), "track").is_err());
        assert!(
            jellyfin_value(&serde_json::json!({"ItemId":"other","Likes":true}), "track").is_err()
        );
        assert_eq!(
            navidrome_value(&serde_json::json!({"id":"track","userRating":1}), "track").unwrap(),
            Preference::Neutral
        );
        assert!(
            navidrome_value(&serde_json::json!({"id":"track","starred":false}), "track").is_err()
        );
        assert!(!FeedbackCapabilities::favorites().supports(Preference::Dislike));
    }

    #[tokio::test]
    async fn feedback_jellyfin_writes_negative_rating_for_exact_user_and_track() {
        let mut server = Server::new_async().await;
        let user = "11111111-1111-4111-8111-111111111111";
        let _info = server
            .mock("GET", "/System/Info/Public")
            .with_body(r#"{"Version":"12.1.0"}"#)
            .create_async()
            .await;
        let _me = server
            .mock("GET", "/Users/Me")
            .with_body(format!(r#"{{"Id":"{user}"}}"#))
            .create_async()
            .await;
        let _read = server
            .mock("GET", "/UserItems/track/UserData")
            .match_query(Matcher::UrlEncoded("userId".into(), user.into()))
            .with_body(r#"{"ItemId":"track","IsFavorite":false}"#)
            .create_async()
            .await;
        let write = server
            .mock("POST", "/UserItems/track/Rating")
            .match_header("authorization", "MediaBrowser Token=\"secret\"")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("userId".into(), user.into()),
                Matcher::UrlEncoded("likes".into(), "false".into()),
            ]))
            .match_body("")
            .with_body(r#"{"ItemId":"track","Likes":false}"#)
            .expect(1)
            .create_async()
            .await;
        let p = crate::providers::jellyfin::JellyfinProvider::new(
            crate::api::JellyfinClient::new(),
            server.url(),
            "secret",
            user,
        );
        let before = p.read_feedback("track").await.unwrap();
        assert!(before.capabilities.dislike);
        p.set_feedback("track", Preference::Dislike).await.unwrap();
        write.assert_async().await;
    }

    #[tokio::test]
    async fn feedback_navidrome_rejects_dislike_without_a_mutation() {
        let mut server = Server::new_async().await;
        let _ping=server.mock("GET","/rest/ping.view").match_query(Matcher::Any)
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.64.2 (10114574)","openSubsonic":true}}"#).create_async().await;
        let _read=server.mock("GET","/rest/getSong.view").match_query(Matcher::UrlEncoded("id".into(),"track".into()))
            .with_body(r#"{"subsonic-response":{"status":"ok","version":"1.16.1","song":{"id":"track","starred":"2026-09-29T00:00:00Z","userRating":1}}}"#).create_async().await;
        let no_rating = server
            .mock("GET", "/rest/setRating.view")
            .match_query(Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        let no_unstar = server
            .mock("GET", "/rest/unstar.view")
            .match_query(Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        let p = crate::providers::subsonic::SubsonicProvider::from_stored_config(
            ProviderCredentials {
                server_url: server.url(),
                credential: CredentialKind::Password {
                    username: "user".into(),
                    password: "secret".into(),
                },
            },
            true,
            None,
        )
        .unwrap();
        assert_eq!(
            p.read_feedback("track").await.unwrap().value,
            Preference::Like
        );
        assert!(p.set_feedback("track", Preference::Dislike).await.is_err());
        no_rating.assert_async().await;
        no_unstar.assert_async().await;
    }

    #[tokio::test]
    async fn feedback_delivery_confirms_only_readback_and_never_replays_failures() {
        use crate::playback::feedback::*;
        use std::sync::atomic::AtomicBool;
        for (http, after, changed_account, shutdown, expected) in [
            (200, true, false, false, FeedbackOperationStatus::Confirmed),
            (200, false, false, false, FeedbackOperationStatus::Ambiguous),
            (503, true, false, false, FeedbackOperationStatus::Ambiguous),
            (403, true, false, false, FeedbackOperationStatus::Failed),
            (200, true, true, false, FeedbackOperationStatus::Failed),
            (200, true, false, true, FeedbackOperationStatus::Failed),
        ] {
            let mut server = Server::new_async().await;
            let user = "11111111-1111-4111-8111-111111111111";
            let _info = server
                .mock("GET", "/System/Info/Public")
                .with_body(r#"{"Version":"12.1.0"}"#)
                .create_async()
                .await;
            let _me = server
                .mock("GET", "/Users/Me")
                .with_body(format!(r#"{{"Id":"{user}"}}"#))
                .create_async()
                .await;
            let _read = server
                .mock("GET", "/UserItems/track/UserData")
                .match_query(Matcher::Any)
                .with_body(format!(
                    r#"{{"ItemId":"track","IsFavorite":false,"Likes":{after}}}"#
                ))
                .create_async()
                .await;
            let post = server
                .mock("POST", "/UserItems/track/Rating")
                .match_query(Matcher::Any)
                .with_status(http)
                .with_body(r#"{"ItemId":"track","Likes":true}"#)
                .expect(if changed_account || shutdown { 0 } else { 1 })
                .create_async()
                .await;
            let provider = crate::providers::jellyfin::JellyfinProvider::new(
                crate::api::JellyfinClient::new(),
                server.url(),
                "secret",
                user,
            );
            let verified = provider.read_feedback("track").await.unwrap();
            let db = crate::db::Database::memory().unwrap();
            db.init_playback().unwrap();
            let target = FeedbackTarget {
                session_id: "session".into(),
                logical_session_id: "session".into(),
                occurrence_id: "occ".into(),
                source: crate::playback::model::TrackSource {
                    server_id: "server".into(),
                    track_id: "track".into(),
                },
            };
            db.record_feedback(
                &target,
                if changed_account {
                    "other-account"
                } else {
                    &verified.account_scope
                },
                Preference::Like,
                "op",
            )
            .unwrap();
            let row = db.claim_feedback().unwrap().unwrap();
            deliver_feedback(&db, &row, &provider, &AtomicBool::new(shutdown))
                .await
                .unwrap();
            assert_eq!(
                db.feedback_operation("op").unwrap().unwrap().status,
                expected
            );
            db.recover_feedback().unwrap();
            assert!(db.claim_feedback().unwrap().is_none());
            post.assert_async().await;
        }
    }

    #[tokio::test]
    async fn feedback_unknown_jellyfin_version_or_user_does_not_offer_or_send_preferences() {
        for (version, user) in [("12.1.1", "expected"), ("12.1.0", "different")] {
            let mut server = Server::new_async().await;
            let _info = server
                .mock("GET", "/System/Info/Public")
                .with_body(format!(r#"{{"Version":"{version}"}}"#))
                .create_async()
                .await;
            let _me = server
                .mock("GET", "/Users/Me")
                .with_body(format!(r#"{{"Id":"{user}"}}"#))
                .create_async()
                .await;
            let no_write = server
                .mock("POST", "/UserItems/track/Rating")
                .match_query(Matcher::Any)
                .expect(0)
                .create_async()
                .await;
            let p = crate::providers::jellyfin::JellyfinProvider::new(
                crate::api::JellyfinClient::new(),
                server.url(),
                "secret",
                "expected",
            );
            assert!(matches!(
                p.read_feedback("track").await,
                Err(ProviderError::UnsupportedCapability(_))
            ));
            assert!(p.set_feedback("track", Preference::Like).await.is_err());
            no_write.assert_async().await;
        }
    }

    #[tokio::test]
    async fn feedback_body_limit_and_invalid_json_preserve_unknown() {
        let mut server = Server::new_async().await;
        for body in [
            "x".repeat(MAX_FEEDBACK_RESPONSE_BYTES + 1),
            "not-json".into(),
        ] {
            let response = server
                .mock("GET", "/read")
                .with_body(body)
                .expect(1)
                .create_async()
                .await;
            assert!(
                request_json(reqwest::Client::new().get(format!("{}/read", server.url())))
                    .await
                    .is_err()
            );
            response.assert_async().await;
            response.remove_async().await;
        }
    }

    #[tokio::test]
    async fn feedback_other_subsonic_versions_and_implementations_never_enable_a_mapping() {
        for (kind, version, open) in [
            ("navidrome", "0.64.1", true),
            ("subsonic", "0.64.2", true),
            ("navidrome", "0.64.2", false),
        ] {
            let mut server = Server::new_async().await;
            let _ping=server.mock("GET","/rest/ping.view").match_query(Matcher::Any)
                .with_body(serde_json::json!({"subsonic-response":{"status":"ok","version":"1.16.1","type":kind,"serverVersion":version,"openSubsonic":open}}).to_string()).create_async().await;
            let no_write = server
                .mock("GET", "/rest/star.view")
                .match_query(Matcher::Any)
                .expect(0)
                .create_async()
                .await;
            let p = crate::providers::subsonic::SubsonicProvider::from_stored_config(
                ProviderCredentials {
                    server_url: server.url(),
                    credential: CredentialKind::Password {
                        username: "user".into(),
                        password: "secret".into(),
                    },
                },
                true,
                None,
            )
            .unwrap();
            assert!(matches!(
                p.read_feedback("track").await,
                Err(ProviderError::UnsupportedCapability(_))
            ));
            assert!(p.set_feedback("track", Preference::Like).await.is_err());
            no_write.assert_async().await;
        }
    }
}
