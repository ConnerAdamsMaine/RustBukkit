//! N3 combat integration tests: serverbound `attack` validation, victim health
//! updates, hurt animation/entity event/knockback feedback, death with an
//! inventory drop as item entities, and the client-driven respawn path.
//!
//! The wire is exercised through the real server via `TestClient`; packet IDs
//! come from the version pack so a 26.3 ID change fails loudly here too.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, bail};
use rustbukkit_bin::server;
use rustbukkit_bin::test_client::TestClient;
use rustbukkit_config::ServerConfig;
use rustbukkit_decoding::{Frame, Reader};
use rustbukkit_protocol::{ClientboundPacket, VersionPack, VersionRegistry};
use tokio::sync::watch;

const JOIN_TIMEOUT: Duration = Duration::from_secs(10);
const PACKET_TIMEOUT: Duration = Duration::from_secs(5);
/// One server tick is 50 ms; the vanilla melee invulnerability window is 10.
const COOLDOWN_PAUSE: Duration = Duration::from_millis(620);
/// Dirt's 26.3 item registry ID (`item_registry_id`).
const DIRT_ITEM_ID: i32 = 55;

fn version() -> &'static dyn VersionPack {
    VersionRegistry::select("26.3").unwrap()
}

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
        let root = std::env::temp_dir().join(format!("rustbukkit-combat-{}", uuid::Uuid::new_v4()));
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
        let version = version();
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

/// Vanilla `set_health` (ID 106): health f32, food VarInt, saturation f32.
async fn wait_for_set_health(client: &mut TestClient) -> Result<f32> {
    let frame = client
        .wait_for(version().clientbound_id(ClientboundPacket::UpdateHealth), PACKET_TIMEOUT)
        .await?;
    let mut reader = Reader::new(&frame.payload);
    Ok(reader.f32()?)
}

/// Vanilla `hurt_animation` (ID 43): entity id VarInt, yaw f32.
async fn wait_for_hurt_animation(client: &mut TestClient) -> Result<(i32, f32)> {
    let frame = client
        .wait_for(version().clientbound_id(ClientboundPacket::HurtAnimation), PACKET_TIMEOUT)
        .await?;
    let mut reader = Reader::new(&frame.payload);
    let entity_id = reader.varint()?;
    Ok((entity_id, reader.f32()?))
}

/// Vanilla `entity_event` (ID 34): entity id i32 (big endian), event i8.
async fn wait_for_entity_event(client: &mut TestClient) -> Result<(i32, i8)> {
    let frame = client
        .wait_for(version().clientbound_id(ClientboundPacket::EntityEvent), PACKET_TIMEOUT)
        .await?;
    let mut reader = Reader::new(&frame.payload);
    let entity_id = reader.i32()?;
    Ok((entity_id, reader.i8()?))
}

/// Vanilla `set_entity_motion` (ID 103): entity id VarInt, LpVec3 velocity.
async fn wait_for_set_entity_motion(client: &mut TestClient) -> Result<(i32, [f64; 3])> {
    let frame = client
        .wait_for(version().clientbound_id(ClientboundPacket::EntityVelocity), PACKET_TIMEOUT)
        .await?;
    let mut reader = Reader::new(&frame.payload);
    let entity_id = reader.varint()?;
    Ok((entity_id, read_lp_vec3(&mut reader)?))
}

fn unpack_lp(value: u64) -> f64 {
    ((value & 32_767) as f64).min(32_766.0) * 2.0 / 32_766.0 - 1.0
}

/// LpVec3 reader mirroring `Vec3.LP_STREAM_CODEC` (see the protocol crate's
/// writer, adapted from the HorizonX `LpVec3` codec).
fn read_lp_vec3(reader: &mut Reader<'_>) -> Result<[f64; 3]> {
    let lowest = u64::from(reader.u8()?);
    if lowest == 0 {
        return Ok([0.0; 3]);
    }
    let middle = u64::from(reader.u8()?);
    let highest_bytes = reader.raw(4)?;
    let highest = u32::from_be_bytes(highest_bytes.try_into().unwrap()) as u64;
    let buffer = highest << 16 | middle << 8 | lowest;
    let mut scale = lowest & 3;
    if lowest & 4 == 4 {
        scale |= u64::from(reader.varint()? as u32) << 2;
    }
    let scale = scale as f64;
    Ok([
        unpack_lp(buffer >> 3) * scale,
        unpack_lp(buffer >> 18) * scale,
        unpack_lp(buffer >> 33) * scale,
    ])
}

/// The entity id from a Join Game payload (first field, i32 BE).
fn join_game_entity_id(frame: &Frame) -> i32 {
    i32::from_be_bytes(frame.payload[0..4].try_into().unwrap())
}

