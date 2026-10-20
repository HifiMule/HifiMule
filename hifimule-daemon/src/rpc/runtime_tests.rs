// These tests put the production authenticated router on its own current-thread
// runtime. The client and blocking gates run elsewhere, so a starved RPC runtime
// cannot conceal itself by also preventing its own timeout from firing.
struct RuntimeBlock {
    stage: &'static str,
    entered: std::sync::mpsc::Sender<String>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    panic_after_release: bool,
    used: std::sync::atomic::AtomicBool,
}

impl RuntimeBlock {
    fn enter(&self, stage: &str) {
        if self.stage != stage || self.used.swap(true, AtomicOrdering::AcqRel) {
            return;
        }
        self.entered
            .send(std::thread::current().name().unwrap_or("unnamed").into())
            .unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(15))
            .unwrap();
        assert!(!self.panic_after_release, "controlled sync handler panic");
    }
}

struct ReleaseRuntimeBlock(Option<std::sync::mpsc::Sender<()>>);
impl ReleaseRuntimeBlock {
    fn release(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
impl Drop for ReleaseRuntimeBlock {
    fn drop(&mut self) {
        self.release();
    }
}

fn runtime_block(
    stage: &'static str,
    panic_after_release: bool,
) -> (
    Arc<RuntimeBlock>,
    std::sync::mpsc::Receiver<String>,
    ReleaseRuntimeBlock,
) {
    let (entered, receiver) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    (
        Arc::new(RuntimeBlock {
            stage,
            entered,
            release: std::sync::Mutex::new(released),
            panic_after_release,
            used: std::sync::atomic::AtomicBool::new(false),
        }),
        receiver,
        ReleaseRuntimeBlock(Some(release)),
    )
}

struct TestCoreRuntime {
    handle: tokio::runtime::Handle,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TestCoreRuntime {
    fn new() -> Self {
        let (ready, receiver) = std::sync::mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("sync-core-test")
                .enable_all()
                .build()
                .unwrap();
            ready.send(runtime.handle().clone()).unwrap();
            runtime.block_on(async {
                let _ = stopped.await;
            });
        });
        Self {
            handle: receiver.recv().unwrap(),
            stop: Some(stop),
            thread: Some(thread),
        }
    }
}
impl Drop for TestCoreRuntime {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

struct TestRpcServer {
    url: String,
    client: reqwest::Client,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TestRpcServer {
    async fn new(state: Arc<AppState>, core: tokio::runtime::Handle) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (ready, received) = tokio::sync::oneshot::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("sync-rpc-test".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                runtime.block_on(async {
                    let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                    let router = rpc_router(state, Arc::new("runtime-test-token".into()), core);
                    ready.send(()).unwrap();
                    tokio::select! {
                        _ = stopped => {},
                        result = axum::serve(listener, router).into_future() => result.unwrap(),
                    }
                });
            })
            .unwrap();
        received.await.unwrap();
        Self {
            url,
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap(),
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    fn request(&self, method: &str, params: Value) -> reqwest::RequestBuilder {
        self.client
            .post(&self.url)
            .bearer_auth("runtime-test-token")
            .json(&json!({"jsonrpc":"2.0","id":"runtime-request","method":method,"params":params}))
    }

    async fn call(&self, method: &str, params: Value) -> Value {
        self.request(method, params)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
}
impl Drop for TestRpcServer {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

async fn blocked_runtime_thread(receiver: std::sync::mpsc::Receiver<String>) -> String {
    tokio::task::spawn_blocking(move || receiver.recv_timeout(std::time::Duration::from_secs(3)))
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn sync_runtime_isolation_keeps_authenticated_controls_responsive_during_preparation() {
    let _credentials = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let core = TestCoreRuntime::new();
    let server = TestRpcServer::new(state.clone(), core.handle.clone()).await;
    let (block, entered, mut release) = runtime_block("album", false);
    let mut provider = FakeBrowseProvider::new(vec![], vec![]);
    Arc::get_mut(&mut provider).unwrap().runtime_block = Some(block);
    state
        .server_manager
        .write()
        .await
        .set_test_provider(provider);
    let request = server
        .request("sync_prepare", json!({"itemIds":["held-album"]}))
        .timeout(std::time::Duration::from_secs(15));
    let pending =
        tokio::spawn(async move { request.send().await.unwrap().json::<Value>().await.unwrap() });
    assert_eq!(blocked_runtime_thread(entered).await, "sync-core-test");
    let unauthenticated = server
        .client
        .post(&server.url)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"daemon.health"}))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), http::StatusCode::UNAUTHORIZED);
    let health = server.call("daemon.health", json!({})).await;
    assert!(health["error"].is_null(), "{health}");
    let status = server
        .call("sync_get_operation_status", json!({"operationId":"absent"}))
        .await;
    assert_eq!(status["error"]["code"], ERR_INVALID_PARAMS);
    let cancelled = server.call("sync_cancel", json!({})).await;
    assert!(cancelled["error"].is_null(), "{cancelled}");
    assert!(!pending.is_finished());
    release.release();
    let response = pending.await.unwrap();
    assert_eq!(response["id"], "runtime-request");
    assert_eq!(response["error"]["code"], ERR_SYNC_CANCELLED);
    assert!(
        state
            .sync_operation_manager
            .pending_plan
            .lock()
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn sync_runtime_isolation_detached_execution_uses_core_and_retains_pipeline_owner() {
    let _credentials = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let core = TestCoreRuntime::new();
    let server = TestRpcServer::new(state.clone(), core.handle.clone()).await;
    let (block, entered, mut release) = runtime_block("download", false);
    let mut provider = FakeBrowseProvider::new(vec![], vec![]);
    Arc::get_mut(&mut provider).unwrap().runtime_block = Some(block);
    state
        .server_manager
        .write()
        .await
        .set_test_provider(provider);
    let response = server.call("sync_execute", json!({"delta": {
        "adds":[{"jellyfinId":"held-track","name":"Track","sizeBytes":100,"providerSuffix":"mp3"}],
        "deletes":[],"idChanges":[],"unchanged":0,"playlists":[]
    }})).await;
    assert!(response["error"].is_null(), "{response}");
    let operation = response["result"]["operationId"].as_str().unwrap();
    assert_eq!(blocked_runtime_thread(entered).await, "sync-core-test");
    assert!(state.sync_operation_manager.try_start_pipeline().is_none());
    let health = server.call("daemon.health", json!({})).await;
    assert!(health["error"].is_null(), "{health}");
    let status = server
        .call(
            "sync_get_operation_status",
            json!({"operationId":operation}),
        )
        .await;
    assert!(status["error"].is_null(), "{status}");
    let cancel = server
        .call("sync_cancel", json!({"operationId":operation}))
        .await;
    assert!(cancel["error"].is_null(), "{cancel}");
    release.release();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while state.sync_operation_manager.try_start_pipeline().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let finished = state
        .sync_operation_manager
        .get_operation(operation)
        .await
        .unwrap();
    assert_eq!(finished.status, crate::sync::SyncStatus::Cancelled);
}

#[tokio::test]
async fn sync_runtime_isolation_health_survives_core_blocking_pool_drain() {
    let state = make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
    let core = TestCoreRuntime::new();
    let handle = core.handle.clone();
    let server = TestRpcServer::new(state.clone(), handle.clone()).await;
    let (block, entered, mut release) = runtime_block("teardown", false);
    handle.spawn_blocking(move || block.enter("teardown"));
    blocked_runtime_thread(entered).await;
    state.sync_operation_manager.begin_shutdown_fence();
    state.sync_operation_manager.commit_shutdown().await;
    let teardown = std::thread::spawn(move || drop(core));
    // A cancelled spawn proves the async core has stopped. Its blocking pool
    // remains held until release, matching production runtime-drop ordering.
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while handle.spawn(async {}).await.is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!teardown.is_finished());
    let health = server.call("daemon.health", json!({})).await;
    assert_eq!(health["result"]["data"]["status"], "stopping");
    assert!(health["error"].is_null());
    release.release();
    teardown.join().unwrap();
}

#[tokio::test]
async fn sync_runtime_isolation_precommit_fence_rejects_all_heavy_routes_on_active_core() {
    let state = make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
    let core = TestCoreRuntime::new();
    let server = TestRpcServer::new(state.clone(), core.handle.clone()).await;
    core.handle.spawn(async {}).await.unwrap();
    state.sync_operation_manager.begin_shutdown_fence();
    assert!(!state.sync_operation_manager.is_shutdown_committed());
    for method in [
        "sync_calculate_delta",
        "sync_prepare",
        "sync_detect_changes",
        "sync_execute",
        "basket.estimate",
        "basket.autoFill",
    ] {
        let response = server.call(method, json!({})).await;
        assert_eq!(response["id"], "runtime-request", "{method}");
        assert_eq!(
            response["error"]["data"]["errorCode"], "DAEMON_STOPPED",
            "{method}"
        );
        assert!(response["result"].is_null(), "{method}");
    }
    assert!(server.call("daemon.health", json!({})).await["error"].is_null());
    assert!(
        state
            .sync_operation_manager
            .get_all_operations()
            .await
            .is_empty()
    );
    assert_eq!(
        state
            .sync_operation_manager
            .shutdown_snapshot()
            .await
            .unwrap()
            .pending_mutation_count,
        0
    );
}

#[tokio::test]
async fn sync_runtime_isolation_shutdown_drain_tracks_a_blocked_heavy_read() {
    let _credentials = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let core = TestCoreRuntime::new();
    let server = TestRpcServer::new(state.clone(), core.handle.clone()).await;
    let (block, entered, mut release) = runtime_block("changes", false);
    let mut provider = FakeBrowseProvider::new(vec![], vec![]);
    Arc::get_mut(&mut provider).unwrap().runtime_block = Some(block);
    state
        .server_manager
        .write()
        .await
        .set_test_provider(provider);
    let request = server
        .request("sync_detect_changes", json!({"syncToken":null}))
        .timeout(std::time::Duration::from_secs(15));
    let pending =
        tokio::spawn(async move { request.send().await.unwrap().json::<Value>().await.unwrap() });
    assert_eq!(blocked_runtime_thread(entered).await, "sync-core-test");
    state.sync_operation_manager.begin_shutdown_fence();
    state.sync_operation_manager.commit_shutdown().await;
    assert_eq!(
        state
            .sync_operation_manager
            .shutdown_snapshot()
            .await
            .unwrap()
            .pending_mutation_count,
        1
    );
    let drain = state.sync_operation_manager.wait_for_shutdown_drain();
    tokio::pin!(drain);
    assert!(futures::poll!(drain.as_mut()).is_pending());
    let health = server.call("daemon.health", json!({})).await;
    assert!(health["error"].is_null());
    assert_eq!(health["result"]["data"]["status"], "stopping");
    release.release();
    let response = pending.await.unwrap();
    assert_eq!(response["id"], "runtime-request");
    tokio::time::timeout(std::time::Duration::from_secs(2), drain)
        .await
        .unwrap();
    assert_eq!(
        state
            .sync_operation_manager
            .shutdown_snapshot()
            .await
            .unwrap()
            .pending_mutation_count,
        0
    );
}

#[tokio::test]
async fn sync_runtime_isolation_closed_core_preserves_ids_and_rejects_all_heavy_routes() {
    let state = make_test_state(Arc::new(crate::db::Database::memory().unwrap()));
    let core = TestCoreRuntime::new();
    let handle = core.handle.clone();
    drop(core);
    let server = TestRpcServer::new(state.clone(), handle).await;
    for method in [
        "sync_calculate_delta",
        "sync_prepare",
        "sync_detect_changes",
        "sync_execute",
        "basket.estimate",
        "basket.autoFill",
    ] {
        let response = server.call(method, json!({})).await;
        assert_eq!(response["id"], "runtime-request", "{method}");
        assert!(response["result"].is_null(), "{method}");
        assert_eq!(
            response["error"]["data"]["errorCode"], "DAEMON_STOPPED",
            "{method}"
        );
    }
    assert!(server.call("daemon.health", json!({})).await["error"].is_null());
    state.sync_operation_manager.begin_shutdown_fence();
    state.sync_operation_manager.commit_shutdown().await;
    let response = server.call("sync_prepare", json!({"itemIds":[]})).await;
    assert_eq!(response["error"]["data"]["errorCode"], "DAEMON_STOPPED");
    assert!(
        state
            .sync_operation_manager
            .get_all_operations()
            .await
            .is_empty()
    );
    let health = server.call("daemon.health", json!({})).await;
    assert_eq!(health["result"]["data"]["status"], "stopping");
}

#[tokio::test]
async fn sync_runtime_isolation_panicked_dispatch_is_an_internal_error_with_original_id() {
    let _credentials = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let core = TestCoreRuntime::new();
    let server = TestRpcServer::new(state.clone(), core.handle.clone()).await;
    let (block, entered, mut release) = runtime_block("changes", true);
    let mut provider = FakeBrowseProvider::new(vec![], vec![]);
    Arc::get_mut(&mut provider).unwrap().runtime_block = Some(block);
    state
        .server_manager
        .write()
        .await
        .set_test_provider(provider);
    let request = server
        .request("sync_detect_changes", json!({"syncToken":null}))
        .timeout(std::time::Duration::from_secs(15));
    let pending =
        tokio::spawn(async move { request.send().await.unwrap().json::<Value>().await.unwrap() });
    assert_eq!(blocked_runtime_thread(entered).await, "sync-core-test");
    release.release();
    let response = pending.await.unwrap();
    assert_eq!(response["id"], "runtime-request");
    assert_eq!(response["error"]["code"], ERR_INTERNAL_ERROR);
    assert!(response["result"].is_null());
}

#[tokio::test]
async fn sync_runtime_isolation_dropped_forwarder_releases_unfinished_preparation() {
    let _credentials = credential_test_lock();
    let (_dir, state) = sync_plan_test_state().await;
    let core = TestCoreRuntime::new();
    let gate = Arc::new((tokio::sync::Notify::new(), tokio::sync::Notify::new()));
    let mut provider = FakeBrowseProvider::new(vec![], vec![]);
    Arc::get_mut(&mut provider).unwrap().album_gate = Some(gate.clone());
    state
        .server_manager
        .write()
        .await
        .set_test_provider(provider);
    let request = JsonRpcRequest {
        jsonrpc: "2.0".into(),
        method: "sync_prepare".into(),
        params: Some(json!({"itemIds":["held-album"]})),
        id: json!(42),
    };
    let pending = tokio::spawn(dispatch_handler(
        State(state.clone()),
        Extension(core.handle.clone()),
        Json(request),
    ));
    tokio::time::timeout(std::time::Duration::from_secs(2), gate.0.notified())
        .await
        .unwrap();
    assert!(state.sync_operation_manager.try_start_pipeline().is_none());
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while state.sync_operation_manager.try_start_pipeline().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    state.sync_operation_manager.begin_shutdown_fence();
    state.sync_operation_manager.commit_shutdown().await;
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        state.sync_operation_manager.wait_for_shutdown_drain(),
    )
    .await
    .unwrap();
    assert!(
        state
            .sync_operation_manager
            .pending_plan
            .lock()
            .unwrap()
            .is_none()
    );
}
