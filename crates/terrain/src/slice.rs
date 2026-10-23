//! Sliced chunk stages (S10). A slice is the minimal self-describing
//! input a chunk needs from the height/distance artifacts: its node
//! range plus edge aprons. Corners are deliberately omitted — normals
//! never read diagonals, so including them would wrongly couple
//! diagonal neighbours in the cache key.

use crate::{Grid, HeightField};

/// Decoded chunk slice: heights over [i0..i1]x[j0..j1] (node indices),
/// edge aprons, and the coast distance over the node range.
pub struct ChunkSlice {
    pub i0: usize,
    pub j0: usize,
    /// Nodes per side (cells_per_chunk + 1).
    pub ni: usize,
    pub nj: usize,
    /// (ni*nj) node-range heights.
    pub h: Vec<f64>,
    /// Edge aprons: column i0-1 / i1+1 (len nj) and row j0-1 / j1+1
    /// (len ni). Absent at the world edge.
    pub west: Option<Vec<f64>>,
    pub east: Option<Vec<f64>>,
    pub south: Option<Vec<f64>>,
    pub north: Option<Vec<f64>>,
    /// (ni*nj) coast distances over the node range.
    pub coast: Vec<f64>,
}

impl ChunkSlice {
    /// Height at world node (i, j). Indices are clamped to the covered
    /// range, which coincides with world-edge clamping: at a world edge
    /// the apron is absent and the clamp lands on the edge node, exactly
    /// like the full-grid formula.
    pub fn h_at(&self, i: usize, j: usize) -> f64 {
        let i1 = self.i0 + self.ni - 1;
        let j1 = self.j0 + self.nj - 1;
        if i >= self.i0 && i <= i1 && j >= self.j0 && j <= j1 {
            return self.h[(j - self.j0) * self.ni + (i - self.i0)];
        }
        if j >= self.j0 && j <= j1 {
            if i == self.i0.saturating_sub(1)
                && i < self.i0
                && let Some(w) = &self.west
            {
                return w[j - self.j0];
            }
            if i == i1 + 1
                && let Some(e) = &self.east
            {
                return e[j - self.j0];
            }
        }
        if i >= self.i0 && i <= i1 {
            if j == self.j0.saturating_sub(1)
                && j < self.j0
                && let Some(s) = &self.south
            {
                return s[i - self.i0];
            }
            if j == j1 + 1
                && let Some(n) = &self.north
            {
                return n[i - self.i0];
            }
        }
        // Clamp to the nearest covered node (world-edge behaviour).
        let ic = i.clamp(self.i0, i1);
        let jc = j.clamp(self.j0, j1);
        self.h[(jc - self.j0) * self.ni + (ic - self.i0)]
    }

    pub fn coast_at(&self, i: usize, j: usize) -> f64 {
        self.coast[(j - self.j0) * self.ni + (i - self.i0)]
    }
}

/// Build the slice bytes for chunk (cx, cz) from the decoded height
/// planes. Header: i0, j0, ni, nj (u32 LE), apron flags (u8:
/// W=1,E=2,S=4,N=8); then node-range f64s, aprons (W,E,S,N order),
/// then the coast node-range f64s.
pub fn encode_chunk_slice(
    grid: &Grid,
    hf: &HeightField,
    coast: &[f64],
    cells_per_chunk: usize,
    cx: usize,
    cz: usize,
) -> Vec<u8> {
    let i0 = cx * cells_per_chunk;
    let j0 = cz * cells_per_chunk;
    let (ni, nj) = (cells_per_chunk + 1, cells_per_chunk + 1);
    let (i1, j1) = (i0 + ni - 1, j0 + nj - 1);
    let mut flags = 0u8;
    if i0 > 0 {
        flags |= 1;
    }
    if i1 < grid.nx - 1 {
        flags |= 2;
    }
    if j0 > 0 {
        flags |= 4;
    }
    if j1 < grid.nz - 1 {
        flags |= 8;
    }
    let mut out = Vec::new();
    for v in [i0 as u32, j0 as u32, ni as u32, nj as u32] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.push(flags);
    let gat = |i: usize, j: usize| hf.h[j * grid.nx + i];
    for j in j0..=j1 {
        for i in i0..=i1 {
            out.extend_from_slice(&gat(i, j).to_le_bytes());
        }
    }
    if flags & 1 != 0 {
        for j in j0..=j1 {
            out.extend_from_slice(&gat(i0 - 1, j).to_le_bytes());
        }
    }
    if flags & 2 != 0 {
        for j in j0..=j1 {
            out.extend_from_slice(&gat(i1 + 1, j).to_le_bytes());
        }
    }
    if flags & 4 != 0 {
        for i in i0..=i1 {
            out.extend_from_slice(&gat(i, j0 - 1).to_le_bytes());
        }
    }
    if flags & 8 != 0 {
        for i in i0..=i1 {
            out.extend_from_slice(&gat(i, j1 + 1).to_le_bytes());
        }
    }
    for j in j0..=j1 {
        for i in i0..=i1 {
            out.extend_from_slice(&coast[j * grid.nx + i].to_le_bytes());
        }
    }
    out
}

