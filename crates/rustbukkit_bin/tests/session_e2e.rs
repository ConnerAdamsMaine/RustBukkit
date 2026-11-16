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
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new("debug"))
            .with_test_writer()
            .try_init();
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

/// Dirt's 26.3 item registry ID (`item_registry_id`).
const DIRT_ITEM_ID: i32 = 55;

#[tokio::test]
async fn inventory_clicks_move_items_and_survive_reconnect() {
    let harness = Harness::start().await;
    let mut alice = TestClient::connect(&harness.addr).await.unwrap();
    alice.join("Alice").await.unwrap();
    let (x, y, z) = alice.wait_for_position(JOIN_TIMEOUT).await.unwrap();
    let initial = alice.wait_for_container_content(JOIN_TIMEOUT).await.unwrap();
    assert_eq!(initial.container_id, 0);
    assert_eq!(initial.state_id, 0);
    assert_eq!(initial.slots.len(), 46);
    assert!(initial.slots.iter().all(Option::is_none), "fresh inventory is empty");
    assert_eq!(initial.carried, None);

    // Water sits at the spawn column and item drops sink through it, so
    // build a two-block grass stack first, stand next to it, then break the
    // top block: the drop lands on the lower block within pickup range.
    let base = (x.floor() as i32 + 2, y.floor() as i32 - 1, z.floor() as i32);
    alice.use_item_on(base.0, base.1, base.2, 1).await.unwrap();
    alice.use_item_on(base.0, base.1 + 1, base.2, 1).await.unwrap();
    alice
        .send_position(f64::from(base.0) + 0.5, y, f64::from(base.2) + 0.5)
        .await
        .unwrap();
    alice.dig(base.0, base.1 + 2, base.2).await.unwrap();
    let pickup = alice.wait_for_container_content(JOIN_TIMEOUT).await.unwrap();
    let picked_slot = pickup
        .slots
        .iter()
        .position(Option::is_some)
        .expect("the dirt drop must be picked up into the inventory");
    assert_eq!(pickup.slots[picked_slot], Some((DIRT_ITEM_ID, 1)));
    assert_eq!(pickup.carried, None);

    // Left click: the server moves the stack to the cursor and resyncs.
    alice.container_click(picked_slot as i16, 0, 0).await.unwrap();
    let clicked = alice.wait_for_container_content(JOIN_TIMEOUT).await.unwrap();
    assert_eq!(clicked.slots[picked_slot], None);
    assert_eq!(clicked.carried, Some((DIRT_ITEM_ID, 1)));

    // A second left click on a hotbar slot places the carried stack there.
    alice.container_click(36, 0, 0).await.unwrap();
    let placed = alice.wait_for_container_content(JOIN_TIMEOUT).await.unwrap();
    assert_eq!(placed.slots[36], Some((DIRT_ITEM_ID, 1)));
    assert_eq!(placed.carried, None);

    // Clicking a craft/armor slot (0-8) is rejected: contents unchanged.
    alice.container_click(5, 0, 0).await.unwrap();
    let rejected = alice.wait_for_container_content(JOIN_TIMEOUT).await.unwrap();
    assert_eq!(rejected.slots, placed.slots, "rejected click must not change slots");
    assert_eq!(rejected.carried, placed.carried);

    drop(alice);
    // The previous session releases its registration asynchronously after the
    // connection closes; retry the join until the name is free again.
    let mut rejoined = None;
    for _ in 0..20 {
        let mut candidate = TestClient::connect(&harness.addr).await.unwrap();
        if candidate.join("Alice").await.is_ok() {
            rejoined = Some(candidate);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let mut again = rejoined.expect("rejoin within the retry budget");
    again.wait_for_position(JOIN_TIMEOUT).await.unwrap();
    let restored = again.wait_for_container_content(JOIN_TIMEOUT).await.unwrap();
    assert_eq!(restored.slots[36], Some((DIRT_ITEM_ID, 1)), "inventory survives reconnect");
    assert_eq!(restored.carried, None);

    drop(again);
    harness.stop().await;
}

#[tokio::test]
async fn players_receive_each_others_presence_and_movement() {
    let harness = Harness::start().await;
    let version = VersionRegistry::select("26.3").unwrap();
    let player_info = version.clientbound_id(ClientboundPacket::PlayerInfo);
    let add_entity = version.clientbound_id(ClientboundPacket::SpawnEntity);
    let entity_move = version.clientbound_id(ClientboundPacket::EntityMove);

    let mut alice = TestClient::connect(&harness.addr).await.unwrap();
    alice.join("Alice").await.unwrap();
    alice.wait_for_position(JOIN_TIMEOUT).await.unwrap();

    let mut bob = TestClient::connect(&harness.addr).await.unwrap();
    bob.join("Bob").await.unwrap();
    let (_, y, _) = bob.wait_for_position(JOIN_TIMEOUT).await.unwrap();

    // Alice learns about Bob's tab-list entry and avatar.
    alice.wait_for(player_info, JOIN_TIMEOUT).await.unwrap();
    alice.wait_for(add_entity, JOIN_TIMEOUT).await.unwrap();
    // Bob receives Alice's existing presence on join.
    bob.wait_for(player_info, JOIN_TIMEOUT).await.unwrap();
    bob.wait_for(add_entity, JOIN_TIMEOUT).await.unwrap();

    // Bob moves and Alice observes it.
    bob.send_position(2.5, y, 0.5).await.unwrap();
    let movement = alice.wait_for(entity_move, JOIN_TIMEOUT).await.unwrap();
    assert!(movement.payload.len() >= 9);

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
