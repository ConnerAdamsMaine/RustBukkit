use anyhow::{Result, bail};
use rustbukkit_decoding::{Frame, FrameDecoder, Reader};
use rustbukkit_encoding::Writer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Duration, timeout};
use uuid::Uuid;

/// Minimal 26.3 protocol client used by integration tests.
///
/// This speaks the wire protocol directly; it is not a substitute for an
/// unmodified Minecraft client, but it exercises the real server code paths.
///
/// A background actor task owns both socket halves: it drains the socket while
/// the test is not explicitly reading (without that the server's writes, large
/// chunk payloads, back up in the TCP buffers while a test sleeps), and it
/// closes the connection gracefully on drop. A socket closed with unread data
/// in its receive buffer sends RST instead of FIN, which destroys the data the
/// server has already sent to that peer; the graceful close below prevents
/// that from discarding the server's pending inbound frames.
pub struct TestClient {
    commands: tokio::sync::mpsc::UnboundedSender<Command>,
    frames:   tokio::sync::mpsc::UnboundedReceiver<Result<Option<Frame>>>,
}

enum Command {
    /// Write one encoded frame to the socket, in order.
    Send(Vec<u8>),
    /// Close the connection gracefully: FIN, then drain until the peer
    /// closes or the drain deadline passes.
    Close,
}

impl Drop for TestClient {
    fn drop(&mut self) {
        // The actor keeps running on the test's runtime: it finishes writing,
        // half-closes, and drains the socket so the server observes a clean
        // disconnect with all previously sent data intact.
        let _ = self.commands.send(Command::Close);
    }
}

