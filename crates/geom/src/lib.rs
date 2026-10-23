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

// 8 unit directions rotated 22.5 degrees off the axes so no gradient
// lies on a lattice axis (axis-aligned gradients produce visible square
// artifacts). Constants are literals: no trig at runtime.
const COS22_5: f64 = 0.9238795325112867;
const SIN22_5: f64 = 0.3826834323650898;
const GRAD: [(f64, f64); 8] = [
    (COS22_5, SIN22_5),
    (-COS22_5, SIN22_5),
    (COS22_5, -SIN22_5),
    (-COS22_5, -SIN22_5),
    (SIN22_5, COS22_5),
    (-SIN22_5, COS22_5),
    (SIN22_5, -COS22_5),
    (-SIN22_5, -COS22_5),
];

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
    sum_octaves(p, x, z, params, false)
}

/// fBm over the per-octave ridged transform, normalized by Σ g^i.
/// Output in [0, 1] with sharp crests.
pub fn ridged_fbm(p: &Perlin, x: f64, z: f64, params: &FbmParams) -> f64 {
    sum_octaves(p, x, z, params, true)
}

/// Shared octave loop. To break up lattice alignment, each successive
/// octave samples a point rotated once more by the rational rotation
/// R = [[0.6, -0.8], [0.8, 0.6]] (a Pythagorean rotation, exact) and
/// offset by `i * (37.719, -91.137)` lattice units after scaling. All
/// constants are literals; no trig, so output stays deterministic.
fn sum_octaves(p: &Perlin, x: f64, z: f64, params: &FbmParams, ridged_mode: bool) -> f64 {
    let (sum, norm) = octave_loop(p, x, z, params, 0, params.octaves, ridged_mode);
    if norm == 0.0 { 0.0 } else { sum / norm }
}

/// Unnormalized octave sum `Σ_{i=start}^{end-1} g^i · n_i(p)` using the
/// SAME rotation/offset scheme as [`fbm`], so octave i here is identical
/// to octave i there. Used to extend the series with extra octaves in
/// high-detail regions.
pub fn fbm_octave_sum(p: &Perlin, x: f64, z: f64, params: &FbmParams, start: u32, end: u32) -> f64 {
    octave_loop(p, x, z, params, start, end, false).0
}

/// Normalization Σ_{i=0}^{count-1} g^i of the base octave series.
pub fn fbm_norm(params: &FbmParams, count: u32) -> f64 {
    let mut norm = 0.0;
    let mut amp = 1.0;
    for _ in 0..count {
        norm += amp;
        amp *= params.gain;
    }
    norm
}

/// Shared loop over octaves start..end. Returns (sum, norm) where norm =
/// Σ gain^i over the SAME range. Rotation is cumulative from octave 0, so
/// starting mid-series still lands each octave on its canonical lattice.
fn octave_loop(
    p: &Perlin,
    x: f64,
    z: f64,
    params: &FbmParams,
    start: u32,
    end: u32,
    ridged_mode: bool,
) -> (f64, f64) {
    let mut amp = 1.0;
    let mut freq = params.frequency;
    let (mut rx, mut rz) = (x, z);
    for _ in 0..start {
        amp *= params.gain;
        freq *= params.lacunarity;
        let next_rx = 0.6 * rx - 0.8 * rz;
        rz = 0.8 * rx + 0.6 * rz;
        rx = next_rx;
    }
    let mut sum = 0.0;
    let mut norm = 0.0;
    for i in start..end {
        let n = p.noise(rx * freq + i as f64 * 37.719, rz * freq - i as f64 * 91.137);
        sum += amp * if ridged_mode { ridged(n) } else { n };
        norm += amp;
        amp *= params.gain;
        freq *= params.lacunarity;
        let next_rx = 0.6 * rx - 0.8 * rz;
        rz = 0.8 * rx + 0.6 * rz;
        rx = next_rx;
    }
    (sum, norm)
}

/// Ridged transform of a noise sample: `(1 - |n|)^2`, in [0, 1].
pub fn ridged(n: f64) -> f64 {
    let r = 1.0 - n.abs();
    r * r
}
