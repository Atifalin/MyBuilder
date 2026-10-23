//! S4: higher-detail regions with seamless borders.
//!
//! Invariant: `HeightCtx::height_at` defines terrain at any world point.
//! A region samples it on a finer node grid and blends onto the coarse
//! exported mesh surface over `blend_band_m`:
//!   h_final = h_coarse_mesh + w * (h_true - h_coarse_mesh)
//! with h_true = height_at + detail (extra fBm octaves) and
//! w = smoothstep(0, blend_band_m, distance to the region boundary).
//! On the boundary w = 0, so border vertices lie exactly on the coarse
//! mesh surface and use the interpolated coarse normals bit-for-bit.

use mb_schema::DetailRegion;

use crate::mesh::vertex_color;
use crate::{Grid, HeightCtx, HeightField, smoothstep};

/// The fine node grid of a region, INCLUDING a 1-fine-cell apron so
/// border normals can use central differences.
pub struct FineGrid {
    /// Node counts per axis (region + apron).
    pub nx: usize,
    pub nz: usize,
    /// Fine spacing in meters (cell / divisor).
    pub spacing: f64,
    /// World x/z of node (0,0), i.e. region min corner minus apron cell.
    pub origin_x: f64,
    pub origin_z: f64,
    /// Region min/max corner in world meters (boundary, not apron).
    pub rx0: f64,
    pub rz0: f64,
    pub rx1: f64,
    pub rz1: f64,
}

impl FineGrid {
    /// `i0`,`j0` = coarse node index of the region's min corner.
    pub fn new(grid: &Grid, region: &DetailRegion, cells_per_chunk: usize) -> Self {
        let div = region.cell_divisor as usize;
        let spacing = grid.cell / div as f64;
        let i0 = region.chunk_min[0] as usize * cells_per_chunk;
        let j0 = region.chunk_min[1] as usize * cells_per_chunk;
        let i1 = (region.chunk_max[0] as usize + 1) * cells_per_chunk;
        let j1 = (region.chunk_max[1] as usize + 1) * cells_per_chunk;
        // Shared node formula: origin + i*spacing everywhere.
        let rx0 = grid.x(i0);
        let rz0 = grid.z(j0);
        let rx1 = grid.x(i1);
        let rz1 = grid.z(j1);
        let nx = (i1 - i0) * div + 3; // +1 node span, +2 apron
        let nz = (j1 - j0) * div + 3;
        FineGrid {
            nx,
            nz,
            spacing,
            origin_x: rx0 - spacing,
            origin_z: rz0 - spacing,
            rx0,
            rz0,
            rx1,
            rz1,
        }
    }

    /// World x of fine node i (apron included).
    pub fn x(&self, i: usize) -> f64 {
        self.origin_x + i as f64 * self.spacing
    }

    pub fn z(&self, j: usize) -> f64 {
        self.origin_z + j as f64 * self.spacing
    }

    /// Distance from (x, z) to the region's outer boundary; 0 outside.
    pub fn boundary_dist(&self, x: f64, z: f64) -> f64 {
        (x - self.rx0)
            .min(self.rx1 - x)
            .min(z - self.rz0)
            .min(self.rz1 - z)
    }
}

/// Height of the coarse exported mesh surface at world point (x, z):
/// barycentric interpolation inside the coarse mesh triangle containing
/// p. The coarse mesh triangulates quad (a,b,c,d) as (a,c,b) + (b,c,d),
/// so the diagonal runs b--c; that convention is reproduced exactly here.
/// `mesh_step` is the coarse mesh stride in cells.
pub fn coarse_mesh_height(grid: &Grid, hf: &HeightField, mesh_step: usize, x: f64, z: f64) -> f64 {
    let sm = grid.cell * mesh_step as f64;
    // Quad index in coarse-mesh space; clamp to the last valid quad.
    let fx = ((x - grid.origin_x) / sm).clamp(0.0, (grid.nx - 1) as f64 / mesh_step as f64 - 1e-9);
    let fz = ((z - grid.origin_z) / sm).clamp(0.0, (grid.nz - 1) as f64 / mesh_step as f64 - 1e-9);
    let qx = fx.floor() as usize;
    let qz = fz.floor() as usize;
    let u = fx - qx as f64;
    let v = fz - qz as f64;
    let (i, j) = (qx * mesh_step, qz * mesh_step);
    let n = grid.nx;
    let ha = hf.h[j * n + i];
    let hb = hf.h[j * n + i + mesh_step];
    let hc = hf.h[(j + mesh_step) * n + i];
    let hd = hf.h[(j + mesh_step) * n + i + mesh_step];
    if u + v <= 1.0 {
        // Triangle (a, c, b): h = ha + u*(hb - ha) + v*(hc - ha)
        ha + u * (hb - ha) + v * (hc - ha)
    } else {
        // Triangle (b, c, d): h = hd + (1-u)*(hc - hd) + (1-v)*(hb - hd)
        hd + (1.0 - u) * (hc - hd) + (1.0 - v) * (hb - hd)
    }
}

