#![allow(dead_code)]

/// Represents a Minecraft world/dimension
pub struct MinecraftWorld {
    name: String,
}

impl MinecraftWorld {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

impl Default for MinecraftWorld {
    fn default() -> Self {
        let name = crate::consts::WORLD_PATH
            .split('/')
            .next_back()
            .unwrap_or("world")
            .to_string();
        Self { name }
    }
}
