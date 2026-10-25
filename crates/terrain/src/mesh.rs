//! Per-vertex derived data and chunk mesh extraction.

use crate::{Grid, HeightField};

/// One exported chunk mesh, positions chunk-local (origin at the chunk's
/// min corner), y absolute in meters.
pub struct ChunkMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// World meters (x, z) so textures tile at real-world size.
    pub uvs: Vec<[f32; 2]>,
    /// Linear RGB, f32 VEC3.
    pub colors: Vec<[f32; 3]>,
    /// RGBA f32 VEC4 — used only by the gen-3 water mesh (depth01,
    /// kind, 0, 1); empty for terrain meshes.
    pub colors4: Vec<[f32; 4]>,
    /// Splat UVs for TEXCOORD_1 (gen 2 only; empty on gen 1).
    pub uv2: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// World coords of the chunk's min corner.
    pub origin: [f64; 3],
    pub vertex_count: usize,
    pub triangle_count: usize,
}

pub(crate) const SAND: [f32; 3] = [0.87, 0.79, 0.56];
pub(crate) const GRASS: [f32; 3] = [0.35, 0.55, 0.28];
pub(crate) const ROCK: [f32; 3] = [0.45, 0.43, 0.40];
pub(crate) const SNOW: [f32; 3] = [0.92, 0.93, 0.95];

/// Central-difference normal at one node. `h_at` supplies heights by
/// (i, j); indices are clamped to [i_lo..=i_hi]x[j_lo..=j_hi] exactly the
/// way the full-grid version clamps at world edges. This is THE normal
/// formula shared by the full grid and sliced chunk stages — same
/// arithmetic, so same bits.
pub fn node_normal(
    h_at: &dyn Fn(usize, usize) -> f64,
    i: usize,
    j: usize,
    i_hi: usize,
    j_hi: usize,
    cell: f64,
) -> [f32; 3] {
    let d2 = 2.0 * cell;
    let hl = h_at(i.saturating_sub(1), j);
    let hr = h_at((i + 1).min(i_hi), j);
    let hd = h_at(i, j.saturating_sub(1));
    let hu = h_at(i, (j + 1).min(j_hi));
    let ddx = (hr - hl) / d2;
    let ddz = (hu - hd) / d2;
    let (nx, ny, nz) = (-ddx, 1.0, -ddz);
    let inv = 1.0 / (nx * nx + ny * ny + nz * nz).sqrt();
    [(nx * inv) as f32, (ny * inv) as f32, (nz * inv) as f32]
}

/// Gradient magnitude (tan of slope) at one node from the SAME central
/// differences as [`node_normal`].
pub fn node_slope(
    h_at: &dyn Fn(usize, usize) -> f64,
    i: usize,
    j: usize,
    i_hi: usize,
    j_hi: usize,
    cell: f64,
) -> f64 {
    let d2 = 2.0 * cell;
    let hl = h_at(i.saturating_sub(1), j);
    let hr = h_at((i + 1).min(i_hi), j);
    let hd = h_at(i, j.saturating_sub(1));
    let hu = h_at(i, (j + 1).min(j_hi));
    let ddx = (hr - hl) / d2;
    let ddz = (hu - hd) / d2;
    (ddx * ddx + ddz * ddz).sqrt()
}

/// Full-res per-node normals from central differences on the height
/// field, edges clamped. Computed on the WHOLE grid so vertices on chunk
/// borders get identical normals from both neighboring chunks.
/// (sqrt is IEEE-754 correctly rounded — identical on every platform.)
pub fn normals(grid: &Grid, hf: &HeightField) -> Vec<[f32; 3]> {
    let h = &hf.h;
    let nx = grid.nx;
    let h_at = |i: usize, j: usize| h[j * nx + i];
    let mut out = vec![[0.0; 3]; grid.len()];
    for j in 0..grid.nz {
        for i in 0..grid.nx {
            out[j * grid.nx + i] = node_normal(&h_at, i, j, grid.nx - 1, grid.nz - 1, grid.cell);
        }
    }
    out
}

