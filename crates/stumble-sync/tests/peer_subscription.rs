use axum::{extract::State, routing::get, Json, Router};
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;
use stumble_core::*;
use stumble_sync::{synchronize_subscription_from_peer, PeerSyncError};
use uuid::Uuid;

struct TestDataDir(std::path::PathBuf);

impl TestDataDir {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("stumble-peer-sync-{label}-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
struct PeerOriginState {
    snapshot: FederationPodSnapshot,
    event_behavior: EventBehavior,
}

#[derive(Clone)]
enum EventBehavior {
    Immediate,
    CoordinateApply(ApplyGate),
}

#[derive(Clone)]
struct ApplyGate {
    events_requested: mpsc::Sender<()>,
    application_lock_acquired: Arc<Barrier>,
    application_phase: Arc<tokio::sync::Notify>,
}

async fn test_node(State(state): State<PeerOriginState>) -> Json<WellKnownNode> {
    Json(WellKnownNode {
        protocol: CURRENT_PROTOCOL_VERSION.into(),
        node: state.snapshot.node,
        endpoints: Default::default(),
    })
}

async fn test_manifest(State(state): State<PeerOriginState>) -> Json<PodManifest> {
    Json(state.snapshot.manifest)
}

async fn test_events(State(state): State<PeerOriginState>) -> Json<Vec<EventLog>> {
    if let EventBehavior::CoordinateApply(gate) = state.event_behavior {
        let ApplyGate {
            events_requested,
            application_lock_acquired,
            application_phase,
        } = gate;
        events_requested.send(()).unwrap();
        tokio::task::spawn_blocking(move || application_lock_acquired.wait())
            .await
            .unwrap();
        application_phase.notify_one();
    }
    Json(state.snapshot.events)
}

fn peer_origin_router(state: PeerOriginState) -> Router {
    Router::new()
        .route("/.well-known/stumble-node", get(test_node))
        .route(
            "/federation/pods/runtime-scheduling/manifest",
            get(test_manifest),
        )
        .route(
            "/federation/pods/runtime-scheduling/events",
            get(test_events),
        )
        .with_state(state)
}

async fn serve_snapshot(snapshot: FederationPodSnapshot) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let router = peer_origin_router(PeerOriginState {
        snapshot,
        event_behavior: EventBehavior::Immediate,
    });
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base_url, server)
}

fn origin_snapshot() -> (NodeInfo, FederationPodSnapshot) {
    let origin = AgentTools::new(seed_store());
    let origin_ctx = origin.default_auth_context().unwrap();
    let origin_node = origin.node_info(&origin_ctx).unwrap();
    let mut origin_pod = origin
        .create_pod(
            &origin_ctx,
            CreatePodRequest {
                name: "Runtime scheduling".into(),
                slug: "runtime-scheduling".into(),
                description: "Peer synchronization scheduling regression".into(),
                visibility: Visibility::Private,
            },
        )
        .unwrap();
    origin_pod.visibility = Visibility::Public;
    {
        let store = origin.store();
        let mut store = store.write().unwrap();
        store.pods.insert(origin_pod.id, origin_pod.clone());
        store
            .pod_rules
            .get_mut(&origin_pod.id)
            .unwrap()
            .federate_sources = true;
    }
    let snapshot = origin
        .federation_pod_snapshot(&origin_ctx, &origin_pod.slug, None)
        .unwrap();
    (origin_node, snapshot)
}

fn subscribe_home(
    home: &AgentTools,
    public_pod_url: &str,
    snapshot: FederationPodSnapshot,
) -> (AuthContext, Subscription) {
    let mut home_ctx = home.default_auth_context().unwrap();
    home_ctx.user_id = home.store().read().unwrap().users.keys().next().copied();
    let subscription = home
        .subscribe_public_pod(
            &home_ctx,
            SubscribePublicPodRequest::new(public_pod_url, snapshot),
            chrono::Utc::now(),
        )
        .unwrap()
        .subscription;
    (home_ctx, subscription)
}

