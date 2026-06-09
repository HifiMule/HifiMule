//! Multi-server runtime state (Story 2.11).
//!
//! Replaces the former single `AppState.provider`/`server_type`/`server_version`
//! triple. The manager owns the list of configured servers, the currently
//! selected server id, and a lazily-populated provider cache keyed by server
//! UUID. Providers are connected on first use only (`get_provider`) so idle RAM
//! stays low (AC14) — never eagerly for every configured server.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::api::{CredentialManager, JellyfinClient};
use crate::db::{Database, ServerConfig};
use crate::providers::{CredentialKind, MediaProvider, ProviderCredentials, ProviderError};

/// In-memory view of one configured server row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerRecord {
    pub id: String,
    pub url: String,
    pub server_type: String,
    pub username: String,
    pub server_version: Option<String>,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub selected: bool,
    /// Deterministic portable identity (Story 2.13) — used for manifest/basket
    /// tagging and sync routing. Resolved back to `id` for provider lookup.
    pub server_id: Option<String>,
    pub server_reported_id: Option<String>,
}

impl From<ServerConfig> for ServerRecord {
    fn from(c: ServerConfig) -> Self {
        ServerRecord {
            id: c.id,
            url: c.url,
            server_type: c.server_type,
            username: c.username,
            server_version: c.server_version,
            name: c.name,
            icon: c.icon,
            selected: c.selected,
            server_id: c.server_id,
            server_reported_id: c.server_reported_id,
        }
    }
}

#[derive(Default)]
pub struct ServerManager {
    pub servers: Vec<ServerRecord>,
    pub selected_server_id: Option<String>,
    /// Lazy provider cache keyed by server UUID.
    pub providers: HashMap<String, Arc<dyn MediaProvider>>,
}

impl ServerManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads all server rows from the DB into memory and sets the selected id
    /// from the `selected = 1` row. Does NOT connect any provider (AC14).
    pub fn load_from_db(&mut self, db: &Database) {
        let servers: Vec<ServerRecord> = db
            .list_servers()
            .unwrap_or_default()
            .into_iter()
            .map(ServerRecord::from)
            .collect();
        self.selected_server_id = servers.iter().find(|s| s.selected).map(|s| s.id.clone());
        self.servers = servers;
    }

    pub fn selected_record(&self) -> Option<&ServerRecord> {
        let id = self.selected_server_id.as_deref()?;
        self.servers.iter().find(|s| s.id == id)
    }

    /// Test seam replacing the former `*state.provider.write() = Some(p)` pattern:
    /// installs a single selected server backed by `provider`.
    #[cfg(test)]
    pub fn set_test_provider(&mut self, provider: Arc<dyn MediaProvider>) {
        let id = "test-server".to_string();
        let server_type = crate::providers::server_type_slug(provider.server_type())
            .unwrap_or("jellyfin")
            .to_string();
        let server_version = provider.server_version().map(str::to_string);
        self.servers = vec![ServerRecord {
            id: id.clone(),
            url: String::new(),
            server_type,
            username: String::new(),
            server_version,
            name: None,
            icon: None,
            selected: true,
            // In tests the portable id mirrors the local id (1:1 per machine).
            server_id: Some(id.clone()),
            server_reported_id: None,
        }];
        self.selected_server_id = Some(id.clone());
        self.providers.clear();
        self.providers.insert(id, provider);
    }
}

/// Builds a provider from a server's stored credentials. Construction only — no
/// network round-trip — so it is safe to call lazily on first selection/use.
async fn connect_provider_for(
    record: &ServerRecord,
) -> Result<Arc<dyn MediaProvider>, ProviderError> {
    let creds = CredentialManager::get_server_credential(&record.id)
        .map_err(|e| ProviderError::Auth(e.to_string()))?;

    match record.server_type.as_str() {
        "jellyfin" => {
            let user_id = creds
                .user_id
                .clone()
                .unwrap_or_else(|| record.username.clone());
            Ok(Arc::new(
                crate::providers::jellyfin::JellyfinProvider::new_with_version(
                    JellyfinClient::new(),
                    record.url.clone(),
                    creds.token_or_password.clone(),
                    user_id,
                    record.server_version.clone(),
                ),
            ) as Arc<dyn MediaProvider>)
        }
        "subsonic" | "openSubsonic" => {
            let credentials = ProviderCredentials {
                server_url: record.url.clone(),
                credential: CredentialKind::Password {
                    username: record.username.clone(),
                    password: creds.token_or_password.clone(),
                },
            };
            let open_subsonic = record.server_type == "openSubsonic";
            let provider = crate::providers::subsonic::SubsonicProvider::from_stored_config(
                credentials,
                open_subsonic,
                record.server_version.clone(),
            )
            .map_err(|e| ProviderError::Auth(e.to_string()))?;
            Ok(Arc::new(provider) as Arc<dyn MediaProvider>)
        }
        other => Err(ProviderError::UnsupportedCapability(format!(
            "Unknown server type: {other}"
        ))),
    }
}

