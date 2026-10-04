use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use rustbukkit_config::ServerConfig;
use rustbukkit_decoding::Reader;
use rustbukkit_encoding::Writer;
use rustbukkit_protocol::{
    ClientboundPacket,
    Connection,
    LoginTransition,
    PlayInbound,
    ProtocolState,
    VersionPack,
};
use serde_json::json;
use tokio::sync::Semaphore;
use tokio::time::MissedTickBehavior;
use tracing::{debug, info};
use uuid::Uuid;

use crate::player_location::{PlayerLocation, PlayerLocationStore};

pub async fn run(
    connection: &mut Connection,
    config: &ServerConfig,
    version: &dyn VersionPack,
    slots: std::sync::Arc<Semaphore>,
    locations: &PlayerLocationStore,
) -> Result<()> {
    let frame = connection
        .read_frame()
        .await?
        .ok_or_else(|| anyhow::anyhow!("closed before handshake"))?;
    if frame.id != 0 {
        bail!("expected handshake packet, got 0x{:02x}", frame.id);
    }
    let mut handshake = Reader::new(&frame.payload);
    let client_protocol = handshake.varint()?;
    let _host = handshake.string(255)?;
    let _port = handshake.raw(2)?;
    let requested_state = handshake.varint()?;
    if handshake.remaining() != 0 {
        bail!("trailing bytes in handshake");
    }
    info!(
        "[VERSION] client={} configured={} minecraft={}",
        client_protocol,
        version.protocol_version(),
        version.minecraft_version()
    );
    match requested_state {
        1 => status(connection, config, version, &slots).await,
        2 => login(connection, config, version, client_protocol, slots, locations).await,
        other => bail!("unsupported handshake next state {other}"),
    }
}

async fn status(
    connection: &mut Connection,
    config: &ServerConfig,
    version: &dyn VersionPack,
    slots: &Semaphore,
) -> Result<()> {
    connection.set_state(ProtocolState::Status);
    let request = connection
        .read_frame()
        .await?
        .ok_or_else(|| anyhow::anyhow!("closed before status request"))?;
    if request.id != 0 || !request.payload.is_empty() {
        bail!("invalid status request");
    }
    let response = json!({
        "version": { "name": version.minecraft_version(), "protocol": version.protocol_version() },
        "players": { "max": config.max_players, "online": config.max_players as usize - slots.available_permits(), "sample": [] },
        "description": { "text": config.motd },
        "enforcesSecureChat": false
    });
    let mut writer = Writer::new();
    writer.string(&response.to_string());
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::StatusResponse), &writer.finish())
        .await?;
    if let Some(ping) = connection.read_frame().await? {
        if ping.id != 1 || ping.payload.len() != 8 {
            bail!("invalid status ping");
        }
        connection
            .write_frame(version.clientbound_id(ClientboundPacket::StatusPong), &ping.payload)
            .await?;
    }
    Ok(())
}

