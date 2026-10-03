use std::error::Error;
use std::fmt::{Debug, Display};
use std::io::Error as StdIoError;
use std::net::SocketAddr;
use std::path::Path;
use std::result::Result as StdResult;
use std::sync::Arc;

use anyhow::Result;
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::chunk::ChunkStorage;
use crate::consts::{CHUNK_SEED, GAMELOOP_SLEEP_TICK, WORLD_PATH};
use crate::core::game_loop::GameLoop;
use crate::core::thread_pool::ChunkGenThreadPool;
use crate::error_tracker::{ErrorKey, ErrorTracker};
use crate::player::PlayerData;
use crate::terrain::ChunkGenerator;

// TODO: @dx : for various reasons, we might consider having a chunk_manager: ChunkManager as a single field
// and it's constructed of ChunkStorage + ChunKGenerator + ChunkGenThreadPool etc.
//  and having a trait that both ourselves, and the ChunkManager implement for easier passing
//  around. This would then be easier for
//  dep. injection for say, handler data (and by extension, our handle_X traits take
//  a generic parameter that implements that trait).

pub struct MinecraftServer {
    listener:  TcpListener,
    game_loop: Arc<RwLock<GameLoop>>,
    hdata:     HandlerData,
}

#[derive(Clone)]
pub struct HandlerData {
    pub chunk_storage:  Arc<ChunkStorage>,
    pub error_tracker:  Arc<ErrorTracker>,
    pub chunk_gen_pool: Arc<ChunkGenThreadPool>,
}

impl HandlerData {
    fn new(
        chunk_storage: Arc<ChunkStorage>,
        error_tracker: Arc<ErrorTracker>,
        chunk_gen_pool: Arc<ChunkGenThreadPool>,
    ) -> Self {
        Self {
            chunk_storage,
            error_tracker,
            chunk_gen_pool,
        }
    }
}

impl MinecraftServer {
    pub async fn new<A>(addr: A, error_tracker: Arc<ErrorTracker>) -> Result<Self>
    where
        A: ToSocketAddrs + Display + Debug,
    {
        let listener = TcpListener::bind(&addr).await?;
        info!("[STARTUP] Server listening on {}", addr);

        // Initialize thread pools
        let chunk_gen_pool = Arc::new(ChunkGenThreadPool::new());

        // Create chunk generator and storage with the pool
        let chunk_gen = Arc::new(ChunkGenerator::new::<u64>(CHUNK_SEED));
        let chunk_storage = Arc::new(ChunkStorage::new(chunk_gen, Arc::clone(&chunk_gen_pool))?);

        let handler_data = HandlerData::new(
            Arc::clone(&chunk_storage),
            Arc::clone(&error_tracker),
            Arc::clone(&chunk_gen_pool),
        );

        Ok(Self {
            listener,
            game_loop: Arc::new(RwLock::new(GameLoop::new())),
            hdata: handler_data,
        })
    }

    pub async fn run(self) -> Result<()> {
        // Start hit count reset task (runs every 5 minutes)
        // self.chunk_storage.start_hit_reset_task(); // now done inside ChunkStorage::new()
        // Realistically; this should never happen due to generation
        // running on the constructor of `MinecraftServer::new()`, which itself constructs
        // a `ChunkStorage` that initializes the world folder.
        // The path (until a later refactor) is defined in `consts::WORLD_PATH`.
        if !Path::new(&WORLD_PATH).exists() {
            error!("[STARTUP] World directory does not exist after initialization!");
            error!(
                "[STARTUP] This should never happen unless you've deleted the world folder while the server is setting up."
            );
            error!("What have you done???");
            panic!();
        }

        info!("[STARTUP] Chunk generation thread pool initialization complete.");

        // Spawn game loop task (main thread for game loop and logging)
        tokio::spawn(async move {
            let game_loop = Arc::clone(&self.game_loop);
            loop {
                let mut gl = game_loop.write().await;
                gl.tick(); // function is infallible. Semantically, prefer an Option though
                drop(gl);
                tokio::time::sleep(tokio::time::Duration::from_millis(GAMELOOP_SLEEP_TICK)).await;
            }
        });

        let hdata = self.hdata;

        loop {
            tokio::select! {
                biased; // biased here causes futures to be polled in the order they appear/defined

                // we 'get' res from calling accept() (like if let Some(res) = ... etc.
                res = self.listener.accept() => {
                    // let hd = Arc::clone(&handler_data);
                    let hdata = hdata.clone();
                    handle_accept(hdata, res).await?;
                }

                // Easily add other handlers as needed (sep heartbeat, logging, etc.)
            }
        }
    }
}

async fn handle_accept(
    hdata: HandlerData,
    res: StdResult<(TcpStream, SocketAddr), StdIoError>,
) -> Result<()> {
    if let Err(e) = &res {
        error!("[NETWORK] Accept error: {}", e);
        let key = ErrorKey::new("NETWORK", "accept_failed");
        if hdata.error_tracker.record_error(key) {
            error!("[SHUTDOWN] Initiating safe shutdown due to critical errors");
            return Ok(());
        }
    }

    let (socket, addr) = res.unwrap(); // if it's not an err above, we can unwrap safely
    info!("[CONNECTION] New connection from {}", addr);

    tokio::spawn(async move {
        if let Err(e) = handle_client(socket, hdata).await {
            error!("[CLIENT] Connection error: {}", e);
        }
    });

    Ok(())
}

async fn handle_client(socket: TcpStream, hd: HandlerData) -> Result<()> {
    let player = PlayerData::new(socket).await?;
    player.handle(hd).await?;
    Ok(())
}
