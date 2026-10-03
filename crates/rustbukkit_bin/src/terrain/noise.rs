/// Deterministic hash function for 2D coordinates
pub fn hash2d(x: i32, y: i32, seed: u64) -> f64 {
    let mut hash = seed;
    hash = hash.wrapping_mul(73856093);
    hash ^= x as u64;
    hash = hash.wrapping_mul(19349663);
    hash ^= y as u64;
    hash = hash.wrapping_mul(83492791);

    let bits = hash & 0x7fffffff;
    (bits as f64) / (0x7fffffff as f64)
}

/// Perlin-like noise at a given scale
pub fn perlin_noise(x: f64, y: f64, scale: f64, seed: u64) -> f64 {
    let freq = 1.0 / scale;
    let xi = (x * freq).floor() as i32;
    let yi = (y * freq).floor() as i32;

    let xf = (x * freq) - xi as f64;
    let yf = (y * freq) - yi as f64;

    // Fade function (smoothstep)
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

/// Multi-octave Perlin noise (fractional Brownian motion)
pub fn fbm(x: f64, y: f64, octaves: usize, seed: u64) -> f64 {
    let mut value = 0.0;
    let mut amplitude = 1.0;
    let mut frequency = 1.0;
    let mut max_value = 0.0;

    for i in 0..octaves {
        let noise = perlin_noise(x * frequency, y * frequency, 1.0, seed.wrapping_add(i as u64));
        value += noise * amplitude;
        max_value += amplitude;

        amplitude *= 0.5;
        frequency *= 2.0;
    }

    value / max_value
}