fn matching_peer(origin_node: &NodeInfo, base_url: &str) -> TrustedPeer {
    TrustedPeer {
        id: Uuid::now_v7(),
        node_id: origin_node.node_id,
        tenant_id: None,
        display_name: "selected Origin".into(),
        base_url: base_url.into(),
        public_key: origin_node.public_key.clone(),
        trust_level: TrustLevel::ReadOnly,
        enabled: true,
        created_at: chrono::Utc::now(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn peer_subscription_lookup_keeps_the_async_runtime_responsive() {
    let (origin_node, snapshot) = origin_snapshot();
    let home = AgentTools::new(seed_store());
    let (home_ctx, subscription) = subscribe_home(
        &home,
        "http://127.0.0.1:9/federation/pods/runtime-scheduling",
        snapshot,
    );
    let peer = matching_peer(&origin_node, "http://127.0.0.1:9");

    let lock_acquired = Arc::new(Barrier::new(2));
    let (runtime_progress, wait_for_runtime) = mpsc::channel();
    let store = home.store();
    let lock_acquired_in_thread = Arc::clone(&lock_acquired);
    let lock_holder = std::thread::spawn(move || {
        let _guard = store.write().unwrap();
        lock_acquired_in_thread.wait();
        wait_for_runtime
            .recv_timeout(Duration::from_secs(2))
            .is_ok()
    });
    lock_acquired.wait();
    tokio::spawn(async move {
        runtime_progress.send(()).unwrap();
    });

    let result = synchronize_subscription_from_peer(&home, &home_ctx, &peer, subscription.id).await;

    assert!(matches!(result, Err(PeerSyncError::DirectSubscription(_))));
    assert!(
        lock_holder.join().unwrap(),
        "subscription lookup blocked the current-thread Tokio runtime"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn peer_subscription_apply_keeps_the_async_runtime_responsive() {
    let (origin_node, snapshot) = origin_snapshot();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}", listener.local_addr().unwrap());
    let data_dir = TestDataDir::new("blocking-apply");
    let home = AgentTools::open_home_node(&data_dir.0, seed_store).unwrap();
    let (home_ctx, subscription) = subscribe_home(
        &home,
        &format!("{base_url}/federation/pods/runtime-scheduling"),
        snapshot.clone(),
    );
    let peer = matching_peer(&origin_node, &base_url);

    let (events_requested, wait_for_events) = mpsc::channel();
    let application_lock_acquired = Arc::new(Barrier::new(2));
    let application_phase = Arc::new(tokio::sync::Notify::new());
    let router = peer_origin_router(PeerOriginState {
        snapshot,
        event_behavior: EventBehavior::CoordinateApply(ApplyGate {
            events_requested,
            application_lock_acquired: Arc::clone(&application_lock_acquired),
            application_phase: Arc::clone(&application_phase),
        }),
    });
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let (runtime_progress, wait_for_runtime) = mpsc::channel();
    let store = home.store();
    let lock_holder = std::thread::spawn(move || {
        wait_for_events.recv().unwrap();
        let _guard = store.write().unwrap();
        application_lock_acquired.wait();
        wait_for_runtime
            .recv_timeout(Duration::from_secs(2))
            .is_ok()
    });
    tokio::spawn(async move {
        application_phase.notified().await;
        runtime_progress.send(()).unwrap();
    });

    let result = synchronize_subscription_from_peer(&home, &home_ctx, &peer, subscription.id).await;

    assert_eq!(result.unwrap().imported_events, 0);
    assert!(
        lock_holder.join().unwrap(),
        "snapshot projection and SQLite persistence blocked the current-thread Tokio runtime"
    );
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn peer_subscription_rejects_a_selected_peer_mismatch_before_fetch() {
    let (origin_node, snapshot) = origin_snapshot();
    let home = AgentTools::new(seed_store());
    let (home_ctx, subscription) = subscribe_home(
        &home,
        "http://127.0.0.1:9/federation/pods/runtime-scheduling",
        snapshot,
    );
    let mut peer = matching_peer(&origin_node, "http://127.0.0.1:9");
    peer.public_key = "different-key".into();

    let result = synchronize_subscription_from_peer(&home, &home_ctx, &peer, subscription.id).await;

    assert!(matches!(
        result,
        Err(PeerSyncError::SubscriptionPeerMismatch)
    ));
}

#[tokio::test]
async fn peer_subscription_preserves_remote_identity_errors() {
    let (origin_node, snapshot) = origin_snapshot();
    let home = AgentTools::new(seed_store());
    let (home_ctx, subscription) = subscribe_home(
        &home,
        "http://127.0.0.1:9/federation/pods/runtime-scheduling",
        snapshot.clone(),
    );
    let mut mismatched_identity = snapshot.clone();
    mismatched_identity.node.public_key = "different-key".into();
    let (identity_base_url, identity_server) = serve_snapshot(mismatched_identity).await;
    let peer = matching_peer(&origin_node, &identity_base_url);

    let identity_result =
        synchronize_subscription_from_peer(&home, &home_ctx, &peer, subscription.id).await;

    assert!(matches!(
        identity_result,
        Err(PeerSyncError::PublicKeyMismatch)
    ));
    identity_server.abort();
    let _ = identity_server.await;
}

#[tokio::test]
async fn peer_subscription_preserves_core_apply_errors() {
    let (origin_node, mut invalid_snapshot) = origin_snapshot();
    let home = AgentTools::new(seed_store());
    let (home_ctx, subscription) = subscribe_home(
        &home,
        "http://127.0.0.1:9/federation/pods/runtime-scheduling",
        invalid_snapshot.clone(),
    );

    invalid_snapshot.manifest.pod.slug = "different-pod".into();
    let (invalid_base_url, invalid_server) = serve_snapshot(invalid_snapshot).await;
    let peer = matching_peer(&origin_node, &invalid_base_url);

    let apply_result =
        synchronize_subscription_from_peer(&home, &home_ctx, &peer, subscription.id).await;

    assert!(matches!(apply_result, Err(PeerSyncError::Core(_))));
    invalid_server.abort();
    let _ = invalid_server.await;
}