/// Bilinear interpolation over the coarse MESH vertex lattice of the
/// exported (f32) normals. Computed in f64, matching the shared formula.
pub fn coarse_mesh_normal(
    grid: &Grid,
    normals_full: &[[f32; 3]],
    mesh_step: usize,
    x: f64,
    z: f64,
) -> [f64; 3] {
    let sm = grid.cell * mesh_step as f64;
    // Valid quads are 0..=span-1; clamp like coarse_mesh_height so the
    // last quad is used rather than extrapolated past span-2.
    let span = (grid.nx - 1) as f64 / mesh_step as f64;
    let fx = ((x - grid.origin_x) / sm).clamp(0.0, span - 1e-9);
    let fz = ((z - grid.origin_z) / sm).clamp(0.0, span - 1e-9);
    let last = (span as usize).saturating_sub(1);
    let qx = (fx.floor() as usize).min(last);
    let qz = (fz.floor() as usize).min(last);
    let u = fx - qx as f64;
    let v = fz - qz as f64;
    let n = grid.nx;
    let get = |i: usize, j: usize| normals_full[j * n + i];
    let a = get(qx * mesh_step, qz * mesh_step);
    let b = get((qx + 1) * mesh_step, qz * mesh_step);
    let c = get(qx * mesh_step, (qz + 1) * mesh_step);
    let d = get((qx + 1) * mesh_step, (qz + 1) * mesh_step);
    let mut out = [0.0f64; 3];
    for k in 0..3 {
        let (a, b, c, d) = (a[k] as f64, b[k] as f64, c[k] as f64, d[k] as f64);
        let top = a + u * (b - a);
        let bot = c + u * (d - c);
        out[k] = top + v * (bot - top);
    }
    out
}

/// Everything a region build needs.
pub struct RegionBuild {
    /// h_final per fine node (apron included).
    pub h: Vec<f64>,
    /// Blend weight per fine node.
    pub w: Vec<f64>,
    pub fine: FineGrid,
}

/// Evaluate h_final on the whole fine grid (with apron). On apron cells
/// and the boundary w = 0 so h_final = h_coarse_mesh there.
pub fn build_region_height(
    ctx: &HeightCtx,
    hf: &HeightField,
    region: &DetailRegion,
    cells_per_chunk: usize,
    coarse_mesh_step: usize,
) -> RegionBuild {
    let fine = FineGrid::new(&ctx.grid, region, cells_per_chunk);
    let base_oct = ctx.fbm_params.octaves;
    let norm_n = mb_geom::fbm_norm(&ctx.fbm_params, base_oct);
    let t = &ctx.terrain;
    let mut h = vec![0.0; fine.nx * fine.nz];
    let mut w = vec![0.0; fine.nx * fine.nz];
    for j in 0..fine.nz {
        for i in 0..fine.nx {
            let idx = j * fine.nx + i;
            let (x, z) = (fine.x(i), fine.z(j));
            let wt = smoothstep(0.0, region.blend_band_m, fine.boundary_dist(x, z).max(0.0));
            let hc = coarse_mesh_height(&ctx.grid, hf, coarse_mesh_step, x, z);
            let hfinal = if wt <= 0.0 {
                hc
            } else {
                // Detail continues the base fBm series past the base
                // octaves: fbm octaves only (no ridged term).
                let d = ctx.coast_at(x, z);
                let detail = t.relief_m
                    * ctx.amplitude(d)
                    * mb_geom::fbm_octave_sum(
                        &ctx.perlin_f,
                        x,
                        z,
                        &ctx.fbm_params,
                        base_oct,
                        base_oct + region.extra_octaves,
                    )
                    / norm_n;
                let htrue = ctx.height_at(x, z) + detail;
                hc + wt * (htrue - hc)
            };
            h[idx] = hfinal;
            w[idx] = wt;
        }
    }
    RegionBuild { h, w, fine }
}