pub fn decode_chunk_slice(b: &[u8]) -> ChunkSlice {
    let u32at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    let (i0, j0, ni, nj) = (u32at(0), u32at(4), u32at(8), u32at(12));
    let flags = b[16];
    let mut o = 17;
    let take = |n: usize, o: &mut usize| -> Vec<f64> {
        let v: Vec<f64> = (0..n)
            .map(|k| f64::from_le_bytes(b[*o + 8 * k..*o + 8 * k + 8].try_into().unwrap()))
            .collect();
        *o += 8 * n;
        v
    };
    let h = take(ni * nj, &mut o);
    let west = (flags & 1 != 0).then(|| take(nj, &mut o));
    let east = (flags & 2 != 0).then(|| take(nj, &mut o));
    let south = (flags & 4 != 0).then(|| take(ni, &mut o));
    let north = (flags & 8 != 0).then(|| take(ni, &mut o));
    let coast = take(ni * nj, &mut o);
    ChunkSlice {
        i0,
        j0,
        ni,
        nj,
        h,
        west,
        east,
        south,
        north,
        coast,
    }
}

/// Decoded region-chunk slice: a window of coarse heights, coast and
/// mountain distances around the chunk, sized to cover every lookup the
/// fine grid makes (bilinear cells + coarse mesh quad vertices + their
/// ±1 normal neighbours). Stored as dense window arrays.
pub struct RegionSlice {
    /// Window min node indices (world grid).
    pub wi0: usize,
    pub wj0: usize,
    pub wnx: usize,
    pub wnz: usize,
    pub heights: Vec<f64>,
    pub coast: Vec<f64>,
    pub mountain: Vec<f64>,
    /// Gen >= 3: the hydrology carve delta (final_h - h) window.
    /// Empty for earlier generators.
    pub delta: Vec<f64>,
}

impl RegionSlice {
    /// A virtual [`Grid`] covering the window: world node (wi0+i, wj0+j)
    /// maps to local index j*wnx+i. Feeding this plus the window planes
    /// to `bilinear`/`HeightCtx` reproduces the full-grid arithmetic
    /// verbatim, because those functions only touch grid coords inside
    /// the window.
    pub fn virtual_grid(&self, grid: &Grid) -> Grid {
        Grid {
            nx: self.wnx,
            nz: self.wnz,
            cell: grid.cell,
            origin_x: grid.x(self.wi0),
            origin_z: grid.z(self.wj0),
        }
    }
}

/// Coarse-node window margin around a region chunk, in coarse cells:
/// fine points up to one fine cell outside the chunk land in a coarse
/// mesh quad whose far vertex is `mesh_step` away, and that vertex's
/// normal needs one more node. mesh_step + 1 covers everything.
pub fn region_margin(mesh_step: usize) -> usize {
    mesh_step + 1
}

#[allow(clippy::too_many_arguments)]
pub fn encode_region_slice(
    grid: &Grid,
    hf: &HeightField,
    coast: &[f64],
    mountain: &[f64],
    cells_per_chunk: usize,
    mesh_step: usize,
    cx: usize,
    cz: usize,
    delta: Option<&[f64]>,
) -> Vec<u8> {
    let m = region_margin(mesh_step);
    let i0 = cx * cells_per_chunk;
    let j0 = cz * cells_per_chunk;
    let i1 = i0 + cells_per_chunk;
    let j1 = j0 + cells_per_chunk;
    let wi0 = i0.saturating_sub(m);
    let wj0 = j0.saturating_sub(m);
    let wi1 = (i1 + m).min(grid.nx - 1);
    let wj1 = (j1 + m).min(grid.nz - 1);
    let (wnx, wnz) = (wi1 - wi0 + 1, wj1 - wj0 + 1);
    let mut out = Vec::new();
    for v in [wi0 as u32, wj0 as u32, wnx as u32, wnz as u32] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let gat = |i: usize, j: usize, p: &[f64]| p[j * grid.nx + i];
    for plane in [&hf.h, &coast.to_vec(), &mountain.to_vec()] {
        for j in wj0..=wj1 {
            for i in wi0..=wi1 {
                out.extend_from_slice(&gat(i, j, plane).to_le_bytes());
            }
        }
    }
    // Trailing flag byte: delta window present or not.
    match delta {
        Some(d) => {
            out.push(1);
            for j in wj0..=wj1 {
                for i in wi0..=wi1 {
                    out.extend_from_slice(&gat(i, j, d).to_le_bytes());
                }
            }
        }
        None => out.push(0),
    }
    out
}

pub fn decode_region_slice(b: &[u8]) -> RegionSlice {
    let u32at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    let (wi0, wj0, wnx, wnz) = (u32at(0), u32at(4), u32at(8), u32at(12));
    let mut o = 16;
    let take = |o: &mut usize| -> Vec<f64> {
        let n = wnx * wnz;
        let v: Vec<f64> = (0..n)
            .map(|k| f64::from_le_bytes(b[*o + 8 * k..*o + 8 * k + 8].try_into().unwrap()))
            .collect();
        *o += 8 * n;
        v
    };
    RegionSlice {
        wi0,
        wj0,
        wnx,
        wnz,
        heights: take(&mut o),
        coast: take(&mut o),
        mountain: take(&mut o),
        delta: {
            if b[o] == 1 {
                o += 1;
                take(&mut o)
            } else {
                Vec::new()
            }
        },
    }
}