/// Join an attacker next to an already-joined victim and return the victim's
/// entity id plus the spawn coordinates.
async fn join_pair(harness: &Harness) -> Result<(TestClient, TestClient, i32, (f64, f64, f64))> {
    let mut victim = TestClient::connect(&harness.addr).await?;
    let join = victim.join("Bob").await?;
    let victim_entity_id = join_game_entity_id(&join);
    let (x, y, z) = victim.wait_for_position(JOIN_TIMEOUT).await?;
    victim.wait_for(0x2e, JOIN_TIMEOUT).await?;

    let mut attacker = TestClient::connect(&harness.addr).await?;
    attacker.join("Alice").await?;
    attacker.wait_for_position(JOIN_TIMEOUT).await?;
    attacker.wait_for(0x2e, JOIN_TIMEOUT).await?;
    // Both spawn on the same block; the attacker stands one block east.
    attacker.send_position(x + 1.0, y, z).await?;
    // Keep both sockets draining instead of sleeping so the server's chunk
    // writes never block on a full client receive buffer.
    victim.drain_for(Duration::from_millis(125)).await?;
    attacker.drain_for(Duration::from_millis(125)).await?;
    Ok((attacker, victim, victim_entity_id, (x, y, z)))
}

#[tokio::test]
async fn rejected_attacks_leave_victim_health_untouched() {
    let harness = Harness::start().await;
    let (mut attacker, mut victim, victim_entity_id, (x, y, z)) = join_pair(&harness).await.unwrap();

    // An unknown target entity id is rejected: the victim hears nothing.
    attacker.attack(999_999).await.unwrap();
    victim.drain_for(Duration::from_millis(150)).await.unwrap();
    attacker.drain_for(Duration::from_millis(150)).await.unwrap();

    // A valid in-range hit applies exactly one heart of vanilla melee damage.
    attacker.attack(victim_entity_id).await.unwrap();
    // The victim's stream carries its own hurt animation first, then the
    // authoritative set_health (the entity event and velocity are skipped).
    let (own_hurt_entity, _) = wait_for_hurt_animation(&mut victim).await.unwrap();
    assert_eq!(own_hurt_entity, victim_entity_id);
    assert_eq!(wait_for_set_health(&mut victim).await.unwrap(), 19.0);
    let (hurt_entity, _) = wait_for_hurt_animation(&mut attacker).await.unwrap();
    assert_eq!(hurt_entity, victim_entity_id);
    let (event_entity, event) = wait_for_entity_event(&mut attacker).await.unwrap();
    assert_eq!(event_entity, victim_entity_id);
    assert_eq!(event, 2, "vanilla hurt entity event");
    let (motion_entity, motion) = wait_for_set_entity_motion(&mut attacker).await.unwrap();
    assert_eq!(motion_entity, victim_entity_id);
    // Vanilla knockback magnitude: 0.4 vertical, 0.4 horizontal.
    assert!((motion[1] - 0.4).abs() < 0.01, "unexpected vertical knockback {motion:?}");
    let horizontal = (motion[0] * motion[0] + motion[2] * motion[2]).sqrt();
    assert!((horizontal - 0.4).abs() < 0.02, "unexpected knockback {motion:?}");

    // Out of range: the attacker walks six blocks away and swings. Nothing
    // happens; the victim's health is untouched.
    attacker.send_position(x + 6.0, y, z).await.unwrap();
    victim.drain_for(COOLDOWN_PAUSE).await.unwrap();
    attacker.drain_for(COOLDOWN_PAUSE).await.unwrap();
    attacker.attack(victim_entity_id).await.unwrap();
    victim.drain_for(Duration::from_millis(200)).await.unwrap();
    attacker.drain_for(Duration::from_millis(200)).await.unwrap();

    // After returning to range the next valid hit lands as the *second* point
    // of damage: had either rejected swing applied, this would read 18.0/17.0.
    // The spawn column is water, so both fighters re-assert their positions
    // right before the swing; idle sinking would otherwise drift the victim
    // out of vanilla reach.
    attacker.send_position(x + 1.0, y, z).await.unwrap();
    victim.send_position(x, y, z).await.unwrap();
    victim.drain_for(COOLDOWN_PAUSE).await.unwrap();
    attacker.drain_for(COOLDOWN_PAUSE).await.unwrap();
    victim.send_position(x, y, z).await.unwrap();
    attacker.attack(victim_entity_id).await.unwrap();
    assert_eq!(wait_for_set_health(&mut victim).await.unwrap(), 18.0);

    drop(attacker);
    drop(victim);
    harness.stop().await;
}

/// Wait for the vanilla death entity event (byte 3) on the victim entity,
/// skipping the hurt events (byte 2) that arrive on every hit.
async fn wait_for_death_event(client: &mut TestClient, victim_entity_id: i32, limit: Duration) -> Result<()> {
    let id = version().clientbound_id(ClientboundPacket::EntityEvent);
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            bail!("no death entity event within {limit:?}");
        }
        let frame = client.wait_for(id, remaining).await?;
        let mut reader = Reader::new(&frame.payload);
        let entity_id = reader.i32()?;
        let event = reader.i8()?;
        assert_eq!(entity_id, victim_entity_id);
        if event == 3 {
            return Ok(());
        }
    }
}