/// Fine normals: central differences of h_final on the fine grid (the
/// apron supplies neighbors past the border), then blended toward the
/// interpolated coarse normal by w. Where w == 0 the coarse normal is
/// used verbatim so shared vertices stay bit-equal.
pub fn region_normals(
    ctx: &HeightCtx,
    normals_full: &[[f32; 3]],
    rb: &RegionBuild,
    coarse_mesh_step: usize,
) -> Vec<[f32; 3]> {
    let g = &rb.fine;
    let mut out = vec![[0.0f32; 3]; g.nx * g.nz];
    let d2 = 2.0 * g.spacing;
    for j in 0..g.nz {
        for i in 0..g.nx {
            let idx = j * g.nx + i;
            let (x, z) = (g.x(i), g.z(j));
            let wt = rb.w[idx];
            let nc = coarse_mesh_normal(&ctx.grid, normals_full, coarse_mesh_step, x, z);
            if wt <= 0.0 {
                out[idx] = [nc[0] as f32, nc[1] as f32, nc[2] as f32];
                continue;
            }
            let hl = rb.h[j * g.nx + i.saturating_sub(1)];
            let hr = rb.h[j * g.nx + (i + 1).min(g.nx - 1)];
            let hd = rb.h[j.saturating_sub(1) * g.nx + i];
            let hu = rb.h[(j + 1).min(g.nz - 1) * g.nx + i];
            let (mut nx, mut ny, mut nz) = (-(hr - hl) / d2, 1.0, -(hu - hd) / d2);
            // Normalize n_true BEFORE blending: blending an unnormalized
            // vector (|v| > 1 on slopes) with the unit n_coarse skews w.
            let inv = 1.0 / (nx * nx + ny * ny + nz * nz).sqrt();
            nx *= inv;
            ny *= inv;
            nz *= inv;
            // Blend in f64, then normalize (sqrt is correctly rounded).
            let mut v = [
                nc[0] + wt * (nx - nc[0]),
                nc[1] + wt * (ny - nc[1]),
                nc[2] + wt * (nz - nc[2]),
            ];
            let inv = 1.0 / (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            v = [v[0] * inv, v[1] * inv, v[2] * inv];
            out[idx] = [v[0] as f32, v[1] as f32, v[2] as f32];
        }
    }
    out
}

/// Extract the fine mesh for coarse chunk (cx, cz) inside `region`.
/// Positions are chunk-local f32 via the same `(world - origin) as f32`
/// formula as the coarse chunks.
#[allow(clippy::too_many_arguments)]
pub fn region_chunk_mesh(
    ctx: &HeightCtx,
    region: &DetailRegion,
    cells_per_chunk: usize,
    rb: &RegionBuild,
    norms: &[[f32; 3]],
    sea_level_m: f64,
    snow_line_m: f64,
    cx: usize,
    cz: usize,
) -> crate::mesh::ChunkMesh {
    let g = &rb.fine;
    let div = region.cell_divisor as usize;
    let step = region.mesh_step_cells as usize; // in fine cells
    // Chunk covers coarse-node range [cx*cpc, (cx+1)*cpc] -> fine range
    // [cx*cpc*div, (cx+1)*cpc*div] in region-local fine units; +1 for the
    // apron offset in FineGrid indexing.
    let cells_fine = cells_per_chunk * div;
    let per = cells_fine / step;
    let vside = per + 1;
    let i0 = cx * cells_per_chunk;
    let j0 = cz * cells_per_chunk;
    // Fine node index of the chunk's min corner (apron at index 0).
    let fi0 = (i0 - region.chunk_min[0] as usize * cells_per_chunk) * div + 1;
    let fj0 = (j0 - region.chunk_min[1] as usize * cells_per_chunk) * div + 1;
    let ox = ctx.grid.x(i0);
    let oz = ctx.grid.z(j0);

    let mut m = crate::mesh::ChunkMesh {
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
            let fi = fi0 + vi * step;
            let fj = fj0 + vj * step;
            let idx = fj * g.nx + fi;
            let (x, z) = (g.x(fi), g.z(fj));
            let h = rb.h[idx];
            m.positions
                .push([(x - ox) as f32, h as f32, (z - oz) as f32]);
            m.normals.push(norms[idx]);
            m.uvs.push([x as f32, z as f32]);
            let d = ctx.coast_at(x, z);
            m.colors
                .push(vertex_color(h, d, norms[idx][1], sea_level_m, snow_line_m));
        }
    }
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
