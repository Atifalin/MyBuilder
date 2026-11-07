//! Phase 3 step 4a: procedural detail textures. Near-white greyscale albedo
//! that multiplies the existing material/vertex colour, so hue stays per
//! house. Every texture tiles exactly: noise is periodic value noise on an
//! integer lattice wrapped modulo the period, and line patterns are
//! symmetric about the tile border. Only + - * / and floor (no libm).

pub const MATERIAL_GENERATOR_VERSION: u32 = 1;
/// Texture edge in pixels.
pub const TEX: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TexSlot {
    Plaster,
    Stone,
    Roof,
    Floor,
    Wood,
}

pub const SLOTS: [TexSlot; 5] = [
    TexSlot::Plaster,
    TexSlot::Stone,
    TexSlot::Roof,
    TexSlot::Floor,
    TexSlot::Wood,
];

impl TexSlot {
    pub fn name(self) -> &'static str {
        match self {
            TexSlot::Plaster => "plaster",
            TexSlot::Stone => "stone",
            TexSlot::Roof => "roof",
            TexSlot::Floor => "floor",
            TexSlot::Wood => "wood",
        }
    }
    /// Real-world size of one texture tile in metres (UVs are metres).
    pub fn tile_m(self) -> f64 {
        match self {
            TexSlot::Plaster | TexSlot::Roof => 2.0,
            TexSlot::Stone | TexSlot::Floor => 1.2,
            TexSlot::Wood => 1.0,
        }
    }
    fn seed(self) -> u64 {
        mb_seed::derive(0x6d62_7465_7874_7572, "texture", self.name())
    }
}

/// Lattice value in [0,1) at (ix, iz) wrapped to the period.
fn lat(seed: u64, ix: i64, iz: i64, px: i64, pz: i64) -> f64 {
    let (x, z) = (ix.rem_euclid(px) as u64, iz.rem_euclid(pz) as u64);
    (mb_seed::Rng::at(seed, (x << 20) | z) >> 11) as f64 * (1.0 / 9007199254740992.0)
}

fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// Periodic value noise in [0,1). `u, v` are tile coords in [0,1); the
/// lattice has `cu x cv` cells across one tile.
fn vnoise(seed: u64, u: f64, v: f64, cu: i64, cv: i64) -> f64 {
    let (x, z) = (u * cu as f64, v * cv as f64);
    let (fx, fz) = (x.floor(), z.floor());
    let (ix, iz) = (fx as i64, fz as i64);
    let (tx, tz) = (smooth(x - fx), smooth(z - fz));
    let a = lat(seed, ix, iz, cu, cv);
    let b = lat(seed, ix + 1, iz, cu, cv);
    let c = lat(seed, ix, iz + 1, cu, cv);
    let d = lat(seed, ix + 1, iz + 1, cu, cv);
    let top = a + (b - a) * tx;
    let bot = c + (d - c) * tx;
    top + (bot - top) * tz
}

/// Periodic fbm in [0,1): octaves double the lattice (still periodic).
fn fbm(seed: u64, u: f64, v: f64, cu: i64, cv: i64, oct: u32) -> f64 {
    let (mut sum, mut amp, mut norm) = (0.0, 0.5, 0.0);
    for o in 0..oct {
        let k = 1i64 << o;
        sum += amp * vnoise(seed.wrapping_add(o as u64), u, v, cu * k, cv * k);
        norm += amp;
        amp *= 0.5;
    }
    sum / norm
}

/// Unit hash of an integer cell (already wrapped by the caller).
fn cell(seed: u64, a: i64, b: i64) -> f64 {
    lat(seed ^ 0x5bd1_e995, a, b, 1 << 20, 1 << 20)
}

/// True on the 2 px that straddle every multiple of `step` (wraps at 0).
fn on_line(p: usize, step: usize) -> bool {
    let m = p % step;
    m == 0 || m == step - 1
}

fn brightness(slot: TexSlot, x: usize, y: usize) -> f64 {
    let s = slot.seed();
    let (u, v) = ((x as f64 + 0.5) / TEX as f64, (y as f64 + 0.5) / TEX as f64);
    let speckle = |k: u64, amt: f64| (vnoise(s ^ k, u, v, 128, 128) - 0.5) * amt;
    match slot {
        TexSlot::Plaster => 0.84 + 0.16 * fbm(s, u, v, 4, 4, 4) + speckle(1, 0.06),
        TexSlot::Stone => {
            // 1.2 m tile: 4 courses of 300 mm (64 px), blocks 600 mm (128 px),
            // alternate courses offset half a block.
            let row = y / 64;
            let xo = (x + (row % 2) * 64) % TEX;
            if on_line(y, 64) || on_line(xo, 128) {
                return 0.72 + speckle(2, 0.04);
            }
            0.85 + 0.15 * cell(s, row as i64, (xo / 128) as i64)
                + (fbm(s, u, v, 8, 8, 3) - 0.5) * 0.10
                + speckle(3, 0.05)
        }
        TexSlot::Roof => {
            // Lime terrace, faint joints every 1 m (128 px).
            let joint = if on_line(x, 128) || on_line(y, 128) {
                -0.06
            } else {
                0.0
            };
            0.86 + 0.10 * fbm(s, u, v, 4, 4, 3) + speckle(4, 0.10) + joint
        }
        TexSlot::Floor => {
            // Kota stone: 600 mm tiles (128 px) with grout lines.
            if on_line(x, 128) || on_line(y, 128) {
                return 0.62;
            }
            0.84 + 0.12 * cell(s, (x / 128) as i64, (y / 128) as i64)
                + (fbm(s, u, v, 6, 6, 3) - 0.5) * 0.08
                + speckle(5, 0.03)
        }
        TexSlot::Wood => {
            // Grain runs along u: few cells along, many across.
            0.78 + 0.18 * fbm(s, u, v, 2, 32, 3) + (vnoise(s ^ 6, u, v, 1, 128) - 0.5) * 0.08
        }
    }
}

/// Raw RGBA8 pixels (alpha 255), row-major, TEX x TEX.
pub fn texture_rgba(slot: TexSlot) -> Vec<u8> {
    let mut img = vec![255u8; TEX * TEX * 4];
    for y in 0..TEX {
        for x in 0..TEX {
            let b = brightness(slot, x, y).clamp(0.0, 1.0);
            let c = (b * 255.0 + 0.5).floor() as u8;
            let i = (y * TEX + x) * 4;
            img[i..i + 3].copy_from_slice(&[c, c, c]);
        }
    }
    img
}

pub fn texture_png(slot: TexSlot) -> Vec<u8> {
    let img = texture_rgba(slot);
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, TEX as u32, TEX as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().expect("png header");
    w.write_image_data(&img).expect("png data");
    drop(w);
    out
}