async fn login(
    connection: &mut Connection,
    config: &ServerConfig,
    version: &dyn VersionPack,
    client_protocol: i32,
    slots: std::sync::Arc<Semaphore>,
    locations: &PlayerLocationStore,
) -> Result<()> {
    connection.set_state(ProtocolState::Login);
    if client_protocol != version.protocol_version() {
        let reason = format!(
            "Please use Minecraft {} (protocol {})",
            version.minecraft_version(),
            version.protocol_version()
        );
        send_login_disconnect(connection, version, &reason).await?;
        bail!(
            "client protocol {client_protocol} is incompatible with configured protocol {}",
            version.protocol_version()
        );
    }
    let frame = connection
        .read_frame()
        .await?
        .ok_or_else(|| anyhow::anyhow!("closed before Login Start"))?;
    if frame.id != 0 {
        bail!("expected Login Start, got 0x{:02x}", frame.id);
    }
    let username = match version.parse_login_start(&frame.payload) {
        Ok(username) => username,
        Err(error) => {
            send_login_disconnect(connection, version, "Invalid Login Start").await?;
            return Err(error.into());
        }
    };

    let _slot = match slots.try_acquire_owned() {
        Ok(slot) => slot,
        Err(_) => {
            send_login_disconnect(connection, version, "Server is full").await?;
            bail!("server is full");
        }
    };

    let uuid = offline_uuid(&username);
    let mut location = locations.load(uuid).await?;
    let mut writer = Writer::new();
    writer.uuid(uuid);
    writer.string(&username);
    writer.varint(0);
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::LoginSuccess), &writer.finish())
        .await?;
    info!("[LOGIN] {username} {uuid} authenticated offline");
    match version.login_transition() {
        LoginTransition::Play => connection.set_state(ProtocolState::Play),
        LoginTransition::Configuration => bail!("selected version requires a Configuration handler"),
    }
    debug!("[PLAY] entered for {username}");
    let play_result = play(connection, config, version, &mut location).await;
    let save_result = locations.save(uuid, location).await;
    match (play_result, save_result) {
        (Err(play_error), Err(save_error)) => {
            tracing::error!("[PLAYER] Failed to save {username} location: {save_error}");
            Err(play_error)
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

async fn play(
    connection: &mut Connection,
    config: &ServerConfig,
    version: &dyn VersionPack,
    position: &mut PlayerLocation,
) -> Result<()> {
    let join =
        version.join_game_payload(config.max_players, config.view_distance, config.simulation_distance);
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::JoinGame), &join)
        .await?;
    let mut server_data = Writer::new();
    server_data.bool(true);
    server_data.string(&json!({ "text": config.motd }).to_string());
    server_data.bool(false); // no icon
    server_data.bool(false); // no chat previews
    server_data.bool(false); // offline server does not enforce signed chat
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::ServerData), &server_data.finish())
        .await?;
    let mut health = Writer::new();
    health.f32(20.0);
    health.varint(20);
    health.f32(5.0);
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::UpdateHealth), &health.finish())
        .await?;
    let mut time = Writer::new();
    time.i64(0);
    time.i64(1000);
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::UpdateTime), &time.finish())
        .await?;
    let mut spawn = Writer::new();
    spawn.position(0, 64, 0);
    spawn.f32(0.0);
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::SpawnPosition), &spawn.finish())
        .await?;
    let mut position_packet = Writer::new();
    position_packet.f64(position.x);
    position_packet.f64(position.y);
    position_packet.f64(position.z);
    position_packet.f32(0.0);
    position_packet.f32(0.0);
    position_packet.i8(0);
    position_packet.varint(1);
    position_packet.bool(false); // dismount vehicle
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::PlayerPosition), &position_packet.finish())
        .await?;

    let mut loaded = HashSet::new();
    let mut center = position.chunk();
    send_chunks(connection, version, center.0, center.1, config.view_distance, &mut loaded).await?;
    let mut next_keepalive = 1i64;
    let mut outstanding: Option<(i64, Instant)> = None;
    let mut interval = tokio::time::interval(Duration::from_secs(15));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    interval.tick().await;
    loop {
        tokio::select! {
            frame = connection.read_frame() => {
                let Some(frame) = frame? else { return Ok(()); };
                match version.parse_play(frame.id, &frame.payload)? {
                    PlayInbound::TeleportConfirm(id) => {
                        if id != 1 { bail!("invalid teleport acknowledgement"); }
                    }
                    PlayInbound::KeepAlive(received) => {
                        if outstanding.map(|(id, _)| id) != Some(received) {
                            bail!("invalid keepalive reply");
                        }
                        outstanding = None;
                    }
                    PlayInbound::Position { x, y, z } => {
                        *position = PlayerLocation { x, y, z };
                        tracing::trace!("[MOVE] x={} y={} z={}", position.x, position.y, position.z);
                    }
                    PlayInbound::Look | PlayInbound::OnGround => {}
                    PlayInbound::Other(id) => debug!("[PLAY] Ignoring packet 0x{:02x}", id),
                }
                let chunk = position.chunk();
                if chunk != center {
                    center = chunk;
                    send_chunks(connection, version, center.0, center.1, config.view_distance, &mut loaded).await?;
                }
            }
            _ = interval.tick() => {
                if let Some((_, sent)) = outstanding {
                    if sent.elapsed() >= Duration::from_secs(30) { bail!("keepalive timed out"); }
                    continue;
                }
                let mut writer = Writer::new();
                writer.i64(next_keepalive);
                connection.write_frame(version.clientbound_id(ClientboundPacket::KeepAlive), &writer.finish()).await?;
                outstanding = Some((next_keepalive, Instant::now()));
                next_keepalive = next_keepalive.wrapping_add(1);
            }
        }
    }
}