/// Punch the victim on a 10-tick cooldown until it dies. Each round the victim
/// re-asserts its position so knockback never carries it out of reach.
async fn fight_to_death(
    attacker: &mut TestClient,
    victim: &mut TestClient,
    victim_entity_id: i32,
    (x, y, z): (f64, f64, f64),
) -> Result<()> {
    // 20 damage points at one per hit; a couple of spare rounds absorb any
    // attack that races the invulnerability window.
    for _ in 0..24 {
        // Only the attacker drains: the victim's feedback frames (hurt, death,
        // health) must accumulate in its buffer for the death check below, and
        // re-asserted positions produce no chunk writes that could back up the
        // server's write pipe.
        attacker.drain_for(COOLDOWN_PAUSE).await?;
        attacker.attack(victim_entity_id).await?;
        victim.send_position(x, y, z).await?;
    }
    wait_for_death_event(victim, victim_entity_id, PACKET_TIMEOUT).await
}

#[tokio::test]
async fn death_drops_inventory_and_respawn_restores_health() {
    let harness = Harness::start().await;
    let mut bob = TestClient::connect(&harness.addr).await.unwrap();
    let bob_join = bob.join("Bob").await.unwrap();
    let victim_entity_id = join_game_entity_id(&bob_join);
    let (x, y, z) = bob.wait_for_position(JOIN_TIMEOUT).await.unwrap();
    bob.wait_for(0x2e, JOIN_TIMEOUT).await.unwrap();

    // The flat test world is an ocean: bedrock at y=63, water from 64 to 99,
    // air at 100. Build a 3x3 solid platform at feet level (y=100) so Bob has
    // solid footing and the death drop lands on ground instead of sinking to
    // the ocean floor, then stand on top of it at y=101.
    let base = (x.floor() as i32 + 2, y.floor() as i32 - 1, z.floor() as i32);
    let stand = (f64::from(base.0) + 0.5, y + 1.0, f64::from(base.2) + 0.5);
    bob.send_position(stand.0, stand.1, stand.2).await.unwrap();
    for bx in base.0 - 1..=base.0 + 1 {
        for bz in base.2 - 1..=base.2 + 1 {
            bob.use_item_on(bx, base.1, bz, 1).await.unwrap();
        }
    }
    // A separate stack column Bob does not occupy: the dirt drop from
    // breaking its top rests within pickup range of the stand spot.
    let stack = (base.0, base.1 + 2, base.2 - 1);
    bob.use_item_on(stack.0, base.1 + 1, stack.2, 1).await.unwrap();
    bob.use_item_on(stack.0, stack.1, stack.2, 1).await.unwrap();
    bob.dig(stack.0, stack.1 + 1, stack.2).await.unwrap();
    let pickup = bob.wait_for_container_content(JOIN_TIMEOUT).await.unwrap();
    assert!(pickup.slots.contains(&Some((DIRT_ITEM_ID, 1))), "Bob must hold the dirt drop before dying");

    let mut alice = TestClient::connect(&harness.addr).await.unwrap();
    alice.join("Alice").await.unwrap();
    alice.wait_for_position(JOIN_TIMEOUT).await.unwrap();
    alice.wait_for(0x2e, JOIN_TIMEOUT).await.unwrap();
    // Alice stands one block east of Bob on the same platform level.
    let alice_spot = (stand.0 + 1.0, stand.1, stand.2);
    alice
        .send_position(alice_spot.0, alice_spot.1, alice_spot.2)
        .await
        .unwrap();
    bob.drain_for(Duration::from_millis(125)).await.unwrap();
    alice.drain_for(Duration::from_millis(125)).await.unwrap();

    // Bob fights to death standing at the platform.
    fight_to_death(&mut alice, &mut bob, victim_entity_id, stand)
        .await
        .unwrap();

    // Bob's own death: the inventory is emptied into item entities and the
    // respawn request restores full health at the world spawn.
    bob.respawn_request().await.unwrap();
    let respawn = bob
        .wait_for(version().clientbound_id(ClientboundPacket::Respawn), PACKET_TIMEOUT)
        .await
        .unwrap();
    assert!(!respawn.payload.is_empty());
    assert_eq!(wait_for_set_health(&mut bob).await.unwrap(), 20.0);
    let resync = bob.wait_for_container_content(PACKET_TIMEOUT).await.unwrap();
    assert!(resync.slots.iter().all(Option::is_none), "the death drop must have emptied the inventory");

    // The dropped dirt became a real item entity: walking back to the death
    // location collects it again through the normal pickup path. The drop
    // rest toward the +z platform edge (the death yaw throws it there), so
    // Bob walks to the edge column rather than the stand spot itself.
    bob.send_position(stand.0, stand.1, stand.2 + 0.5).await.unwrap();
    let restored = bob
        .wait_for_container_content(Duration::from_secs(6))
        .await
        .unwrap();
    assert!(
        restored.slots.contains(&Some((DIRT_ITEM_ID, 1))),
        "the dropped dirt must return as a pickable item entity"
    );

    drop(alice);
    drop(bob);
    harness.stop().await;
}
