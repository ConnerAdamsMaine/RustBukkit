use crate::BlockKind;

const DEFAULT_SEED: u64 = 12_345;
pub const SEA_LEVEL: i32 = 100;

#[derive(Debug, Clone, Copy)]
pub(crate) struct TerrainColumn {
    height: i32,
    biome:  Biome,
}

#[derive(Debug, Clone, Copy)]
enum Biome {
    Ocean,
    Beach,
    Plains,
    Forest,
    Mountain,
    Snow,
    SnowMountain,
}

#[derive(Debug, Clone, Copy)]
pub struct TerrainGenerator {
    seed: u64,
}

impl Default for TerrainGenerator {
    fn default() -> Self {
        Self::new(DEFAULT_SEED)
    }
}

impl TerrainGenerator {
    pub const fn new(seed: u64) -> Self {
        Self { seed }
    }

    pub const fn seed(self) -> u64 {
        self.seed
    }

    pub(crate) fn column(self, x: i32, z: i32) -> TerrainColumn {
        let elevation = self.elevation(x, z);
        let slope_x = (self.elevation(x + 1, z) - self.elevation(x - 1, z)) / 2.0;
        let slope_z = (self.elevation(x, z + 1) - self.elevation(x, z - 1)) / 2.0;
        let slope = (slope_x * slope_x + slope_z * slope_z).sqrt();
        let height = (((elevation + 1.0) / 2.0 * 190.0) + 10.0) as i32;
        TerrainColumn {
            height,
            biome: Self::biome(elevation, slope),
        }
    }

    pub(crate) fn block_at(self, x: i32, y: i32, z: i32) -> BlockKind {
        self.column(x, z).block_at(y)
    }

    pub(crate) fn spawn_y(self, x: i32, z: i32) -> i32 {
        self.column(x, z).spawn_y()
    }

    fn elevation(self, x: i32, z: i32) -> f64 {
        let x = f64::from(x);
        let z = f64::from(z);
        let large = fbm(x / 512.0, z / 512.0, 3, self.seed);
        let medium = fbm(x / 128.0, z / 128.0, 2, self.seed.wrapping_add(1));
        let small = perlin_noise(x / 32.0, z / 32.0, self.seed.wrapping_add(2));
        let mut elevation = (large * 0.6 + medium * 0.3 + small * 0.1) * 2.0 - 1.0;

        let plate_scale = 256.0;
        let distance_x = (x.rem_euclid(plate_scale) - plate_scale / 2.0).abs() / (plate_scale / 8.0);
        let distance_z = (z.rem_euclid(plate_scale) - plate_scale / 2.0).abs() / (plate_scale / 8.0);
        let distance_to_boundary = distance_x.min(distance_z);
        if distance_to_boundary < 1.0 {
            elevation += (1.0 - distance_to_boundary) * 0.15;
        }
        elevation.clamp(-1.0, 1.0)
    }

    fn biome(elevation: f64, slope: f64) -> Biome {
        if elevation > 0.7 {
            if slope > 0.3 {
                Biome::SnowMountain
            } else {
                Biome::Snow
            }
        } else if elevation > 0.5 {
            if slope > 0.25 {
                Biome::Mountain
            } else {
                Biome::Forest
            }
        } else if elevation > 0.1 {
            if slope > 0.2 {
                Biome::Mountain
            } else if elevation > 0.3 {
                Biome::Forest
            } else {
                Biome::Plains
            }
        } else if elevation > -0.05 {
            Biome::Beach
        } else {
            Biome::Ocean
        }
    }
}

impl TerrainColumn {
    fn spawn_y(self) -> i32 {
        self.height.max(SEA_LEVEL)
    }

    pub(crate) fn block_at(self, y: i32) -> BlockKind {
        if y == -64 {
            return BlockKind::Bedrock;
        }
        if (-63..self.height).contains(&y) {
            let depth = self.height - y;
            return match self.biome {
                Biome::Ocean => BlockKind::Stone,
                Biome::Beach if depth <= 2 => BlockKind::Sand,
                Biome::Plains if depth == 1 => BlockKind::GrassBlock,
                Biome::Plains if depth <= 4 => BlockKind::Dirt,
                Biome::Forest if depth == 1 => BlockKind::GrassBlock,
                Biome::Forest if depth <= 5 => BlockKind::Dirt,
                Biome::Mountain if depth <= 2 => BlockKind::Stone,
                Biome::Mountain if depth <= 6 => BlockKind::Cobblestone,
                Biome::Snow if depth == 1 => BlockKind::GrassBlock,
                Biome::Snow if depth <= 3 => BlockKind::Dirt,
                Biome::SnowMountain => BlockKind::Stone,
                _ => BlockKind::Stone,
            };
        }
        if self.height < SEA_LEVEL && y < SEA_LEVEL {
            BlockKind::Water
        } else {
            BlockKind::Air
        }
    }
}

fn hash2d(x: i32, y: i32, seed: u64) -> f64 {
    let mut hash = seed;
    hash = hash.wrapping_mul(73_856_093);
    hash ^= x as u64;
    hash = hash.wrapping_mul(19_349_663);
    hash ^= y as u64;
    hash = hash.wrapping_mul(83_492_791);
    f64::from((hash & 0x7fff_ffff) as u32) / f64::from(0x7fff_ffffu32)
}

fn perlin_noise(x: f64, y: f64, seed: u64) -> f64 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let xf = x - f64::from(xi);
    let yf = y - f64::from(yi);
    let u = xf * xf * (3.0 - 2.0 * xf);
    let v = yf * yf * (3.0 - 2.0 * yf);
    let n00 = hash2d(xi, yi, seed);
    let n10 = hash2d(xi + 1, yi, seed);
    let n01 = hash2d(xi, yi + 1, seed);
    let n11 = hash2d(xi + 1, yi + 1, seed);
    let nx0 = n00 * (1.0 - u) + n10 * u;
    let nx1 = n01 * (1.0 - u) + n11 * u;
    nx0 * (1.0 - v) + nx1 * v
}

fn fbm(x: f64, y: f64, octaves: usize, seed: u64) -> f64 {
    let mut value = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut max_value = 0.0;
    for octave in 0..octaves {
        value += perlin_noise(x * frequency, y * frequency, seed.wrapping_add(octave as u64)) * amplitude;
        max_value += amplitude;
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    value / max_value
}

#[cfg(test)]
mod tests {
    use super::TerrainGenerator;
    use crate::BlockKind;

    #[test]
    fn seeded_terrain_is_deterministic_and_varies_across_columns() {
        let first = TerrainGenerator::default();
        let same_seed = TerrainGenerator::new(12_345);
        assert_eq!(first.column(0, 0).height, same_seed.column(0, 0).height);
        let heights: std::collections::HashSet<_> = (-16..16)
            .flat_map(|x| (-16..16).map(move |z| first.column(x, z).height))
            .collect();
        assert!(heights.len() > 1);
    }

    #[test]
    fn terrain_supports_negative_coordinates_and_has_a_spawn_surface() {
        let terrain = TerrainGenerator::default();
        let y = terrain.spawn_y(-17, -33);
        assert!((100..=200).contains(&y));
        assert_ne!(terrain.column(-17, -33).block_at(y - 1), BlockKind::Air);
        assert_eq!(terrain.column(-17, -33).block_at(y), BlockKind::Air);
        assert_eq!(terrain.block_at(-17, -64, -33), BlockKind::Bedrock);
    }
}
