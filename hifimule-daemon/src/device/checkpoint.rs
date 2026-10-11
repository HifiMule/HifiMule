use super::*;
use tokio::time::{Duration, Instant};

const TRACK_CHECKPOINT_UPDATES: usize = 1000;
const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(10 * 60);

pub(crate) struct ProvenanceIndex {
    first: HashMap<String, usize>,
    duplicates: HashMap<String, Vec<usize>>,
}

impl ProvenanceIndex {
    pub(crate) fn positions(&self, id: &str) -> impl Iterator<Item = usize> + '_ {
        self.first
            .get(id)
            .copied()
            .into_iter()
            .chain(self.duplicates.get(id).into_iter().flatten().copied())
    }
}

struct BufferedState {
    updates: usize,
    changed: bool,
    previous: Instant,
    failure: Option<String>,
}

struct SessionState {
    manager: Arc<DeviceManager>,
    device_id: String,
    path: PathBuf,
    observation: u64,
    io: Arc<dyn crate::device_io::DeviceIO>,
    buffered: tokio::sync::Mutex<BufferedState>,
}

/// Owns its timer and the original physical connection. Finalization joins the
/// timer before the caller may publish a clean manifest or completed operation.
pub(crate) struct SyncManifestSession {
    inner: Arc<SessionState>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    timer: Option<tokio::task::JoinHandle<()>>,
}

impl SyncManifestSession {
    pub(crate) async fn for_target(
        manager: Arc<DeviceManager>,
        target: &crate::sync::SyncTarget,
        operation: Option<(Arc<crate::sync::SyncOperationManager>, String)>,
    ) -> Result<Self> {
        let session = Self::new(manager, &target.manifest.device_id, &target.io, operation).await?;
        if session.inner.path != target.path || session.inner.observation != target.observation {
            anyhow::bail!("Sync connection changed since admission");
        }
        Ok(session)
    }
    pub(crate) async fn new(
        manager: Arc<DeviceManager>,
        device_id: &str,
        io: &Arc<dyn crate::device_io::DeviceIO>,
        operation: Option<(Arc<crate::sync::SyncOperationManager>, String)>,
    ) -> Result<Self> {
        let (path, observation) = {
            let state = manager.state.read().await;
            state
                .connected_devices
                .iter()
                .find_map(|(path, device)| {
                    (device.manifest.device_id == device_id && Arc::ptr_eq(io, &device.device_io))
                        .then(|| (path.clone(), device.observation_token))
                })
                .ok_or_else(|| anyhow::anyhow!("Sync connection changed"))?
        };
        let inner = Arc::new(SessionState {
            manager,
            device_id: device_id.into(),
            path,
            observation,
            io: Arc::clone(io),
            buffered: tokio::sync::Mutex::new(BufferedState {
                updates: 0,
                changed: false,
                previous: Instant::now(),
                failure: None,
            }),
        });
        let (stop, mut stopped) = tokio::sync::oneshot::channel();
        let timer_state = Arc::clone(&inner);
        let timer = tokio::spawn(async move {
            loop {
                let deadline = timer_state.buffered.lock().await.previous + CHECKPOINT_INTERVAL;
                tokio::select! {
                    _ = &mut stopped => break,
                    _ = tokio::time::sleep_until(deadline) => {
                        let mut buffered = timer_state.buffered.lock().await;
                        if buffered.previous.elapsed() < CHECKPOINT_INTERVAL { continue; }
                        if let Err(error) = timer_state.flush(&mut buffered, "timer").await {
                            buffered.failure.get_or_insert_with(||error.to_string());
                            if let Some((manager, id)) = &operation { let _ = manager.request_cancel(id).await; }
                            break;
                        }
                        // Avoid a busy loop when no metadata has changed.
                        if !buffered.changed { buffered.previous = Instant::now(); }
                    }
                }
            }
        });
        Ok(Self {
            inner,
            stop: Some(stop),
            timer: Some(timer),
        })
    }

    pub(crate) async fn stage<F>(&self, track_updates: usize, mutation: F) -> Result<()>
    where
        F: FnOnce(&mut DeviceManifest),
    {
        self.stage_checked(track_updates, |manifest| {
            mutation(manifest);
            Ok(true)
        })
        .await
    }

    /// Only IDs and positions are retained; track records stay in the manifest.
    pub(crate) async fn provenance_index(&self) -> Result<ProvenanceIndex> {
        let state = self.inner.manager.state.read().await;
        let connected = state
            .connected_devices
            .get(&self.inner.path)
            .ok_or_else(|| anyhow::anyhow!("Sync connection removed"))?;
        self.inner.validate(connected)?;
        let mut index = ProvenanceIndex {
            first: HashMap::with_capacity(connected.manifest.synced_items.len()),
            duplicates: HashMap::new(),
        };
        for (position, item) in connected.manifest.synced_items.iter().enumerate() {
            if let std::collections::hash_map::Entry::Vacant(entry) =
                index.first.entry(item.jellyfin_id.clone())
            {
                entry.insert(position);
            } else {
                index
                    .duplicates
                    .entry(item.jellyfin_id.clone())
                    .or_default()
                    .push(position);
            }
        }
        Ok(index)
    }

    pub(crate) async fn stage_provenance(
        &self,
        position: usize,
        id: &str,
        origin: bool,
    ) -> Result<()> {
        self.stage_checked(1, |manifest| {
            let item = manifest
                .synced_items
                .get_mut(position)
                .filter(|item| item.jellyfin_id == id)
                .ok_or_else(|| {
                    anyhow::anyhow!("Manifest track index changed during provenance update")
                })?;
            let changed = item.is_auto_fill != origin;
            item.is_auto_fill = origin;
            Ok(changed)
        })
        .await
    }

