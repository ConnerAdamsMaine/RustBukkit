#![allow(dead_code)]

use std::sync::atomic::AtomicBool;
use std::time::Instant;

use anyhow::Result;

use crate::consts::GAMELOOP_TICK_RATE_DURATION; // replaces 'TICK_RATE'
// use crate::GAMELOOP_TICK_RATE_DURATION; // replaces 'TICK_DURATION'

pub struct GameLoop {
    tick_count: u64,
    last_tick:  Instant,
    // atomic:     AtomicBool,
}

impl GameLoop {
    pub fn new() -> Self {
        Self {
            tick_count: 0,
            last_tick:  Instant::now(),
            // atomic:     AtomicBool::new(false),
        }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_tick);

        if elapsed >= GAMELOOP_TICK_RATE_DURATION {
            self.tick_count += 1;
            self.last_tick = now;

            // TODO: @update_fns : Implement the actual update functions
            // Perform tick updates
            // self.update_players();
            // self.update_entities();
            // self.update_physics();

            tracing::trace!("Tick {}", self.tick_count);
        }

        // Ok(())
    }

    fn update_players(&mut self) {
        // Update player positions, health, etc.
        todo!("Need to implement player updates");
    }

    fn update_entities(&mut self) {
        // Update mobs, projectiles, etc.
        todo!("Need to implement entity updates");
    }

    fn update_physics(&mut self) {
        // Apply gravity, collisions, etc.
        todo!("Need to implement physics updates");
    }

    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }
}
