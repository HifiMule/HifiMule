use mockito::{Matcher, Server};
use serde_json::Value;

const MANIFEST: &str = include_str!("fixtures/audiobookshelf/2.36.1/manifest.json");
const BOOK_PAGE: &str = include_str!("fixtures/audiobookshelf/2.36.1/books-page.json");
const MULTIPART_BOOK: &str = include_str!("fixtures/audiobookshelf/2.36.1/book-multipart.json");
const DELIVERY: &str = include_str!("fixtures/audiobookshelf/2.36.1/direct-delivery.json");
const PROGRESS: &str = include_str!("fixtures/audiobookshelf/2.36.1/progress-contract.json");
const TRANSCODE: &str = include_str!("fixtures/audiobookshelf/2.36.1/transcode-delivery.json");
const CHANGED_REMOVED: &str =
    include_str!("fixtures/audiobookshelf/2.36.1/changed-and-removed-item.json");
const PROGRESS_CONFLICT: &str =
    include_str!("fixtures/audiobookshelf/2.36.1/progress-conflict.json");
const PROGRESS_FAILURE: &str = include_str!("fixtures/audiobookshelf/2.36.1/progress-failure.json");
const AUTH_EXPIRY: &str = include_str!("fixtures/audiobookshelf/2.36.1/auth-expiry-refresh.json");
const AUTH_RATE_LIMIT: &str = include_str!("fixtures/audiobookshelf/2.36.1/auth-rate-limit.json");
const DUPLICATE_IDENTITY: &str =
    include_str!("fixtures/audiobookshelf/2.36.1/duplicate-title-identity.json");
const MISSING_LIBRARY: &str = include_str!("fixtures/audiobookshelf/2.36.1/missing-library.json");
const PROGRESS_IDEMPOTENCY: &str =
    include_str!("fixtures/audiobookshelf/2.36.1/progress-idempotency.json");
const PROGRESS_EMPTY: &str =
    include_str!("fixtures/audiobookshelf/2.36.1/progress-empty-payload.json");
const MIME_MISMATCH: &str =
    include_str!("fixtures/audiobookshelf/2.36.1/delivery-mime-mismatch.json");
const PROXY_500: &str = include_str!("fixtures/audiobookshelf/2.36.1/catalogue-proxy-500.json");

#[test]
fn manifest_is_versioned_complete_and_alias_only() {
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    assert_eq!(manifest["schemaVersion"], 1);
    assert_eq!(manifest["serverVersion"], "2.36.1");
    let cases = manifest["cases"].as_array().unwrap();
    for required in [
        "auth-invalid",
        "libraries-roles",
        "books-page-boundaries",
        "podcasts-page-boundaries",
        "book-multipart-order",
        "book-single-missing-credits",
        "artwork-and-identity",
        "missing-item",
        "delivery-unavailable",
        "progress-contract",
    ] {
        assert!(
            cases.iter().any(|case| case["id"] == required),
            "missing {required}"
        );
    }
    assert!(
        manifest["redaction"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule.as_str().unwrap().contains("fixture-local aliases"))
    );
}

