#![allow(dead_code)]
use crate::terrain::noise;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Biome {
    Ocean,
    Beach,
    Plains,
    Forest,
    Mountain,
    Snow,
    SnowMountain,
    Desert,
}

pub struct HeightMap {
    data:   Vec<Vec<f64>>,
    width:  usize,
    height: usize,
    seed:   u64,
}

impl HeightMap {
    pub fn new(width: usize, height: usize, seed: u64) -> Self {
        let mut hm = Self {
            data: vec![vec![0.0; width]; height],
            width,
            height,
            seed,
        };
        hm.generate();
        hm
    }

    fn generate(&mut self) {
        // Base continental noise
        // PERF: @nested : Loop moved to thread engine
        for y in 0..self.height {
            for x in 0..self.width {
                let fx = x as f64;
                let fy = y as f64;

                // Multi-scale noise for continents
                let large_scale = noise::fbm(fx / 512.0, fy / 512.0, 3, self.seed);
                let medium_scale = noise::fbm(fx / 128.0, fy / 128.0, 2, self.seed.wrapping_add(1));
                let small_scale = noise::perlin_noise(fx / 32.0, fy / 32.0, 1.0, self.seed.wrapping_add(2));

                // Combine scales with weights
                let height = large_scale * 0.6 + medium_scale * 0.3 + small_scale * 0.1;
                self.data[y][x] = height.clamp(-1.0, 1.0);
            }
        }

        // Simulate plate collisions for mountain ranges
        self.apply_plate_collisions();

        // Apply erosion
        self.apply_erosion();
    }

    fn apply_plate_collisions(&mut self) {
        // Simulate collision zones as mountain ridges
        // PERF: @nested : Loop moved to thread engine
        for y in 0..self.height {
            for x in 0..self.width {
                let fx = x as f64;
                let fy = y as f64;

                // Create collision zones at regular intervals
                let plate_scale = 256.0;
                let collision_strength = 0.15;

                let distance_to_boundary_x =
                    (fx % plate_scale - plate_scale / 2.0).abs() / (plate_scale / 8.0);
                let distance_to_boundary_y =
                    (fy % plate_scale - plate_scale / 2.0).abs() / (plate_scale / 8.0);

                if distance_to_boundary_x < 1.0 || distance_to_boundary_y < 1.0 {
                    let boundary_boost =
                        (1.0 - distance_to_boundary_x.min(distance_to_boundary_y)) * collision_strength;
                    self.data[y][x] = (self.data[y][x] + boundary_boost).clamp(-1.0, 1.0);
                }
            }
        }
    }

    fn apply_erosion(&mut self) {
        // Simple thermal erosion: flatten steep slopes
        let iterations = 2;
        let erosion_amount = 0.1;

        for _ in 0..iterations {
            let mut new_data = self.data.clone();
            // PERF: @nested : Loop moved to thread engine
            (1..(self.height - 1)).for_each(|y| {
                (1..(self.width - 1)).for_each(|x| {
                    let center = self.data[y][x];
                    let neighbors = [
                        self.data[y - 1][x],
                        self.data[y + 1][x],
                        self.data[y][x - 1],
                        self.data[y][x + 1],
                    ];

                    let max_neighbor = neighbors.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    let min_neighbor = neighbors.iter().copied().fold(f64::INFINITY, f64::min);

                    if center > max_neighbor + erosion_amount {
                        new_data[y][x] -= erosion_amount * 0.5;
                    }
                    if center < min_neighbor - erosion_amount {
                        new_data[y][x] += erosion_amount * 0.5;
                    }
                });
            });

            self.data = new_data;
        }
    }

    pub fn get(&self, x: usize, y: usize) -> f64 {
        if x < self.width && y < self.height {
            self.data[y][x]
        } else {
            0.0
        }
    }

    pub fn get_slope(&self, x: usize, y: usize) -> f64 {
        if x == 0 || x == self.width - 1 || y == 0 || y == self.height - 1 {
            return 0.0;
        }

        let dx = (self.data[y][x + 1] - self.data[y][x - 1]) / 2.0;
        let dy = (self.data[y + 1][x] - self.data[y - 1][x]) / 2.0;

        (dx * dx + dy * dy).sqrt()
    }
}

pub struct BiomeMap {
    data:   Vec<Vec<Biome>>,
    width:  usize,
    height: usize,
}

impl From<&HeightMap> for BiomeMap {
    fn from(height_map: &HeightMap) -> Self {
        let width = 512; // Match height map size
        let height = 512;
        let mut data = vec![vec![Biome::Plains; width]; height];
        // PERF: @nested : Loop moved to thread engine
        (0..height).for_each(|y| {
            (0..width).for_each(|x| {
                let elevation = height_map.get(x, y);
                let slope = height_map.get_slope(x, y);

                data[y][x] = Self::determine_biome(elevation, slope);
            });
        });

        Self { data, width, height }
    }
}

impl From<&HeightMap> for Option<BiomeMap> {
    fn from(height_map: &HeightMap) -> Self {
        Some(BiomeMap::from(height_map))
    }
}

impl BiomeMap {
    // pub fn from_height_map(height_map: &HeightMap) -> Self {
    //     let width = 512; // Match height map size
    //     let height = 512;
    //     let mut data = vec![vec![Biome::Plains; width]; height];
    //     // PERF: @nested : Loop moved to thread engine
    //     (0..height).for_each(|y| {
    //         (0..width).for_each(|x| {
    //             let elevation = height_map.get(x, y);
    //             let slope = height_map.get_slope(x, y);
    //
    //             data[y][x] = Self::determine_biome(elevation, slope);
    //         });
    //     });
    //
    //     Self { data, width, height }
    // }

    fn determine_biome(elevation: f64, slope: f64) -> Biome {
        // Snowline at elevation 0.7
        if elevation > 0.7 {
            if slope > 0.3 {
                Biome::SnowMountain
            } else {
                Biome::Snow
            }
        }
        // Mountains above 0.5
        else if elevation > 0.5 {
            if slope > 0.25 {
                Biome::Mountain
            } else {
                Biome::Forest
            }
        }
        // Plains/Forest middle elevation
        else if elevation > 0.1 {
            if slope > 0.2 {
                Biome::Mountain
            } else if elevation > 0.3 {
                Biome::Forest
            } else {
                Biome::Plains
            }
        }
        // Beach/coastal
        else if elevation > -0.05 {
            Biome::Beach
        }
        // Ocean
        else {
            Biome::Ocean
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Biome {
        if x < self.width && y < self.height {
            self.data[y][x]
        } else {
            Biome::Ocean
        }
    }
}