impl TestClient {
    pub async fn connect(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true)?;
        let (mut reader, mut writer) = tokio::io::split(stream);
        let (commands_tx, mut commands_rx) = tokio::sync::mpsc::unbounded_channel();
        let (frames_tx, frames_rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut decoder = FrameDecoder::new();
            let mut read_buf = vec![0u8; 8192];
            let mut commands_open = true;
            let mut closing = false;
            // A read that outlives the drain deadline surfaces as a timed-out
            // I/O error; the loop then releases the socket.
            const DRAIN_TIMEOUT: Duration = Duration::from_millis(500);
            loop {
                let read = tokio::select! {
                    biased;
                    command = commands_rx.recv(), if commands_open && !closing => {
                        match command {
                            Some(Command::Send(frame)) => {
                                if let Err(error) = writer.write_all(&frame).await {
                                    let _ = frames_tx.send(Err(anyhow::Error::from(error)));
                                    return;
                                }
                                if let Err(error) = writer.flush().await {
                                    let _ = frames_tx.send(Err(anyhow::Error::from(error)));
                                    return;
                                }
                                continue;
                            }
                            Some(Command::Close) => {
                                // Half-close first so the server keeps any
                                // frames it has already received; then drain.
                                let _ = writer.shutdown().await;
                                closing = true;
                                continue;
                            }
                            None => commands_open = false,
                        }
                        continue;
                    }
                    read = async {
                        if closing {
                            timeout(DRAIN_TIMEOUT, reader.read(&mut read_buf))
                                .await
                                .unwrap_or(Err(std::io::Error::new(
                                    std::io::ErrorKind::TimedOut,
                                    "drain deadline elapsed",
                                )))
                        } else {
                            reader.read(&mut read_buf).await
                        }
                    } => read,
                };
                match read {
                    Ok(0) => {
                        let _ = frames_tx.send(Ok(None));
                        return;
                    }
                    Ok(count) => {
                        if let Err(error) = decoder.feed(&read_buf[..count]) {
                            let _ = frames_tx.send(Err(anyhow::Error::from(error)));
                            return;
                        }
                        loop {
                            match decoder.next_frame() {
                                Ok(Some(frame)) => {
                                    if frames_tx.send(Ok(Some(frame))).is_err() {
                                        return;
                                    }
                                }
                                Ok(None) => break,
                                Err(error) => {
                                    let _ = frames_tx.send(Err(anyhow::Error::from(error)));
                                    return;
                                }
                            }
                        }
                    }
                    Err(_closed) if closing => return,
                    Err(error) => {
                        let _ = frames_tx.send(Err(anyhow::Error::from(error)));
                        return;
                    }
                }
            }
        });
        Ok(Self {
            commands: commands_tx,
            frames:   frames_rx,
        })
    }

    pub async fn read_frame(&mut self) -> Result<Option<Frame>> {
        // The channel closes once the actor has terminated; from the test's
        // perspective that is the same as an idle closed connection.
        self.frames.recv().await.unwrap_or(Ok(None))
    }

    pub async fn write_frame(&mut self, id: i32, payload: &[u8]) -> Result<()> {
        let frame = rustbukkit_encoding::encode_frame(id, payload)?;
        // Ordered fire-and-forget: the actor applies writes in send order;
        // write failures surface later as an Err frame or read error.
        self.commands
            .send(Command::Send(frame))
            .map_err(|_| anyhow::anyhow!("client actor stopped"))?;
        Ok(())
    }

    pub async fn handshake(&mut self, protocol_version: i32, next_state: i32) -> Result<()> {
        let mut writer = Writer::new();
        writer.varint(protocol_version);
        writer.string("localhost");
        writer.i16(25565);
        writer.varint(next_state);
        self.write_frame(0, &writer.finish()).await
    }

    pub async fn login_start(&mut self, username: &str) -> Result<()> {
        let mut writer = Writer::new();
        writer.string(username);
        writer.uuid(Uuid::nil());
        self.write_frame(0, &writer.finish()).await
    }

    pub async fn login_acknowledged(&mut self) -> Result<()> {
        self.write_frame(0x03, &[]).await
    }

    pub async fn client_information(&mut self) -> Result<()> {
        let mut writer = Writer::new();
        writer.string("en_us");
        writer.i8(8);
        writer.varint(0);
        writer.bool(false);
        writer.u8(0x7f);
        writer.varint(0);
        writer.bool(false);
        writer.bool(false);
        writer.varint(0);
        self.write_frame(0, &writer.finish()).await
    }

    pub async fn known_packs(&mut self) -> Result<()> {
        let mut writer = Writer::new();
        writer.varint(1);
        writer.string("minecraft");
        writer.string("core");
        writer.string("26.3");
        self.write_frame(7, &writer.finish()).await
    }

    pub async fn finish_configuration(&mut self) -> Result<()> {
        self.write_frame(3, &[]).await
    }

    pub async fn expect_packet(&mut self, expected_id: i32, state_name: &str) -> Result<Frame> {
        match self.read_frame().await? {
            Some(frame) => {
                if frame.id != expected_id {
                    bail!("Expected packet ID {:#x} in {}, got {:#x}", expected_id, state_name, frame.id);
                }
                Ok(frame)
            }
            None => bail!("Connection closed while waiting for {} packet", state_name),
        }
    }

    pub async fn expect_any_packet(&mut self) -> Result<Option<Frame>> {
        self.read_frame().await
    }

    /// Read and discard frames for `duration`, keeping the server's write
    /// pipe unblocked while the test waits (cooldowns, movement processing).
    /// Returns the number of frames discarded.
    pub async fn drain_for(&mut self, duration: Duration) -> Result<usize> {
        let deadline = tokio::time::Instant::now() + duration;
        let mut discarded = 0usize;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Ok(discarded);
            }
            match timeout(remaining, self.read_frame()).await {
                Ok(Ok(Some(_))) => discarded += 1,
                Ok(Ok(None)) => bail!("connection closed while draining"),
                Ok(Err(error)) => return Err(error),
                Err(_) => return Ok(discarded),
            }
        }
    }

    /// Read frames until `id` arrives, ignoring unrelated packets.
    pub async fn wait_for(&mut self, id: i32, limit: Duration) -> Result<Frame> {
        let deadline = tokio::time::Instant::now() + limit;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                bail!("timed out waiting for packet {id:#x}");
            }
            match timeout(remaining, self.read_frame()).await {
                Ok(Ok(Some(frame))) if frame.id == id => return Ok(frame),
                Ok(Ok(Some(_))) => continue,
                Ok(Ok(None)) => bail!("connection closed while waiting for packet {id:#x}"),
                Ok(Err(error)) => return Err(error),
                Err(_) => bail!("timed out waiting for packet {id:#x}"),
            }
        }
    }

    /// Drive the full handshake, login and configuration exchange, then return
    /// the Join Game packet that starts the play state.
    pub async fn join(&mut self, username: &str) -> Result<Frame> {
        self.handshake(777, 2).await?;
        self.login_start(username).await?;
        self.expect_packet(0x02, "Login Success").await?;
        self.login_acknowledged().await?;
        // Like a vanilla client, reply with client information and known packs
        // immediately; the server then sends registry data and finishes
        // configuration.
        self.client_information().await?;
        self.known_packs().await?;
        self.wait_for(0x03, Duration::from_secs(10)).await?;
        self.finish_configuration().await?;
        self.expect_packet(0x32, "Join Game").await
    }

    pub async fn wait_for_position(&mut self, limit: Duration) -> Result<(f64, f64, f64)> {
        let frame = self.wait_for(0x49, limit).await?;
        let mut reader = Reader::new(&frame.payload);
        let _teleport_id = reader.varint()?;
        Ok((reader.f64()?, reader.f64()?, reader.f64()?))
    }

    pub async fn send_position(&mut self, x: f64, y: f64, z: f64) -> Result<()> {
        let mut writer = Writer::new();
        writer.f64(x);
        writer.f64(y);
        writer.f64(z);
        writer.u8(1); // on_ground
        self.write_frame(30, &writer.finish()).await
    }

    pub async fn dig(&mut self, x: i32, y: i32, z: i32) -> Result<()> {
        let mut writer = Writer::new();
        writer.varint(3); // StopDestroyBlock (finish break)
        writer.position(x, y, z);
        writer.varint(0); // down face; unused for a finish action
        writer.varint(1); // sequence
        self.write_frame(41, &writer.finish()).await
    }

    pub async fn use_item_on(&mut self, x: i32, y: i32, z: i32, face: i32) -> Result<()> {
        let mut writer = Writer::new();
        writer.varint(0); // main hand
        writer.position(x, y, z);
        writer.varint(face);
        writer.f32(0.5);
        writer.f32(0.5);
        writer.f32(0.5);
        writer.bool(false); // not inside block
        writer.bool(false); // no world-border hit
        writer.varint(1); // sequence
        self.write_frame(66, &writer.finish()).await
    }

    /// Serverbound `attack` (ID 1) on a target entity id.
    pub async fn attack(&mut self, entity_id: i32) -> Result<()> {
        let mut writer = Writer::new();
        writer.varint(entity_id);
        self.write_frame(1, &writer.finish()).await
    }

    /// Serverbound `client_command` (ID 12) with `PerformRespawn` (action 0).
    pub async fn respawn_request(&mut self) -> Result<()> {
        let mut writer = Writer::new();
        writer.varint(0);
        self.write_frame(12, &writer.finish()).await
    }

    /// Serverbound `container_click` (ID 18) on the player-inventory window.
    /// Like a vanilla client the prediction payloads are sent, but empty; the
    /// server is authoritative over the result.
    pub async fn container_click(&mut self, slot: i16, button: i8, mode: i32) -> Result<()> {
        let mut writer = Writer::new();
        writer.varint(0); // window 0: player inventory
        writer.varint(0); // state id; ignored by the authoritative server
        writer.i16(slot);
        writer.i8(button);
        writer.varint(mode);
        writer.varint(0); // no changed-slot predictions
        writer.bool(false); // no carried item predicted
        self.write_frame(18, &writer.finish()).await
    }

    /// Wait for the next clientbound `container_set_content` (ID 18) and
    /// decode the authoritative window contents.
    pub async fn wait_for_container_content(&mut self, limit: Duration) -> Result<ContainerContent> {
        let frame = self.wait_for(18, limit).await?;
        let mut reader = Reader::new(&frame.payload);
        let container_id = reader.varint()?;
        let state_id = reader.varint()?;
        let count = reader.varint()?;
        let mut slots = Vec::with_capacity(count as usize);
        for _ in 0..count {
            slots.push(read_slot(&mut reader)?);
        }
        let carried = read_slot(&mut reader)?;
        Ok(ContainerContent {
            container_id,
            state_id,
            slots,
            carried,
        })
    }
}

/// Decoded clientbound `container_set_content` payload.
#[derive(Clone, Debug, PartialEq)]
pub struct ContainerContent {
    pub container_id: i32,
    pub state_id:     i32,
    pub slots:        Vec<Option<(i32, u8)>>,
    pub carried:      Option<(i32, u8)>,
}

/// One network Slot: a VarInt count; when non-zero, item ID plus the two
/// component counts (no components are sent by this server yet).
fn read_slot(reader: &mut Reader<'_>) -> Result<Option<(i32, u8)>> {
    let count = reader.varint()?;
    if count == 0 {
        return Ok(None);
    }
    let item = reader.varint()?;
    let _added_components = reader.varint()?;
    let _removed_components = reader.varint()?;
    Ok(Some((item, count as u8)))
}
