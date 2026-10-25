//! Per-chunk sliced region build (S10). Reproduces, for one chunk, the
//! exact values the whole-region path computes: every quantity is a
//! pure function of position plus local neighbours, so windowing the
//! coarse inputs is lossless. The window (see slice.rs margin) covers
//! every bilinear cell, barycentric coarse-mesh vertex and its ±1
//! normal neighbour the fine grid can reach.

use mb_pipeline::Artifact;
use mb_schema::{DetailRegion, WorldRecipe};

use crate::mesh::{node_normal, vertex_color};
use crate::slice::RegionSlice;
use crate::{Distances, Grid, HeightCtx, smoothstep};

/// Build the fine mesh for region chunk (cx, cz) from the decoded
/// window slice. Bit-identical to the corresponding section of the
/// whole-region build.
#[allow(clippy::too_many_arguments)]
pub fn run_region_chunk(
    grid: &Grid,
    rs: &RegionSlice,
    world: &WorldRecipe,
    region: &DetailRegion,
    seed: u64,
    cx: usize,
    cz: usize,
    poll: &dyn Fn() -> bool,
    generator_version: u32,
) -> Artifact {
    let cells_per_chunk = (world.chunk_size_m / grid.cell) as usize;
    let div = region.cell_divisor as usize;
    let step = region.mesh_step_cells as usize; // fine cells
    let coarse_step = world.export.mesh_step_cells as usize;
    let (wnx, wnz) = (rs.wnx, rs.wnz);
    let (wi0, wj0) = (rs.wi0, rs.wj0);
    // Windowed height accessor by WORLD node index.
    let h_at = |i: usize, j: usize| {
        let ii = i.clamp(wi0, wi0 + wnx - 1);
        let jj = j.clamp(wj0, wj0 + wnz - 1);
        rs.heights[(jj - wj0) * wnx + (ii - wi0)]
    };
    // Coarse mesh-vertex normal at world node (i, j): node_normal on the
    // window heights, same arithmetic as the full-grid normals.
    let n_at = |i: usize, j: usize| node_normal(&h_at, i, j, grid.nx - 1, grid.nz - 1, grid.cell);
    // HeightCtx over the virtual window grid: height_at/coast_at reuse
    // `bilinear` verbatim, so values are identical to the global ctx.
    let vg = rs.virtual_grid(grid);
    let dist = Distances {
        coast: rs.coast.clone(),
        mountain: rs.mountain.clone(),
    };
    let terrain = world.terrain.feel.resolve(&world.terrain.overrides);
    let ctx = HeightCtx::new(vg, &dist, world.sea_level_m, seed, terrain, world.profile);
    // Region boundary rect in world coords (params, not data).
    let rx0 = grid.x(region.chunk_min[0] as usize * cells_per_chunk);
    let rz0 = grid.z(region.chunk_min[1] as usize * cells_per_chunk);
    let rx1 = grid.x((region.chunk_max[0] as usize + 1) * cells_per_chunk);
    let rz1 = grid.z((region.chunk_max[1] as usize + 1) * cells_per_chunk);
    let boundary = |x: f64, z: f64| (x - rx0).min(rx1 - x).min(z - rz0).min(rz1 - z).max(0.0);
    let base_oct = ctx.fbm_params.octaves;
    let norm_n = mb_geom::fbm_norm(&ctx.fbm_params, base_oct);
    let t = &ctx.terrain;

    // Local fine grid: the chunk's fine node range + 1 fine-cell apron.
    let fsp = grid.cell / div as f64;
    let nx_f = cells_per_chunk * div + 3;
    let ox = grid.x(cx * cells_per_chunk);
    let oz = grid.z(cz * cells_per_chunk);
    let fx0 = ox - fsp;
    let fz0 = oz - fsp;
    let fx = |i: usize| fx0 + i as f64 * fsp;
    let fz = |j: usize| fz0 + j as f64 * fsp;

    let mut h = vec![0.0; nx_f * nx_f];
    let mut w = vec![0.0; nx_f * nx_f];
    for j in 0..nx_f {
        if poll() {
            return Artifact::new(Vec::new());
        }
        for i in 0..nx_f {
            let idx = j * nx_f + i;
            let (x, z) = (fx(i), fz(j));
            let wt = smoothstep(0.0, region.blend_band_m, boundary(x, z));
            let hc = crate::region::coarse_mesh_height_at(&h_at, grid, coarse_step, x, z);
            let hfinal = if wt <= 0.0 {
                hc
            } else {
                // placeholder; computed properly below
                // Bilinear inside the VIRTUAL grid uses local coords: the
                // HeightCtx already holds the virtual grid, so pass the
                // world point verbatim — window-local mapping is inside.
                let _d = ctx.coast_at(
                    x - grid.origin_x + vg.origin_x - (grid.x(wi0) - vg.origin_x),
                    z,
                );
                // Correct world-space call: virtual grid origin equals
                // world x(wi0), so world x maps to window index i - wi0.
                // height_at/coast_at expect LOCAL (window-grid) coords.
                let lx = vg.origin_x + (x - grid.x(wi0));
                let lz = vg.origin_z + (z - grid.z(wj0));
                let d = ctx.coast_at(lx, lz);
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
                let htrue = ctx.height_at(lx, lz) + detail;
                // gen >= 3: add the carve delta so interiors keep the
                // carved channels (delta is 0 away from rivers).
                let dc = if rs.delta.is_empty() {
                    0.0
                } else {
                    crate::bilinear(&vg, &rs.delta, lx, lz)
                };
                hc + wt * (htrue + dc - hc)
            };
            h[idx] = hfinal;
            w[idx] = wt;
        }
    }
    // Normals: n_true by central diff on the local fine grid, n_coarse
    // bilinear over coarse mesh-vertex normals (windowed), blended by w
    // with the same normalize-before-blend arithmetic as region_normals.
    let mut norms = vec![[0.0f32; 3]; nx_f * nx_f];
    let d2 = 2.0 * fsp;
    for j in 0..nx_f {
        if poll() {
            return Artifact::new(Vec::new());
        }
        for i in 0..nx_f {
            let idx = j * nx_f + i;
            let (x, z) = (fx(i), fz(j));
            let wt = w[idx];
            let nc = crate::region::coarse_mesh_normal_at(&n_at, grid, coarse_step, x, z);
            if wt <= 0.0 {
                norms[idx] = [nc[0] as f32, nc[1] as f32, nc[2] as f32];
                continue;
            }
            let hl = h[j * nx_f + i.saturating_sub(1)];
            let hr = h[j * nx_f + (i + 1).min(nx_f - 1)];
            let hd = h[j.saturating_sub(1) * nx_f + i];
            let hu = h[(j + 1).min(nx_f - 1) * nx_f + i];
            let (mut nx, mut ny, mut nz) = (-(hr - hl) / d2, 1.0, -(hu - hd) / d2);
            let inv = 1.0 / (nx * nx + ny * ny + nz * nz).sqrt();
            nx *= inv;
            ny *= inv;
            nz *= inv;
            let mut v = [
                nc[0] + wt * (nx - nc[0]),
                nc[1] + wt * (ny - nc[1]),
                nc[2] + wt * (nz - nc[2]),
            ];
            let inv = 1.0 / (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            v = [v[0] * inv, v[1] * inv, v[2] * inv];
            norms[idx] = [v[0] as f32, v[1] as f32, v[2] as f32];
        }
    }
    // Mesh extraction at the region's fine stride — same vertex layout
    // as region_chunk_mesh.
    let cells_fine = cells_per_chunk * div;
    let per = cells_fine / step;
    let vside = per + 1;
    let mut m = crate::mesh::ChunkMesh {
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
    for vj in 0..vside {
        for vi in 0..vside {
            let fi = 1 + vi * step;
            let fj = 1 + vj * step;
            let idx = fj * nx_f + fi;
            let (x, z) = (fx(fi), fz(fj));
            let hh = h[idx];
            m.positions
                .push([(x - ox) as f32, hh as f32, (z - oz) as f32]);
            m.normals.push(norms[idx]);
            m.uvs.push([x as f32, z as f32]);
            let lx = vg.origin_x + (x - grid.x(wi0));
            let lz = vg.origin_z + (z - grid.z(wj0));
            let d = if !rs.water_dist.is_empty() {
                ctx.coast_at(lx, lz)
                    .min(crate::bilinear(&vg, &rs.water_dist, lx, lz))
            } else {
                ctx.coast_at(lx, lz)
            };
            if generator_version >= 2 {
                // gen 2: weights at fine res; slope via central
                // differences on the fine height grid.
                let hi2 = |ii: usize, jj: usize| h[jj.min(nx_f - 1) * nx_f + ii.min(nx_f - 1)];
                let d2 = 2.0 * fsp;
                let ddx = (hi2(fi + 1, fj) - hi2(fi.saturating_sub(1), fj)) / d2;
                let ddz = (hi2(fi, fj + 1) - hi2(fi, fj.saturating_sub(1))) / d2;
                let gmag = (ddx * ddx + ddz * ddz).sqrt();
                let w = crate::splat_weights(
                    hh,
                    gmag,
                    d,
                    f64::MAX,
                    world.sea_level_m,
                    world.profile.snow_line_m,
                    &world.texturing,
                );
                let pal = [
                    crate::mesh::GRASS,
                    crate::mesh::SAND,
                    crate::mesh::ROCK,
                    crate::mesh::SNOW,
                ];
                let mut c = [0.0f32; 3];
                for k in 0..4 {
                    c[0] += (w[k] * pal[k][0] as f64) as f32;
                    c[1] += (w[k] * pal[k][1] as f64) as f32;
                    c[2] += (w[k] * pal[k][2] as f64) as f32;
                }
                m.colors.push(c);
                m.uv2.push([
                    ((vi * step) as f64 + 0.5) as f32 / (cells_fine + 1) as f32,
                    ((vj * step) as f64 + 0.5) as f32 / (cells_fine + 1) as f32,
                ]);
            } else {
                m.colors.push(vertex_color(
                    hh,
                    d,
                    norms[idx][1],
                    world.sea_level_m,
                    world.profile.snow_line_m,
                ));
            }
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
    let key = format!("{cx}_{cz}");
    let id = mb_seed::EntityId::from_path(
        seed,
        &[("world", "main"), ("region", &region.id), ("chunk", &key)],
    );
    let glb = crate::glb::chunk_glb(&m, &format!("chunk_{cx}_{cz}"), &id.to_string(), cx, cz);
    let mut out = Vec::with_capacity(crate::stages::CHUNK_HEADER + glb.len());
    out.extend_from_slice(&(m.vertex_count as u32).to_le_bytes());
    out.extend_from_slice(&(m.triangle_count as u32).to_le_bytes());
    out.extend_from_slice(&glb);
    if generator_version >= 2 {
        // Splat tail: u32 len + RGBA8 PNG at fine node res
        // (cells_fine+1 texels, fine nodes 1..=cells_fine+1 in h).
        let n_tex = cells_fine + 1;
        let mut rgba = Vec::with_capacity(n_tex * n_tex * 4);
        for fj2 in 0..n_tex {
            for fi2 in 0..n_tex {
                let fi = fi2 + 1;
                let fj = fj2 + 1;
                let idx = fj * nx_f + fi;
                let (x, z) = (fx(fi), fz(fj));
                let hi2 = |ii: usize, jj: usize| h[jj.min(nx_f - 1) * nx_f + ii.min(nx_f - 1)];
                let d2 = 2.0 * fsp;
                let ddx = (hi2(fi + 1, fj) - hi2(fi.saturating_sub(1), fj)) / d2;
                let ddz = (hi2(fi, fj + 1) - hi2(fi, fj.saturating_sub(1))) / d2;
                let gmag = (ddx * ddx + ddz * ddz).sqrt();
                let lx = vg.origin_x + (x - grid.x(wi0));
                let lz = vg.origin_z + (z - grid.z(wj0));
                let d = if !rs.water_dist.is_empty() {
                    ctx.coast_at(lx, lz)
                        .min(crate::bilinear(&vg, &rs.water_dist, lx, lz))
                } else {
                    ctx.coast_at(lx, lz)
                };
                let w = crate::splat_weights(
                    h[idx],
                    gmag,
                    d,
                    f64::MAX,
                    world.sea_level_m,
                    world.profile.snow_line_m,
                    &world.texturing,
                );
                for x in w {
                    rgba.push((x * 255.0).round() as u8);
                }
            }
        }
        let png = crate::outputs::rgba_png(&rgba, n_tex as u32, n_tex as u32);
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&png);
    }
    Artifact::new(out)
}