/// Returns the provider for `id`, lazily connecting and caching it on first use.
pub async fn get_provider(
    manager: &Arc<RwLock<ServerManager>>,
    db: &Database,
    id: &str,
) -> Result<Arc<dyn MediaProvider>, ProviderError> {
    if let Some(provider) = manager.read().await.providers.get(id).cloned() {
        return Ok(provider);
    }
    // Prefer the in-memory record (carries the latest version); fall back to DB.
    let record = {
        let guard = manager.read().await;
        guard.servers.iter().find(|s| s.id == id).cloned()
    };
    let record = match record {
        Some(r) => r,
        None => db
            .get_server(id)
            .ok()
            .flatten()
            .map(ServerRecord::from)
            .ok_or_else(|| ProviderError::Auth(format!("Unknown server: {id}")))?,
    };
    let provider = connect_provider_for(&record).await?;
    // Double-checked insert: a concurrent caller may have connected and cached the
    // same id while we were awaiting `connect_provider_for`. Converge on whichever
    // instance landed first so every caller shares one provider Arc (avoids
    // duplicate per-provider session state from a check-then-insert race).
    let mut guard = manager.write().await;
    if let Some(existing) = guard.providers.get(id).cloned() {
        return Ok(existing);
    }
    guard.providers.insert(id.to_string(), provider.clone());
    Ok(provider)
}

/// Resolves a PORTABLE `server_id` (Story 2.13) to its machine-local id, then
/// delegates to [`get_provider`] (which keeps the single provider cache keyed by
/// local id). On a single machine `server_id ↔ local id` is 1:1 (upsert-by-URL
/// prevents duplicate rows), so exactly one record matches. As a resilience
/// fallback — e.g. a not-yet-reconciled tag that already equals a local id — an
/// id matching no portable `server_id` but matching a local `id` is used directly.
pub async fn get_provider_by_server_id(
    manager: &Arc<RwLock<ServerManager>>,
    db: &Database,
    server_id: &str,
) -> Result<Arc<dyn MediaProvider>, ProviderError> {
    let local_id = {
        let guard = manager.read().await;
        guard
            .servers
            .iter()
            .find(|s| s.server_id.as_deref() == Some(server_id))
            .or_else(|| guard.servers.iter().find(|s| s.id == server_id))
            .map(|s| s.id.clone())
    };
    let local_id = match local_id {
        Some(id) => id,
        // Fall back to the DB in case the in-memory manager is stale.
        None => db
            .list_servers()
            .unwrap_or_default()
            .into_iter()
            .find(|s| s.server_id.as_deref() == Some(server_id) || s.id == server_id)
            .map(|s| s.id)
            .ok_or_else(|| ProviderError::Auth(format!("Unknown server: {server_id}")))?,
    };
    get_provider(manager, db, &local_id).await
}

