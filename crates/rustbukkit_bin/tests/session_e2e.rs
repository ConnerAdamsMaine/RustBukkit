use std::path::PathBuf;
use std::time::Duration;

use rustbukkit_bin::server;
use rustbukkit_bin::session::offline_uuid;
use rustbukkit_bin::test_client::TestClient;
use rustbukkit_chunks::{BlockKind, BlockPos};
use rustbukkit_config::ServerConfig;
use rustbukkit_protocol::{ClientboundPacket, VersionRegistry};
use tokio::sync::watch;

const JOIN_TIMEOUT: Duration = Duration::from_secs(10);

struct Harness {
    addr:     String,
    root:     PathBuf,
    shutdown: watch::Sender<bool>,
    task:     tokio::task::JoinHandle<()>,
}

impl Harness {
    async fn start() -> Self {
        let root = std::env::temp_dir().join(format!("rustbukkit-e2e-{}", uuid::Uuid::new_v4()));
        let config = ServerConfig::parse(
            "mcVersion = '26.3'\n\
             bind = '127.0.0.1:0'\n\
             onlineMode = false\n\
             maxPlayers = 20\n\
             viewDistance = 2\n\
             simulationDistance = 2\n\
             motd = 'RustBukkit'\n\
             keepAliveIntervalSeconds = 10\n\
             connectionTimeoutSeconds = 30\n",
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let (shutdown, rx) = watch::channel(false);
        let world_root = root.clone();
        let version = VersionRegistry::select("26.3").unwrap();
        let task = tokio::spawn(async move {
            server::serve(config, version, listener, world_root, rx)
                .await
                .expect("server stopped with an error");
        });
        Self {
            addr,
            root,
            shutdown,
            task,
        }
    }

    async fn stop(self) {
        let _ = self.shutdown.send(true);
        match tokio::time::timeout(Duration::from_secs(10), self.task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => panic!("server task failed: {error}"),
            Err(_) => panic!("server did not stop"),
        }
        let _ = tokio::fs::remove_dir_all(&self.root).await;
    }
}

#[tokio::test]
async fn client_joins_and_receives_authoritative_terrain_chunks() {
    let harness = Harness::start().await;
    let mut client = TestClient::connect(&harness.addr).await.unwrap();
    client.join("Alice").await.unwrap();

    let (px, py, pz) = client.wait_for_position(JOIN_TIMEOUT).await.unwrap();
    assert!(px.is_finite() && py.is_finite() && pz.is_finite());
    assert!((py as i32) < 320);

    let chunk = client.wait_for(0x2e, JOIN_TIMEOUT).await.unwrap();
    assert!(chunk.payload.len() > 8);
    let x = i32::from_be_bytes(chunk.payload[0..4].try_into().unwrap());
    let z = i32::from_be_bytes(chunk.payload[4..8].try_into().unwrap());
    assert!(x.abs() <= 2 && z.abs() <= 2, "chunk outside view distance: ({x},{z})");

    drop(client);
    harness.stop().await;
}

#[tokio::test]
async fn block_changes_reach_every_player_in_the_world() {
    let harness = Harness::start().await;
    let mut alice = TestClient::connect(&harness.addr).await.unwrap();
    alice.join("Alice").await.unwrap();
    let (x, y, z) = alice.wait_for_position(JOIN_TIMEOUT).await.unwrap();
    alice.wait_for(0x2e, JOIN_TIMEOUT).await.unwrap();

    let mut bob = TestClient::connect(&harness.addr).await.unwrap();
    bob.join("Bob").await.unwrap();
    bob.wait_for_position(JOIN_TIMEOUT).await.unwrap();
    // Give Bob's session the moment it needs to subscribe to world block events.
    tokio::time::sleep(Duration::from_millis(250)).await;

    let target = BlockPos::new(x.floor() as i32 + 2, y.floor() as i32 - 1, z.floor() as i32).unwrap();
    let placed = BlockPos::new(target.x, target.y + 1, target.z).unwrap();
    alice.use_item_on(target.x, target.y, target.z, 1).await.unwrap();

    let version = VersionRegistry::select("26.3").unwrap();
    let block_update = version.clientbound_id(ClientboundPacket::BlockUpdate);
    let expected = version.block_update_payload(placed, BlockKind::GrassBlock);
    let update = bob.wait_for(block_update, JOIN_TIMEOUT).await.unwrap();
    assert_eq!(update.payload, expected, "peer received a different block update");
    let echoed = alice.wait_for(block_update, JOIN_TIMEOUT).await.unwrap();
    assert_eq!(echoed.payload, expected, "actor received no block confirmation");

    drop(alice);
    drop(bob);
    harness.stop().await;
}

#[tokio::test]
async fn player_position_is_persisted_when_the_session_ends() {
    let harness = Harness::start().await;
    let mut client = TestClient::connect(&harness.addr).await.unwrap();
    client.join("Carol").await.unwrap();
    let (_, y, _) = client.wait_for_position(JOIN_TIMEOUT).await.unwrap();
    client.send_position(8.5, y, 8.5).await.unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;
    drop(client);

    let path = harness
        .root
        .join("playerdata")
        .join(format!("{}.json", offline_uuid("Carol")));
    let deadline = tokio::time::Instant::now() + JOIN_TIMEOUT;
    let saved = loop {
        if let Ok(bytes) = tokio::fs::read(&path).await {
            break serde_json::from_slice::<serde_json::Value>(&bytes).unwrap();
        }
        assert!(tokio::time::Instant::now() < deadline, "player state was never saved to {}", path.display());
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(saved["username"], "Carol");
    assert_eq!(saved["location"]["x"], 8.5);
    assert_eq!(saved["location"]["z"], 8.5);

    harness.stop().await;
}
