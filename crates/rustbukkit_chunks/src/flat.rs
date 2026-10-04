#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Air,
    Bedrock,
    Dirt,
    GrassBlock,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FlatWorld;

impl FlatWorld {
    pub const HEIGHT: i32 = 384;
    pub const MIN_Y: i32 = -64;
    pub const SPAWN_Y: f64 = 64.0;
    pub const SURFACE_Y: i32 = 63;

    pub fn block_at(&self, y: i32) -> BlockKind {
        match y {
            59 => BlockKind::Bedrock,
            60..=62 => BlockKind::Dirt,
            Self::SURFACE_Y => BlockKind::GrassBlock,
            _ => BlockKind::Air,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BlockKind, FlatWorld};

    #[test]
    fn flat_layers_are_deterministic() {
        let world = FlatWorld;
        assert_eq!(world.block_at(-64), BlockKind::Air);
        assert_eq!(world.block_at(59), BlockKind::Bedrock);
        assert_eq!(world.block_at(60), BlockKind::Dirt);
        assert_eq!(world.block_at(63), BlockKind::GrassBlock);
        assert_eq!(world.block_at(64), BlockKind::Air);
    }
}
