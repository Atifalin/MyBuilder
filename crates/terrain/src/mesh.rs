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
    pub indices: Vec<u32>,
    /// World coords of the chunk's min corner.
    pub origin: [f64; 3],
    pub vertex_count: usize,
    pub triangle_count: usize,
}

const SAND: [f32; 3] = [0.87, 0.79, 0.56];
const GRASS: [f32; 3] = [0.35, 0.55, 0.28];
const ROCK: [f32; 3] = [0.45, 0.43, 0.40];
const SNOW: [f32; 3] = [0.92, 0.93, 0.95];

/// Full-res per-node normals from central differences on the height
/// field, edges clamped. Computed on the WHOLE grid so vertices on chunk
/// borders get identical normals from both neighboring chunks.
/// (sqrt is IEEE-754 correctly rounded — identical on every platform.)
pub fn normals(grid: &Grid, hf: &HeightField) -> Vec<[f32; 3]> {
    let mut out = vec![[0.0; 3]; grid.len()];
    let h = &hf.h;
    let d2 = 2.0 * grid.cell;
    for j in 0..grid.nz {
        for i in 0..grid.nx {
            let idx = j * grid.nx + i;
            let hl = h[j * grid.nx + i.saturating_sub(1)];
            let hr = h[j * grid.nx + (i + 1).min(grid.nx - 1)];
            let hd = h[j.saturating_sub(1) * grid.nx + i];
            let hu = h[(j + 1).min(grid.nz - 1) * grid.nx + i];
            let ddx = (hr - hl) / d2;
            let ddz = (hu - hd) / d2;
            let (nx, ny, nz) = (-ddx, 1.0, -ddz);
            let inv = 1.0 / (nx * nx + ny * ny + nz * nz).sqrt();
            out[idx] = [(nx * inv) as f32, (ny * inv) as f32, (nz * inv) as f32];
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
        origin: [ox, 0.0, oz],
        vertex_count: vside * vside,
        triangle_count: per * per * 2,
    };
    for vj in 0..vside {
        for vi in 0..vside {
            let gi = i0 + vi * step;
            let gj = j0 + vj * step;
            let idx = gj * grid.nx + gi;
            let (x, z) = (grid.x(gi), grid.z(gj));
            let h = hf.h[idx];
            m.positions
                .push([(x - ox) as f32, h as f32, (z - oz) as f32]);
            m.normals.push(normals_full[idx]);
            m.uvs.push([x as f32, z as f32]);
            m.colors.push(vertex_color(
                h,
                coast[idx],
                normals_full[idx][1],
                sea_level_m,
                snow_line_m,
            ));
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
