//! Process-lifecycle persistence: a block placed through the network and a
//! player location must survive shutting the server down and starting a new
//! server instance on the same world directory.
//!
//! Every step is synchronized against an authoritative server response
//! (Block Update or Set Chunk Cache Center) instead of fixed sleeps, so the
//! test does not race packet processing under load.

use std::path::Path;
use std::time::Duration;

use rustbukkit_bin::server;
use rustbukkit_bin::test_client::TestClient;
use rustbukkit_config::ServerConfig;
use rustbukkit_protocol::{ClientboundPacket, VersionRegistry};
use tokio::sync::watch;

const TIMEOUT: Duration = Duration::from_secs(10);

struct Running {
    addr:     String,
    shutdown: watch::Sender<bool>,
    task:     tokio::task::JoinHandle<()>,
}

async fn start(root: &Path) -> Running {
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
    let world_root = root.to_path_buf();
    let version = VersionRegistry::select("26.3").unwrap();
    let task = tokio::spawn(async move {
        server::serve(config, version, listener, world_root, rx)
            .await
            .expect("server stopped with an error");
    });
    Running { addr, shutdown, task }
}

impl Running {
    async fn stop(self) {
        let _ = self.shutdown.send(true);
        match tokio::time::timeout(Duration::from_secs(10), self.task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => panic!("server task failed: {error}"),
            Err(_) => panic!("server did not stop"),
        }
    }
}

#[tokio::test]
async fn world_edits_and_player_position_survive_a_server_restart() {
    let root = std::env::temp_dir().join(format!("rustbukkit-restart-{}", uuid::Uuid::new_v4()));
    let version = VersionRegistry::select("26.3").unwrap();
    let block_update_id = version.clientbound_id(ClientboundPacket::BlockUpdate);
    let view_center_id = version.clientbound_id(ClientboundPacket::ViewCenter);

    let first = start(&root).await;

    let mut client = TestClient::connect(&first.addr).await.unwrap();
    client.join("Restarter").await.unwrap();
    let (x, y, z) = client.wait_for_position(TIMEOUT).await.unwrap();
    client.wait_for(0x2e, TIMEOUT).await.unwrap();

    // Place a block on the ground two blocks east of spawn. The authoritative
    // Block Update proves the placement was committed before the client leaves.
    let ground =
        rustbukkit_chunks::BlockPos::new(x.floor() as i32 + 2, y.floor() as i32 - 1, z.floor() as i32)
            .unwrap();
    let placed = rustbukkit_chunks::BlockPos::new(ground.x, ground.y + 1, ground.z).unwrap();
    client.use_item_on(ground.x, ground.y, ground.z, 1).await.unwrap();
    client.wait_for(block_update_id, TIMEOUT).await.unwrap();

    // Move east into the neighbouring chunk; the Set Chunk Cache Center packet
    // proves the server applied this movement before the session ends, so the
    // leave-save must persist this position.
    let moved_to = (24.5, y, z);
    client
        .send_position(moved_to.0, moved_to.1, moved_to.2)
        .await
        .unwrap();
    client.wait_for(view_center_id, TIMEOUT).await.unwrap();

    drop(client);
    first.stop().await;

    // Second full server lifecycle on the same directory.
    let second = start(&root).await;
    let mut client = TestClient::connect(&second.addr).await.unwrap();
    client.join("Restarter").await.unwrap();
    let (x, y, z) = client.wait_for_position(TIMEOUT).await.unwrap();
    assert_eq!((x, y, z), moved_to, "saved player position was not restored");
    client.wait_for(0x2e, TIMEOUT).await.unwrap();

    // Move back within reach of the placed block; again synchronized on the
    // chunk-center packet so the dig below cannot race unprocessed movement.
    client.send_position(2.5, y, 0.5).await.unwrap();
    client.wait_for(view_center_id, TIMEOUT).await.unwrap();

    // The placed block must exist in the reloaded world: breaking it produces
    // an authoritative Block Update. A world that lost the edit would keep the
    // block, so the break would not commit and no update would arrive.
    client.dig(placed.x, placed.y, placed.z).await.unwrap();
    let update = client.wait_for(block_update_id, TIMEOUT).await.unwrap();
    assert_eq!(
        update.payload,
        version.block_update_payload(placed, rustbukkit_chunks::BlockKind::Air),
        "reloaded world did not contain the block placed before the restart"
    );

    drop(client);
    second.stop().await;

    let world = rustbukkit_bin::world_persistence::WorldStore::new(root.clone())
        .load()
        .await
        .unwrap();
    assert_eq!(
        world.get_block(placed),
        rustbukkit_chunks::BlockKind::Air,
        "the post-restart break was not persisted"
    );
    tokio::fs::remove_dir_all(root).await.unwrap();
}