/// Returns the currently selected server's provider, or `None` if no server is
/// selected. Lazily connects on first use.
pub async fn selected_provider(
    manager: &Arc<RwLock<ServerManager>>,
    db: &Database,
) -> Option<Result<Arc<dyn MediaProvider>, ProviderError>> {
    let id = manager.read().await.selected_server_id.clone()?;
    Some(get_provider(manager, db, &id).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(db: &Database, url: &str, kind: &str, user: &str) -> String {
        db.upsert_server(url, kind, user, None, None, None, None).unwrap()
    }

    // load_from_db reflects the DB's rows and the `selected = 1` row (AC12/AC14:
    // no eager provider connect — `providers` stays empty).
    #[test]
    fn load_from_db_sets_servers_and_selection() {
        let db = Database::memory().unwrap();
        let id1 = seed(&db, "http://a.example", "jellyfin", "u1");
        let _id2 = seed(&db, "http://b.example", "openSubsonic", "u2");

        let mut mgr = ServerManager::new();
        mgr.load_from_db(&db);

        assert_eq!(mgr.servers.len(), 2);
        // First-ever server auto-selected by the DB layer.
        assert_eq!(mgr.selected_server_id.as_deref(), Some(id1.as_str()));
        assert_eq!(mgr.selected_record().unwrap().id, id1);
        assert!(mgr.providers.is_empty(), "no eager connects (AC14)");

        // Switch selection in the DB and reload.
        db.set_selected(&_id2).unwrap();
        mgr.load_from_db(&db);
        assert_eq!(mgr.selected_server_id.as_deref(), Some(_id2.as_str()));
    }

    // get_provider lazily builds a Subsonic provider from stored credentials and
    // caches it; a second call returns the cached instance.
    #[tokio::test]
    async fn get_provider_lazily_connects_and_caches_subsonic() {
        let _lock = crate::api::credential_test_lock();
        let db = Database::memory().unwrap();
        let id = seed(&db, "http://sub.example", "openSubsonic", "u");
        CredentialManager::save_server_credential(
            &id,
            &crate::api::ServerCredentials {
                token_or_password: "pw".to_string(),
                user_id: None,
            },
        )
        .unwrap();

        let mgr = Arc::new(RwLock::new(ServerManager::new()));
        mgr.write().await.load_from_db(&db);
        assert!(mgr.read().await.providers.is_empty());

        let provider = get_provider(&mgr, &db, &id).await.expect("connects");
        assert_eq!(
            provider.server_type(),
            crate::providers::ServerType::OpenSubsonic
        );
        assert!(mgr.read().await.providers.contains_key(&id), "cached");

        // Cached path returns the same Arc.
        let again = get_provider(&mgr, &db, &id).await.unwrap();
        assert!(Arc::ptr_eq(&provider, &again));
    }

    // Removing the selected server then reloading clears the provider cache entry
    // (the rpc layer evicts on remove; here we verify reload picks up reselection).
    #[tokio::test]
    async fn reload_after_remove_updates_selection() {
        let _lock = crate::api::credential_test_lock();
        let db = Database::memory().unwrap();
        let id1 = seed(&db, "http://a.example", "openSubsonic", "u1");
        let id2 = seed(&db, "http://b.example", "openSubsonic", "u2");

        let mgr = Arc::new(RwLock::new(ServerManager::new()));
        mgr.write().await.load_from_db(&db);
        assert_eq!(
            mgr.read().await.selected_server_id.as_deref(),
            Some(id1.as_str())
        );

        // Remove the selected server and reselect the remaining one (AC8 flow).
        db.remove_server(&id1).unwrap();
        db.set_selected(&id2).unwrap();
        mgr.write().await.providers.remove(&id1);
        mgr.write().await.load_from_db(&db);

        assert_eq!(mgr.read().await.servers.len(), 1);
        assert_eq!(
            mgr.read().await.selected_server_id.as_deref(),
            Some(id2.as_str())
        );
    }

    // Story 2.13: get_provider_by_server_id maps a PORTABLE id → local id and reuses
    // the same local-id-keyed provider cache.
    #[tokio::test]
    async fn get_provider_by_server_id_resolves_portable_to_local() {
        let _lock = crate::api::credential_test_lock();
        let db = Database::memory().unwrap();
        let local_id = seed(&db, "http://sub.example", "openSubsonic", "u");
        let portable = db
            .get_server(&local_id)
            .unwrap()
            .unwrap()
            .server_id
            .expect("portable id derived on upsert");
        assert_ne!(portable, local_id, "portable id differs from machine-local id");
        CredentialManager::save_server_credential(
            &local_id,
            &crate::api::ServerCredentials {
                token_or_password: "pw".to_string(),
                user_id: None,
            },
        )
        .unwrap();

        let mgr = Arc::new(RwLock::new(ServerManager::new()));
        mgr.write().await.load_from_db(&db);

        // Resolve by the PORTABLE id; cache is keyed by the LOCAL id.
        let provider = get_provider_by_server_id(&mgr, &db, &portable)
            .await
            .expect("resolves portable id");
        assert_eq!(
            provider.server_type(),
            crate::providers::ServerType::OpenSubsonic
        );
        assert!(
            mgr.read().await.providers.contains_key(&local_id),
            "cache keyed by local id"
        );

        // Unknown portable id errors cleanly.
        assert!(get_provider_by_server_id(&mgr, &db, "no-such-portable")
            .await
            .is_err());
    }
}
