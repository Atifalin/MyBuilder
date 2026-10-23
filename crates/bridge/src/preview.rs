//! Map preview tiles for the UI. Tiling convention: zoom level z ->
//! meters_per_px = 8·2^-z (z = 0 is 8 m/px); 256-px tiles aligned to
//! the world min corner.

use std::path::Path;

pub struct Rect {
    /// World-meter bounds (min corner convention: x0 < x1, z0 < z1).
    pub x0: f64,
    pub z0: f64,
    pub x1: f64,
    pub z1: f64,
}

pub enum PreviewError {
    Missing(String),
    TooLarge(usize),
    Io,
}

impl std::fmt::Debug for PreviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreviewError::Missing(m) => write!(f, "missing {m}"),
            PreviewError::TooLarge(n) => write!(f, "tile side {n} exceeds 2048"),
            PreviewError::Io => write!(f, "io error"),
        }
    }
}

pub struct PreviewSource {
    nx: usize,
    nz: usize,
    cell: f64,
    origin_x: f64,
    origin_z: f64,
    sea: f64,
    h_min: f64,
    h_max: f64,
    /// Dequantized heights.
    h: Vec<f64>,
    #[allow(dead_code)]
    land: Vec<u8>,
}

impl PreviewSource {
    /// Loads layers/height.r16 + layers.json + land.u8 once.
    pub fn open(dir: &Path) -> Result<PreviewSource, PreviewError> {
        let meta: serde_json::Value = serde_json::from_slice(
            &std::fs::read(dir.join("layers/layers.json")).map_err(|_| PreviewError::Io)?,
        )
        .map_err(|_| PreviewError::Missing("layers.json".to_string()))?;
        let nx = meta["nx"].as_u64().unwrap_or(0) as usize;
        let nz = meta["nz"].as_u64().unwrap_or(0) as usize;
        // sea_level_m lives in manifest.json.
        let man: serde_json::Value = serde_json::from_slice(
            &std::fs::read(dir.join("exports/terrain/manifest.json")).unwrap_or_default(),
        )
        .unwrap_or_default();
        let raw = std::fs::read(dir.join("layers/height.r16")).map_err(|_| PreviewError::Io)?;
        let land = std::fs::read(dir.join("layers/land.u8")).map_err(|_| PreviewError::Io)?;
        let h_min = meta["height"]["h_min"].as_f64().unwrap_or(0.0);
        let h_max = meta["height"]["h_max"].as_f64().unwrap_or(1.0);
        let span = if h_max > h_min { h_max - h_min } else { 1.0 };
        let mut h = Vec::with_capacity(nx * nz);
        for i in 0..nx * nz {
            let q = u16::from_le_bytes([raw[2 * i], raw[2 * i + 1]]);
            h.push(h_min + (q as f64 / 65535.0) * span);
        }
        Ok(PreviewSource {
            nx,
            nz,
            cell: meta["cell_size_m"].as_f64().unwrap_or(1.0),
            origin_x: meta["origin"][0].as_f64().unwrap_or(0.0),
            origin_z: meta["origin"][1].as_f64().unwrap_or(0.0),
            sea: meta["sea_level_m"]
                .as_f64()
                .or_else(|| man["sea_level_m"].as_f64())
                .unwrap_or(0.0),
            h_min,
            h_max,
            h,
            land,
        })
    }

    fn sample(&self, x: f64, z: f64) -> f64 {
        // Bilinear sample of the dequantized grid at world (x, z).
        let fi = ((x - self.origin_x) / self.cell).max(0.0);
        let fj = ((z - self.origin_z) / self.cell).max(0.0);
        let i0 = fi.floor() as usize;
        let j0 = fj.floor() as usize;
        let i1 = (i0 + 1).min(self.nx - 1);
        let j1 = (j0 + 1).min(self.nz - 1);
        let i0 = i0.min(self.nx - 1);
        let j0 = j0.min(self.nz - 1);
        let u = (fi - i0 as f64).clamp(0.0, 1.0);
        let v = (fj - j0 as f64).clamp(0.0, 1.0);
        let a = self.h[j0 * self.nx + i0];
        let b = self.h[j0 * self.nx + i1];
        let c = self.h[j1 * self.nx + i0];
        let d = self.h[j1 * self.nx + i1];
        a + u * (b - a) + v * (c - a) + u * v * (a - b - c + d)
    }

    /// RGBA PNG of `rect` at `mpp` meters/px, or Err(TooLarge) over
    /// 2048 px per side. Same light/colors as hillshade.png.
    pub fn tile(&self, rect: &Rect, mpp: f64) -> Result<Vec<u8>, PreviewError> {
        let w = ((rect.x1 - rect.x0) / mpp).ceil() as usize;
        let d = ((rect.z1 - rect.z0) / mpp).ceil() as usize;
        if w > 2048 || d > 2048 {
            return Err(PreviewError::TooLarge(w.max(d)));
        }
        let step = mpp.max(self.cell);
        let mut rgba = Vec::with_capacity(w * d * 4);
        let x_min = self.origin_x - self.cell / 2.0;
        let z_min = self.origin_z - self.cell / 2.0;
        let x_max = self.origin_x + (self.nx - 1) as f64 * self.cell + self.cell / 2.0;
        let z_max = self.origin_z + (self.nz - 1) as f64 * self.cell + self.cell / 2.0;
        for pj in 0..d {
            for pi in 0..w {
                let x = rect.x0 + (pi as f64 + 0.5) * mpp;
                let z = rect.z0 + (pj as f64 + 0.5) * mpp;
                if x < x_min || x > x_max || z < z_min || z > z_max {
                    rgba.extend_from_slice(&[0, 0, 0, 0]);
                    continue;
                }
                let h = self.sample(x, z);
                // Central differences at spacing max(mpp, cell).
                let dx = (self.sample(x + step, z) - self.sample(x - step, z)) / (2.0 * step);
                let dz = (self.sample(x, z + step) - self.sample(x, z - step)) / (2.0 * step);
                let inv = 1.0 / (dx * dx + dz * dz + 1.0).sqrt();
                let n = (-dx * inv, inv, -dz * inv);
                let (r, g, b) = mb_terrain::outputs::terrain_color(
                    h,
                    (n.0 * 0.5 + n.1 * std::f64::consts::FRAC_1_SQRT_2 + n.2 * 0.5).clamp(0.0, 1.0),
                    self.h_min,
                    self.h_max,
                    self.sea,
                );
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w as u32, d as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().map_err(|_| PreviewError::Io)?;
        wr.write_image_data(&rgba).map_err(|_| PreviewError::Io)?;
        drop(wr);
        Ok(out)
    }
}