/// Vertex color rules. Priority: rock (steep) > snow (high) >
/// sand/grass blend (coastal sand fades over ~8 m of altitude).
pub(crate) fn vertex_color(h: f64, d: f64, ny: f32, sea: f64, snow_line_m: f64) -> [f32; 3] {
    if ny < 0.8 {
        return ROCK;
    }
    // Absolute snow line: local data only, no global h_max.
    if h > sea + snow_line_m {
        return SNOW;
    }
    let sand_w = if d < 60.0 {
        (((sea + 3.0) - h) / 8.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mix = |a: [f32; 3], b: [f32; 3], t: f64| {
        [
            (a[0] as f64 + (b[0] as f64 - a[0] as f64) * t) as f32,
            (a[1] as f64 + (b[1] as f64 - a[1] as f64) * t) as f32,
            (a[2] as f64 + (b[2] as f64 - a[2] as f64) * t) as f32,
        ]
    };
    mix(GRASS, SAND, sand_w)
}

/// Extract the mesh for chunk (cx, cz) at `step`-cell vertex stride.
/// `cells_per_chunk` must be divisible by `step`. `normals_full` must be
/// the result of [`normals`] on the whole grid (seam-free borders).
/// `dist.coast` is used for the sand rule.
#[allow(clippy::too_many_arguments)]
pub fn chunk_mesh(
    grid: &Grid,
    hf: &HeightField,
    normals_full: &[[f32; 3]],
    coast: &[f64],
    sea_level_m: f64,
    snow_line_m: f64,
    cells_per_chunk: usize,
    step: usize,
    cx: usize,
    cz: usize,
) -> ChunkMesh {
    let nx = grid.nx;
    chunk_mesh_view(
        grid,
        |i, j| hf.h[j * nx + i],
        |i, j| normals_full[j * nx + i],
        |i, j| coast[j * nx + i],
        sea_level_m,
        snow_line_m,
        cells_per_chunk,
        step,
        cx,
        cz,
        None, // gen-1 helper: no splat UVs
    )
}

/// The shared vertex loop: `h_at`, `normal_at` and `coast_at` supply
/// world-node-indexed data. The whole-grid path and the sliced chunk
/// stage both call this, so their meshes are identical by construction.
#[allow(clippy::too_many_arguments)]
pub fn chunk_mesh_view(
    grid: &Grid,
    h_at: impl Fn(usize, usize) -> f64,
    normal_at: impl Fn(usize, usize) -> [f32; 3],
    coast_at: impl Fn(usize, usize) -> f64,
    sea_level_m: f64,
    snow_line_m: f64,
    cells_per_chunk: usize,
    step: usize,
    cx: usize,
    cz: usize,
    tex2: Option<(&mb_schema::TexturingSection, usize)>,
) -> ChunkMesh {
    let per = cells_per_chunk / step; // cells per chunk side at mesh res
    let vside = per + 1;
    let i0 = cx * cells_per_chunk;
    let j0 = cz * cells_per_chunk;
    let ox = grid.x(i0);
    let oz = grid.z(j0);

    let mut m = ChunkMesh {
        positions: Vec::with_capacity(vside * vside),
        normals: Vec::with_capacity(vside * vside),
        uvs: Vec::with_capacity(vside * vside),
        colors: Vec::with_capacity(vside * vside),
        indices: Vec::with_capacity(per * per * 6),
        uv2: Vec::new(),
        colors4: Vec::new(),
        origin: [ox, 0.0, oz],
        vertex_count: vside * vside,
        triangle_count: per * per * 2,
    };
    if let Some((_, _n_texels)) = tex2 {
        m.uv2 = Vec::with_capacity(vside * vside);
    }
    for vj in 0..vside {
        for vi in 0..vside {
            let gi = i0 + vi * step;
            let gj = j0 + vj * step;
            let (x, z) = (grid.x(gi), grid.z(gj));
            let h = h_at(gi, gj);
            let n = normal_at(gi, gj);
            m.positions
                .push([(x - ox) as f32, h as f32, (z - oz) as f32]);
            m.normals.push(n);
            m.uvs.push([x as f32, z as f32]);
            if let Some((tex, n_texels)) = tex2 {
                // gen 2: weight-blended palette + texel-center splat UVs
                let g = node_slope(&h_at, gi, gj, grid.nx - 1, grid.nz - 1, grid.cell);
                let w = crate::splat_weights(
                    h,
                    g,
                    coast_at(gi, gj),
                    f64::MAX,
                    sea_level_m,
                    snow_line_m,
                    tex,
                );
                let pal = [GRASS, SAND, ROCK, SNOW];
                let mut c = [0.0f32; 3];
                for k in 0..4 {
                    c[0] += (w[k] * pal[k][0] as f64) as f32;
                    c[1] += (w[k] * pal[k][1] as f64) as f32;
                    c[2] += (w[k] * pal[k][2] as f64) as f32;
                }
                m.colors.push(c);
                m.uv2.push([
                    ((vi * step) as f64 + 0.5) as f32 / n_texels as f32,
                    ((vj * step) as f64 + 0.5) as f32 / n_texels as f32,
                ]);
            } else {
                m.colors.push(vertex_color(
                    h,
                    coast_at(gi, gj),
                    n[1],
                    sea_level_m,
                    snow_line_m,
                ));
            }
        }
    }
    // CCW winding with +Y-facing triangles: (a, c, b) and (b, c, d).
    for vj in 0..per {
        for vi in 0..per {
            let a = (vj * vside + vi) as u32;
            let b = a + 1;
            let c = a + vside as u32;
            let d = c + 1;
            m.indices.extend_from_slice(&[a, c, b]);
            m.indices.extend_from_slice(&[b, c, d]);
        }
    }
    m
}
