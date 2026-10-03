#![allow(dead_code)]

use std::marker::PhantomData;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use anyhow::Result;
use tracing::{debug, info};

/// A generic thread pool that processes tasks of type T
pub struct ThreadPool<T: Send + 'static> {
    workers: Vec<Worker<T>>,
    sender:  Sender<Option<Box<dyn FnOnce() + Send>>>,
}

struct Worker<T> {
    _id:      usize,
    _thread:  Option<std::thread::JoinHandle<()>>,
    _phantom: PhantomData<T>,
}

impl<T: Send + 'static> ThreadPool<T> {
    pub fn new<S: AsRef<str>>(num_threads: usize, name: S) -> Self {
        assert!(num_threads > 0, "Pool must have at least 1 thread");

        let (sender, receiver) = channel::<Option<Box<dyn FnOnce() + Send>>>();
        let receiver = Arc::new(std::sync::Mutex::new(receiver));

        let mut workers = Vec::with_capacity(num_threads);

        for id in 0..num_threads {
            let receiver = Arc::clone(&receiver);
            let thread_name = format!("{}-{}", name.as_ref(), id);

            let thread = thread::Builder::new()
                .name(thread_name)
                .spawn(move || {
                    loop {
                        let task = {
                            let receiver = receiver.lock().unwrap();
                            receiver.recv().unwrap()
                        };

                        match task {
                            Some(job) => job(),
                            None => break, // Shutdown signal
                        }
                    }
                })
                .unwrap();

            workers.push(Worker {
                _id:      id,
                _thread:  Some(thread),
                _phantom: PhantomData,
            });
        }

        ThreadPool { workers, sender }
    }

    pub fn execute<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce() + Send + 'static,
    {
        self.sender
            .send(Some(Box::new(f)))
            .map_err(|e| anyhow::anyhow!("Failed to send task to thread pool: {}", e))
    }
}

impl<T> Drop for ThreadPool<T>
where
    T: Send + 'static,
{
    fn drop(&mut self) {
        // Send shutdown signal to all workers
        for _ in 0..self.workers.len() {
            self.sender.send(None).unwrap();
        }

        // Wait for all workers to finish
        for worker in &mut self.workers {
            if let Some(thread) = worker._thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

/// Thread pool specifically for chunk generation (4 threads)
#[derive(Clone)]
pub struct ChunkGenThreadPool {
    pool:       Arc<ThreadPool<ChunkGenTask>>,
    // PERF: @atomic : Possible to do with an atomic bool instead of Mutex<bool>?
    init_state: Arc<(AtomicBool, Condvar)>,
    // Arc<(Mutex<bool>, Condvar)>,
}

pub struct ChunkGenTask;

/// Thread pool for plugins (2 threads, reserved for future plugin system)
#[derive(Clone)]
pub struct PluginThreadPool {
    pool: Arc<ThreadPool<PluginTask>>,
}

pub struct PluginTask;

impl ChunkGenThreadPool {
    pub fn new() -> Self {
        let pool = Arc::new(ThreadPool::new(4, "ChunkGen"));
        info!("[STARTUP] Chunk generation thread pool created with 4 workers");
        // let init_state = Arc::new((Mutex::new(false), Condvar::new()));
        let init_state = Arc::new((AtomicBool::new(false), Condvar::new()));
        Self { pool, init_state }
    }

    pub fn execute<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce() + Send + 'static,
    {
        self.pool.execute(f)
    }

    pub fn signal_init_complete(&self) {
        debug!("[CHUNK_GEN_POOL] Signaling initialization complete...");
        let (atomic, condvar) = &*self.init_state;
        // let mut done = lock.lock().unwrap();
        // *done = true;
        atomic.store(true, std::sync::atomic::Ordering::SeqCst);
        debug!("[CHUNK_GEN_POOL] Initialization complete signaled. Notifying all waiting threads...");
        condvar.notify_all();
    }

    pub fn wait_for_init_complete(&self) {
        // original impl.
        // let (lock, condvar) = &*self.init_state; //:  Arc<(Mutex<bool>, Condvar)>,
        // let mut done = lock.lock().unwrap();
        // while !*done {
        //     done = condvar.wait(done).unwrap();
        // }

        // new impl.
        let (atomic, condvar) = &*self.init_state; // : Arc<(AtomicBool, Condvar)>,
        let dummy = Mutex::new(());

        let _guard = condvar
            .wait_while(dummy.lock().unwrap(), |_: &mut ()| !atomic.load(std::sync::atomic::Ordering::SeqCst))
            .unwrap();
    }
}

impl PluginThreadPool {
    pub fn new() -> Self {
        let pool = Arc::new(ThreadPool::new(2, "Plugin"));
        info!("[STARTUP] Plugin thread pool created with 2 workers");
        Self { pool }
    }

    pub fn execute<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce() + Send + 'static,
    {
        self.pool.execute(f)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn test_chunk_gen_pool() {
        let pool = ChunkGenThreadPool::new();
        let counter = Arc::new(AtomicUsize::new(0));

        for _ in 0..10 {
            let c = Arc::clone(&counter);
            pool.execute(move || {
                c.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
        }

        // Give threads time to complete
        thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }
}