    async fn stage_checked<F>(&self, track_updates: usize, mutation: F) -> Result<()>
    where
        F: FnOnce(&mut DeviceManifest) -> Result<bool>,
    {
        let mut buffered = self.inner.buffered.lock().await;
        if let Some(error) = &buffered.failure {
            anyhow::bail!("Earlier checkpoint failed: {error}");
        }
        let lock = self
            .inner
            .manager
            .manifest_commit_lock(&self.inner.device_id);
        let commit = lock.lock().await;
        let mut state = self.inner.manager.state.write().await;
        let connected = state
            .connected_devices
            .get_mut(&self.inner.path)
            .ok_or_else(|| anyhow::anyhow!("Sync connection removed"))?;
        self.inner.validate(connected)?;
        if !mutation(&mut connected.manifest)? {
            return Ok(());
        }
        connected.manifest.dirty = true;
        buffered.changed = true;
        buffered.updates = buffered.updates.saturating_add(track_updates);
        drop(state);
        drop(commit);
        if buffered.updates >= TRACK_CHECKPOINT_UPDATES
            || buffered.previous.elapsed() >= CHECKPOINT_INTERVAL
        {
            let reason = if buffered.updates >= TRACK_CHECKPOINT_UPDATES {
                "count"
            } else {
                "timer"
            };
            if let Err(error) = self.inner.flush(&mut buffered, reason).await {
                buffered.failure = Some(error.to_string());
                return Err(error);
            }
        }
        Ok(())
    }

    pub(crate) async fn finish(mut self, outcome: &str) -> Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(timer) = self.timer.take() {
            timer.await?;
        }
        let mut buffered = self.inner.buffered.lock().await;
        crate::daemon_log!("[Sync] Manifest checkpoint finalization outcome={outcome}");
        let result = self.inner.flush(&mut buffered, "finalization").await;
        result?;
        if let Some(error) = &buffered.failure {
            anyhow::bail!("Checkpoint failed: {error}");
        }
        Ok(())
    }
}

impl Drop for SyncManifestSession {
    fn drop(&mut self) {
        if let Some(timer) = &self.timer {
            timer.abort();
        }
    }
}

impl SessionState {
    fn validate(&self, connected: &ConnectedDevice) -> Result<()> {
        if connected.observation_token != self.observation
            || connected.manifest.device_id != self.device_id
            || !Arc::ptr_eq(&connected.device_io, &self.io)
        {
            anyhow::bail!("Sync connection changed");
        }
        Ok(())
    }

    async fn flush(&self, buffered: &mut BufferedState, reason: &str) -> Result<()> {
        if !buffered.changed {
            return Ok(());
        }
        let started = Instant::now();
        let lock = self.manager.manifest_commit_lock(&self.device_id);
        let _commit = lock.lock().await;
        let prepared: Result<_> = async {
            let state = self.manager.state.read().await;
            let connected = state
                .connected_devices
                .get(&self.path)
                .ok_or_else(|| anyhow::anyhow!("Sync connection removed"))?;
            self.validate(connected)?;
            let bytes = serde_json::to_vec_pretty(&connected.manifest)?;
            let cache_manifest =
                (connected.device_class == DeviceClass::Mtp).then(|| connected.manifest.clone());
            Ok((bytes, cache_manifest))
        }
        .await;
        let (bytes, cache_manifest) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                crate::daemon_log!(
                    "[Sync] Manifest checkpoint start reason={reason} updates={} elapsed_ms={:.0} manifest_bytes=0",
                    buffered.updates,
                    buffered.previous.elapsed().as_secs_f64() * 1000.0
                );
                crate::daemon_log!(
                    "[Sync] Manifest checkpoint result reason={reason} updates={} elapsed_ms={:.0} manifest_bytes=0 duration_ms={:.0} result=error error={error}",
                    buffered.updates,
                    buffered.previous.elapsed().as_secs_f64() * 1000.0,
                    started.elapsed().as_secs_f64() * 1000.0
                );
                return Err(error);
            }
        };
        crate::daemon_log!(
            "[Sync] Manifest checkpoint start reason={reason} updates={} elapsed_ms={:.0} manifest_bytes={}",
            buffered.updates,
            buffered.previous.elapsed().as_secs_f64() * 1000.0,
            bytes.len()
        );
        let result: Result<()> = async {
            if let Some(manifest) = cache_manifest {
                let path = (self.manager.mtp_manifest_cache_path)(&self.device_id)?;
                persist_local_manifest_atomic(&path, &manifest).await?;
                // MTP's local cache is authoritative; retain the existing best-effort mirror.
                let _ = self.io.write_with_verify(".hifimule.json", &bytes).await;
            } else {
                self.io.write_with_verify(".hifimule.json", &bytes).await?;
            }
            Ok(())
        }
        .await;
        crate::daemon_log!(
            "[Sync] Manifest checkpoint result reason={reason} updates={} elapsed_ms={:.0} manifest_bytes={} duration_ms={:.0} result={} error={}",
            buffered.updates,
            buffered.previous.elapsed().as_secs_f64() * 1000.0,
            bytes.len(),
            started.elapsed().as_secs_f64() * 1000.0,
            if result.is_ok() { "ok" } else { "error" },
            result
                .as_ref()
                .err()
                .map(ToString::to_string)
                .unwrap_or_default()
        );
        if result.is_ok() {
            buffered.changed = false;
            buffered.updates = 0;
            buffered.previous = Instant::now();
        }
        result
    }
}