#[test]
fn fixture_pagination_and_ordering_invariants_are_explicit() {
    let page: Value = serde_json::from_str(BOOK_PAGE).unwrap();
    assert_eq!(page["page"], 0);
    assert_eq!(page["limit"], 1);
    assert!(page["total"].as_u64().unwrap() >= 1);
    assert!(page["results"].as_array().unwrap().len() == 1);
    assert!(page["finalPage"]["results"].as_array().unwrap().is_empty());

    let book: Value = serde_json::from_str(MULTIPART_BOOK).unwrap();
    let files = book["media"]["audioFiles"].as_array().unwrap();
    let chapters = book["media"]["chapters"].as_array().unwrap();
    assert_eq!(files.len(), 10);
    assert_eq!(files[0]["index"], 1);
    assert_eq!(files[1]["index"], 2);
    assert_eq!(chapters[0]["start"], 0);
    assert_eq!(chapters[0]["end"], chapters[1]["start"]);
    assert!(
        book["media"]["metadata"]["authors"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
    assert!(
        book["media"]["metadata"]["narrators"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
}

#[test]
fn delivery_and_book_progress_are_daemon_private_and_alias_only() {
    let delivery: Value = serde_json::from_str(DELIVERY).unwrap();
    assert_eq!(delivery["delivery"]["status"], 206);
    assert_eq!(delivery["delivery"]["contentUrl"], "server-relative");
    assert_eq!(delivery["cleanup"]["status"], 200);

    let progress: Value = serde_json::from_str(PROGRESS).unwrap();
    assert_eq!(progress["wholeItemOffset"]["field"], "currentTime");
    assert_eq!(progress["wholeItemOffset"]["unit"], "seconds");
    assert_eq!(
        progress["cleanup"]["template"],
        "/api/me/progress/{mediaProgressAlias}"
    );
    assert_eq!(progress["status"], "verified-book-and-podcast");
    assert_eq!(progress["completion"]["isFinished"], true);

    let transcode: Value = serde_json::from_str(TRANSCODE).unwrap();
    assert_eq!(transcode["session"]["playMethod"], 2);
    assert_eq!(transcode["scope"], "playback-session-only");
    assert_eq!(transcode["downloadTranscode"], "unsupported");

    let changed_removed: Value = serde_json::from_str(CHANGED_REMOVED).unwrap();
    assert_eq!(changed_removed["change"]["status"], 200);
    assert_eq!(changed_removed["remove"]["hardDelete"], false);
    assert_eq!(changed_removed["afterRemoval"]["status"], 404);

    let conflict: Value = serde_json::from_str(PROGRESS_CONFLICT).unwrap();
    assert_eq!(conflict["status"], 409);
    assert_eq!(conflict["classification"], "conflict");
    assert_eq!(
        conflict["observation"],
        "unsupported-for-progress-on-v2.36.1"
    );

    let failure: Value = serde_json::from_str(PROGRESS_FAILURE).unwrap();
    assert_eq!(failure["status"], 500);
    assert_eq!(failure["classification"], "server-failure");
    assert_eq!(failure["body"], "redacted");

    let expiry: Value = serde_json::from_str(AUTH_EXPIRY).unwrap();
    assert_eq!(expiry["accessExpiry"]["protectedRequestStatus"], 401);
    assert_eq!(expiry["refresh"]["postRefreshProtectedRequestStatus"], 200);

    let rate_limit: Value = serde_json::from_str(AUTH_RATE_LIMIT).unwrap();
    assert_eq!(rate_limit["invalidLoginStatuses"][1], 429);
    assert_eq!(rate_limit["classification"], "rate-limited");

    let duplicates: Value = serde_json::from_str(DUPLICATE_IDENTITY).unwrap();
    assert_eq!(duplicates["sameDisplayTitleItems"], 2);
    assert_eq!(duplicates["stableIdentity"]["libraryItemIdsDistinct"], true);
    assert_eq!(duplicates["stableIdentity"]["titleIsIdentity"], false);

    let missing_library: Value = serde_json::from_str(MISSING_LIBRARY).unwrap();
    assert_eq!(missing_library["status"], 404);
    assert_eq!(missing_library["classification"], "not-found");

    let idempotency: Value = serde_json::from_str(PROGRESS_IDEMPOTENCY).unwrap();
    assert_eq!(idempotency["statuses"], serde_json::json!([200, 200]));
    assert_eq!(idempotency["classification"], "idempotent");

    let empty_progress: Value = serde_json::from_str(PROGRESS_EMPTY).unwrap();
    assert_eq!(empty_progress["status"], 200);
    assert_eq!(empty_progress["classification"], "accepted-defaulting");

    let mismatch: Value = serde_json::from_str(MIME_MISMATCH).unwrap();
    assert_eq!(mismatch["fallbackPlayMethod"], 2);
    assert_eq!(mismatch["classification"], "transcode-fallback");

    let proxy_500: Value = serde_json::from_str(PROXY_500).unwrap();
    assert_eq!(proxy_500["status"], 500);
    assert_eq!(proxy_500["classification"], "server-failure");
    assert_eq!(proxy_500["body"], "redacted");
}

#[tokio::test]
async fn contract_request_shape_uses_bearer_header_without_a_secret() {
    let mut server = Server::new_async().await;
    let endpoint = server
        .mock("GET", "/api/libraries/library-book-1/items")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("page".into(), "0".into()),
            Matcher::UrlEncoded("limit".into(), "1".into()),
        ]))
        .match_header("authorization", "Bearer [fixture-token]")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(BOOK_PAGE)
        .expect(1)
        .create_async()
        .await;
    let response = reqwest::Client::new()
        .get(format!(
            "{}/api/libraries/library-book-1/items?page=0&limit=1",
            server.url()
        ))
        .bearer_auth("[fixture-token]")
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    endpoint.assert_async().await;
}