async fn send_chunks(
    connection: &mut Connection,
    version: &dyn VersionPack,
    x: i32,
    z: i32,
    radius: u8,
    loaded: &mut HashSet<(i32, i32)>,
) -> Result<()> {
    let radius = i32::from(radius);
    let expired: Vec<_> = loaded
        .iter()
        .copied()
        .filter(|&(chunk_x, chunk_z)| {
            (i64::from(chunk_x) - i64::from(x)).abs() > i64::from(radius)
                || (i64::from(chunk_z) - i64::from(z)).abs() > i64::from(radius)
        })
        .collect();
    for chunk in expired {
        loaded.remove(&chunk);
        let mut unload = Writer::new();
        unload.i32(chunk.0);
        unload.i32(chunk.1);
        connection
            .write_frame(version.clientbound_id(ClientboundPacket::UnloadChunk), &unload.finish())
            .await?;
    }
    let mut center = Writer::new();
    center.varint(x);
    center.varint(z);
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::ViewCenter), &center.finish())
        .await?;
    for distance in 0..=radius {
        for dz in -distance..=distance {
            for dx in -distance..=distance {
                if dx.abs() != distance && dz.abs() != distance {
                    continue;
                }
                let chunk = (x.saturating_add(dx), z.saturating_add(dz));
                if loaded.insert(chunk) {
                    let payload = version.chunk_payload(chunk.0, chunk.1);
                    connection
                        .write_frame(version.clientbound_id(ClientboundPacket::ChunkData), &payload)
                        .await?;
                    debug!("[TX] ChunkData chunk=({},{})", chunk.0, chunk.1);
                }
            }
        }
    }
    Ok(())
}

async fn send_login_disconnect(
    connection: &mut Connection,
    version: &dyn VersionPack,
    reason: &str,
) -> Result<()> {
    let mut writer = Writer::new();
    writer.string(&json!({ "text": reason }).to_string());
    connection
        .write_frame(version.clientbound_id(ClientboundPacket::LoginDisconnect), &writer.finish())
        .await?;
    Ok(())
}

