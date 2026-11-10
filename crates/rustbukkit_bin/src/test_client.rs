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
pub struct TestClient {
    stream:   TcpStream,
    decoder:  FrameDecoder,
    read_buf: Vec<u8>,
}

impl TestClient {
    pub async fn connect(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            decoder: FrameDecoder::new(),
            read_buf: vec![0u8; 8192],
        })
    }

    pub async fn read_frame(&mut self) -> Result<Option<Frame>> {
        loop {
            if let Some(frame) = self.decoder.next_frame()? {
                return Ok(Some(frame));
            }
            let count = self.stream.read(&mut self.read_buf).await?;
            if count == 0 {
                return if self.decoder.is_empty() {
                    Ok(None)
                } else {
                    bail!("connection closed with partial frame")
                };
            }
            self.decoder.feed(&self.read_buf[..count])?;
        }
    }

    pub async fn write_frame(&mut self, id: i32, payload: &[u8]) -> Result<()> {
        let frame = rustbukkit_encoding::encode_frame(id, payload)?;
        self.stream.write_all(&frame).await?;
        self.stream.flush().await?;
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
        // Brand, feature flags and known packs arrive before the finish packet.
        self.wait_for(0x03, Duration::from_secs(10)).await?;
        self.client_information().await?;
        self.known_packs().await?;
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
}
