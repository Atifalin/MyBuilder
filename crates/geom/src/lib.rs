//! Geometry foundations: integer grid snapping and deterministic noise.
//!
//! Core convention (glTF style): meters, right-handed, Y-up. Two-dimensional
//! plans (world maps, floor plans) live in the XZ plane, so a "2D point" is
//! `(x, z)`. Integer geometry snaps meters to a millimetre grid so that
//! topology operations are exact and identical on every platform.
//!
//! The Perlin implementation is hand-written and uses only `+`, `-`, `*`,
//! `/` and `floor`: no `sin`/`cos`/`exp`/`powf`/`sqrt` (libm differs across
//! operating systems) and no fused multiply-add. It must never change.

/// Millimetres per meter: the integer grid quantum.
pub const UNITS_PER_METER: i64 = 1000;

/// A point on the integer millimetre grid in the XZ plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IPoint {
    pub x: i64,
    pub z: i64,
}

/// Snap meters to the integer millimetre grid (round half away from zero).
pub fn snap(m: f64) -> i64 {
    (m * UNITS_PER_METER as f64).round() as i64
}

/// Convert an integer grid coordinate back to meters.
pub fn to_meters(i: i64) -> f64 {
    i as f64 / UNITS_PER_METER as f64
}

const GRAD: [(f64, f64); 8] = [
    (1.0, 0.0),
    (-1.0, 0.0),
    (0.0, 1.0),
    (0.0, -1.0),
    (FRAC_1_SQRT_2, FRAC_1_SQRT_2),
    (-FRAC_1_SQRT_2, FRAC_1_SQRT_2),
    (FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
    (-FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
];

// 1/sqrt(2) as a compile-time constant so no runtime sqrt is needed.
const FRAC_1_SQRT_2: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// Deterministic 2D Perlin gradient noise.
///
/// The permutation table is a 256-entry Fisher-Yates shuffle driven by
/// [`mb_seed::Rng`], so the field depends only on the seed. All evaluation
/// happens in world coordinates (meters), never in tile indices.
#[derive(Clone, Debug)]
pub struct Perlin {
    perm: [u8; 256],
}

impl Perlin {
    pub fn new(seed: u64) -> Self {
        let mut perm = [0u8; 256];
        for (i, p) in perm.iter_mut().enumerate() {
            *p = i as u8;
        }
        let mut rng = mb_seed::Rng::new(seed);
        for i in (1..256usize).rev() {
            let j = rng.range_u64(i as u64 + 1) as usize;
            perm.swap(i, j);
        }
        Perlin { perm }
    }

    fn hash(&self, x: i64, z: i64) -> usize {
        let xi = self.perm[(x & 255) as usize] as usize;
        self.perm[(xi + (z & 255) as usize) & 255] as usize
    }

    /// Gradient noise at `(x, z)`. Roughly in [-1, 1]; the theoretical
    /// extrema are ±1 at lattice points where the gradient aligns.
    pub fn noise(&self, x: f64, z: f64) -> f64 {
        let xf = x.floor();
        let zf = z.floor();
        let (xi, zi) = (xf as i64, zf as i64);
        let (u, v) = (x - xf, z - zf);
        let (fu, fv) = (fade(u), fade(v));

        let n00 = self.grad_dot(xi, zi, u, v);
        let n10 = self.grad_dot(xi + 1, zi, u - 1.0, v);
        let n01 = self.grad_dot(xi, zi + 1, u, v - 1.0);
        let n11 = self.grad_dot(xi + 1, zi + 1, u - 1.0, v - 1.0);

        lerp(lerp(n00, n10, fu), lerp(n01, n11, fu), fv)
    }

    fn grad_dot(&self, xi: i64, zi: i64, dx: f64, dz: f64) -> f64 {
        let (gx, gz) = GRAD[self.hash(xi, zi) & 7];
        gx * dx + gz * dz
    }
}

// Fade curve 6t^5 - 15t^4 + 10t^3, evaluated without powf.
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + t * (b - a)
}

/// Fractal Brownian motion parameters.
#[derive(Clone, Copy, Debug)]
pub struct FbmParams {
    pub octaves: u32,
    pub lacunarity: f64,
    pub gain: f64,
    /// Base frequency in 1/meters. Multiply world coords by this before
    /// sampling; higher octaves multiply it by `lacunarity^i`.
    pub frequency: f64,
}

/// Normalized fBm: `(Σ g^i · n(f^i · p)) / (Σ g^i)` over `octaves`.
/// Output stays in approximately [-1, 1] regardless of octave count.
pub fn fbm(p: &Perlin, x: f64, z: f64, params: &FbmParams) -> f64 {
    let mut amp = 1.0;
    let mut freq = params.frequency;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for _ in 0..params.octaves {
        sum += amp * p.noise(x * freq, z * freq);
        norm += amp;
        amp *= params.gain;
        freq *= params.lacunarity;
    }
    if norm == 0.0 { 0.0 } else { sum / norm }
}

/// Ridged transform of a noise sample: `(1 - |n|)^2`, in [0, 1].
pub fn ridged(n: f64) -> f64 {
    let r = 1.0 - n.abs();
    r * r
}

/// fBm over the per-octave ridged transform, normalized by Σ g^i.
/// Output in [0, 1] with sharp crests.
pub fn ridged_fbm(p: &Perlin, x: f64, z: f64, params: &FbmParams) -> f64 {
    let mut amp = 1.0;
    let mut freq = params.frequency;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for _ in 0..params.octaves {
        sum += amp * ridged(p.noise(x * freq, z * freq));
        norm += amp;
        amp *= params.gain;
        freq *= params.lacunarity;
    }
    if norm == 0.0 { 0.0 } else { sum / norm }
}