fn offline_uuid(username: &str) -> Uuid {
    let hash = md5::compute(format!("OfflinePlayer:{username}").as_bytes());
    let mut bytes = hash.0;
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rustbukkit_config::ServerConfig;
    use rustbukkit_decoding::{Frame, FrameDecoder, Reader};
    use rustbukkit_encoding::{Writer, encode_frame};
    use rustbukkit_protocol::{Connection, VersionRegistry};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Semaphore;
    use tokio::time::{Duration, timeout};
    use uuid::Uuid;

    use super::{offline_uuid, run};
    use crate::player_location::PlayerLocationStore;

    struct TestClient {
        stream:  TcpStream,
        decoder: FrameDecoder,
    }

    impl TestClient {
        async fn send(&mut self, bytes: &[u8]) {
            self.stream.write_all(bytes).await.unwrap();
        }

        async fn next(&mut self) -> Frame {
            timeout(Duration::from_secs(10), async {
                loop {
                    if let Some(frame) = self.decoder.next_frame().unwrap() {
                        return frame;
                    }
                    let mut buf = [0u8; 8192];
                    let count = self.stream.read(&mut buf).await.unwrap();
                    assert!(count > 0, "server closed before expected packet");
                    self.decoder.feed(&buf[..count]).unwrap();
                }
            })
            .await
            .unwrap()
        }
    }

    async fn spawn_session() -> (TestClient, tokio::task::JoinHandle<()>) {
        let root = std::env::temp_dir().join(format!("rustbukkit-player-test-{}", Uuid::new_v4()));
        spawn_session_at(root).await
    }

    async fn spawn_session_at(root: PathBuf) -> (TestClient, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = ServerConfig::parse("mcVersion='1.19.2'\nbind='127.0.0.1:0'\nonlineMode=false\nmaxPlayers=20\nviewDistance=2\nsimulationDistance=2\nmotd='RustBukkit'\n").unwrap();
        let version = VersionRegistry::select("1.19.2").unwrap();
        let handle = tokio::spawn(async move {
            let (stream, peer) = listener.accept().await.unwrap();
            let mut connection = Connection::new(stream, peer);
            let slots = std::sync::Arc::new(Semaphore::new(config.max_players as usize));
            let locations = PlayerLocationStore::new(root);
            let _ = run(&mut connection, &config, version, slots, &locations).await;
        });
        (
            TestClient {
                stream:  TcpStream::connect(address).await.unwrap(),
                decoder: FrameDecoder::new(),
            },
            handle,
        )
    }

    fn handshake(protocol: i32, state: i32) -> Vec<u8> {
        let mut payload = Writer::new();
        payload.varint(protocol);
        payload.string("localhost");
        payload.i16(25565);
        payload.varint(state);
        encode_frame(0, &payload.finish()).unwrap()
    }

    fn login_start(username: &str) -> Vec<u8> {
        let mut start = Writer::new();
        start.string(username);
        start.bool(false);
        start.bool(false);
        let mut input = handshake(760, 2);
        input.extend(encode_frame(0, &start.finish()).unwrap());
        input
    }

    #[test]
    fn offline_uuid_matches_java_name_uuid_from_bytes() {
        assert_eq!(offline_uuid("Notch").to_string(), "b50ad385-829d-3141-a216-7e7d7539ba7f");
    }

    #[tokio::test]
    async fn status_ping_and_pong_work_with_coalesced_input() {
        let (mut client, handle) = spawn_session().await;
        let mut input = handshake(760, 1);
        input.extend(encode_frame(0, &[]).unwrap());
        input.extend(encode_frame(1, &42i64.to_be_bytes()).unwrap());
        client.send(&input).await;
        let status = client.next().await;
        assert_eq!(status.id, 0);
        let mut reader = Reader::new(&status.payload);
        let json: serde_json::Value = serde_json::from_str(reader.string(32_767).unwrap()).unwrap();
        assert_eq!(json["version"]["protocol"], 760);
        assert_eq!(json["version"]["name"], "1.19.2");
        let pong = client.next().await;
        assert_eq!(pong.id, 1);
        assert_eq!(&pong.payload[..], &42i64.to_be_bytes());
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn incompatible_login_gets_version_message() {
        let (mut client, handle) = spawn_session().await;
        client.send(&handshake(761, 2)).await;
        let disconnect = client.next().await;
        assert_eq!(disconnect.id, 0);
        let mut reader = Reader::new(&disconnect.payload);
        assert!(reader.string(1024).unwrap().contains("1.19.2"));
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn login_enters_play_and_starts_flat_chunk_stream() {
        let (mut client, handle) = spawn_session().await;
        client.send(&login_start("Notch")).await;
        let success = client.next().await;
        assert_eq!(success.id, 2);
        let mut reader = Reader::new(&success.payload);
        assert_eq!(reader.uuid().unwrap(), offline_uuid("Notch"));
        assert_eq!(reader.string(16).unwrap(), "Notch");
        assert_eq!(reader.varint().unwrap(), 0);
        assert_eq!(client.next().await.id, 0x25); // Join Game
        let mut saw_chunk = false;
        for _ in 0..8 {
            let frame = client.next().await;
            if frame.id == 0x21 {
                let mut chunk = Reader::new(&frame.payload);
                assert_eq!(chunk.i32().unwrap(), 0);
                assert_eq!(chunk.i32().unwrap(), 0);
                saw_chunk = true;
                break;
            }
        }
        assert!(saw_chunk);
        handle.abort();
    }

    #[tokio::test]
    async fn reconnect_restores_saved_position_and_chunk_center() {
        let root = std::env::temp_dir().join(format!("rustbukkit-player-test-{}", Uuid::new_v4()));
        let (mut client, handle) = spawn_session_at(root.clone()).await;
        client.send(&login_start("Traveler")).await;
        loop {
            if client.next().await.id == 0x39 {
                break;
            }
        }
        let mut movement = Writer::new();
        movement.f64(33.25);
        movement.f64(64.0);
        movement.f64(-20.5);
        movement.bool(true);
        client
            .send(&encode_frame(0x14, &movement.finish()).unwrap())
            .await;
        loop {
            let frame = client.next().await;
            if frame.id == 0x4b {
                let mut center = Reader::new(&frame.payload);
                if (center.varint().unwrap(), center.varint().unwrap()) == (2, -2) {
                    break;
                }
            }
        }
        drop(client);
        handle.await.unwrap();

        let (mut client, handle) = spawn_session_at(root.clone()).await;
        client.send(&login_start("Traveler")).await;
        loop {
            let frame = client.next().await;
            if frame.id == 0x39 {
                let mut position = Reader::new(&frame.payload);
                assert_eq!(position.f64().unwrap(), 33.25);
                assert_eq!(position.f64().unwrap(), 64.0);
                assert_eq!(position.f64().unwrap(), -20.5);
                break;
            }
        }
        loop {
            let frame = client.next().await;
            if frame.id == 0x4b {
                let mut center = Reader::new(&frame.payload);
                assert_eq!((center.varint().unwrap(), center.varint().unwrap()), (2, -2));
                break;
            }
        }
        handle.abort();
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
