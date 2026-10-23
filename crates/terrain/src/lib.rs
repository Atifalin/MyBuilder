//! Terrain: rasterized masks -> signed distance -> height -> quantized
//! layers -> chunked glTF meshes.
//!
//! Grid convention: samples sit at grid NODES, `x_i = -W/2 + i*cell` for
//! `i` in `0..=W/cell`, so the default 2048 m world at 1 m cells is
//! 2049x2049 samples. Storage is row-major with z rows: index
//! `j*nx + i` for node (i, j) at world `(x_i, z_j)`. Heights are f64
//! internally and quantized to u16 only on disk.

pub mod build;
pub mod glb;
pub mod mesh;
pub mod outputs;
pub mod region;
pub mod region_slice;
pub mod slice;
pub mod stages;

/// Grid geometry shared by all stages.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    /// Node counts per axis (extent/cell + 1).
    pub nx: usize,
    pub nz: usize,
    /// Cell size in meters.
    pub cell: f64,
    /// World x/z of node (0,0) = world min corner.
    pub origin_x: f64,
    pub origin_z: f64,
}

impl Grid {
    pub fn new(extent_w: f64, extent_d: f64, cell: f64) -> Self {
        Grid {
            nx: (extent_w / cell) as usize + 1,
            nz: (extent_d / cell) as usize + 1,
            cell,
            origin_x: -extent_w / 2.0,
            origin_z: -extent_d / 2.0,
        }
    }

    pub fn len(&self) -> usize {
        self.nx * self.nz
    }

    pub fn is_empty(&self) -> bool {
        self.nx == 0 || self.nz == 0
    }

    pub fn x(&self, i: usize) -> f64 {
        self.origin_x + i as f64 * self.cell
    }

    pub fn z(&self, j: usize) -> f64 {
        self.origin_z + j as f64 * self.cell
    }
}

/// smoothstep(a, b, x) = t^2(3-2t) with t = clamp((x-a)/(b-a), 0, 1).
pub fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The three boolean layers, nx*nz bytes each.
#[derive(Clone, Debug)]
pub struct Masks {
    pub land: Vec<u8>,
    pub mountain: Vec<u8>,
    pub forest: Vec<u8>,
}

/// Rasterize all shape kinds. Land = union of land shapes (or the whole
/// extent in land_first mode when there are none, empty in water_first),
/// minus water shapes. Mountain/forest = union of their shapes.
pub fn build_masks(grid: &Grid, shapes: &[mb_schema::Shape], mode: mb_schema::WorldMode) -> Masks {
    use mb_schema::ShapeKind;
    let mut land = vec![0u8; grid.len()];
    let mut mountain = vec![0u8; grid.len()];
    let mut forest = vec![0u8; grid.len()];
    let mut water = vec![0u8; grid.len()];

    let has_land = shapes.iter().any(|s| s.kind == ShapeKind::Land);
    if !has_land && mode == mb_schema::WorldMode::LandFirst {
        land.fill(1);
    }
    for shape in shapes {
        let dst: &mut Vec<u8> = match shape.kind {
            ShapeKind::Land => &mut land,
            ShapeKind::Water => &mut water,
            ShapeKind::Mountain => &mut mountain,
            ShapeKind::Forest => &mut forest,
        };
        rasterize(grid, &shape.points, dst);
    }
    for i in 0..grid.len() {
        if water[i] == 1 {
            land[i] = 0;
        }
    }
    Masks {
        land,
        mountain,
        forest,
    }
}

/// Even-odd scanline fill. For each node row, collect edge x-crossings
/// (half-open vertex rule min(z0,z1) <= z < max(z0,z1)), sort, then fill
/// spans pairwise with x_left <= x < x_right.
fn rasterize(grid: &Grid, points: &[[f64; 2]], dst: &mut [u8]) {
    if points.len() < 3 {
        return;
    }
    let n = points.len();
    for j in 0..grid.nz {
        let z = grid.z(j);
        let mut xs = Vec::new();
        for e in 0..n {
            let [x0, z0] = points[e];
            let [x1, z1] = points[(e + 1) % n];
            let lo = z0.min(z1);
            let hi = z0.max(z1);
            if z >= lo && z < hi {
                xs.push(x0 + (z - z0) * (x1 - x0) / (z1 - z0));
            }
        }
        xs.sort_by(f64::total_cmp);
        for span in xs.as_chunks::<2>().0 {
            let (xl, xr) = (span[0], span[1]);
            // First i with x_i >= xl, last with x_i < xr.
            let i0 = ((xl - grid.origin_x) / grid.cell).ceil().max(0.0) as usize;
            let i1 = ((xr - grid.origin_x) / grid.cell).ceil().max(0.0) as usize;
            for i in i0..i1.min(grid.nx) {
                dst[j * grid.nx + i] = 1;
            }
        }
    }
}

/// Exact squared Euclidean distance transform (Felzenszwalb-Huttenlocher)
/// in one dimension. `f` is 0 on target samples, INF elsewhere; `d` and
/// `v`/`z` are scratch. Returns squared distances in sample^2 units.
fn edt_1d(f: &[f64], d: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    let intersect = |fq: f64, qf: f64, fv: f64, vf: f64| -> f64 {
        // Boundary where the parabola at q overtakes the one at v:
        // s = ((f(q)+q^2) - (f(v)+v^2)) / (2q - 2v)
        ((fq + qf * qf) - (fv + vf * vf)) / (2.0 * (qf - vf))
    };
    // k < 0 means "no finite site seen yet"; sites with f = INF are
    // skipped because their parabola can never be the lower envelope.
    let mut k: isize = -1;
    for q in 0..n {
        let fq = f[q];
        if fq.is_infinite() {
            continue;
        }
        let qf = q as f64;
        if k < 0 {
            k = 0;
            v[0] = q;
            z[0] = f64::NEG_INFINITY;
            z[1] = f64::INFINITY;
            continue;
        }
        let mut ku = k as usize;
        let mut s = intersect(fq, qf, f[v[ku]], v[ku] as f64);
        while s <= z[ku] {
            k -= 1;
            if k < 0 {
                break;
            }
            ku = k as usize;
            s = intersect(fq, qf, f[v[ku]], v[ku] as f64);
        }
        if k < 0 {
            k = 0;
            v[0] = q;
            z[0] = f64::NEG_INFINITY;
            z[1] = f64::INFINITY;
        } else {
            k += 1;
            let ku = k as usize;
            v[ku] = q;
            z[ku] = s;
            z[ku + 1] = f64::INFINITY;
        }
    }
    if k < 0 {
        d.iter_mut().for_each(|x| *x = f64::INFINITY);
        return;
    }
    let mut kk = 0usize;
    for (q, dq) in d.iter_mut().enumerate() {
        while z[kk + 1] < q as f64 {
            kk += 1;
        }
        let dx = q as f64 - v[kk] as f64;
        *dq = dx * dx + f[v[kk]];
    }
}

/// Squared distance (in cells^2) to the nearest sample where `target` is
/// nonzero. All-target or no-target give `None` (degenerate).
fn squared_edt(grid: &Grid, target: &[u8]) -> Option<Vec<f64>> {
    let any = target.iter().any(|&t| t != 0);
    let all = target.iter().all(|&t| t != 0);
    if !any || all {
        return None;
    }
    let mut f = vec![f64::INFINITY; grid.len()];
    for (i, &t) in target.iter().enumerate() {
        if t != 0 {
            f[i] = 0.0;
        }
    }
    let mut d = vec![0.0; grid.len()];
    let mut v = vec![0usize; grid.nx.max(grid.nz)];
    let mut z = vec![0.0; grid.nx.max(grid.nz) + 1];
    // Rows.
    for j in 0..grid.nz {
        let row = &mut f[j * grid.nx..(j + 1) * grid.nx];
        let mut drow = vec![0.0; grid.nx];
        edt_1d(row, &mut drow, &mut v[..grid.nx], &mut z[..grid.nx + 1]);
        for (i, val) in drow.iter().enumerate() {
            d[j * grid.nx + i] = *val;
        }
    }
    // Columns (operate on d as the new f).
    for i in 0..grid.nx {
        let col: Vec<f64> = (0..grid.nz).map(|j| d[j * grid.nx + i]).collect();
        let mut dcol = vec![0.0; grid.nz];
        edt_1d(&col, &mut dcol, &mut v[..grid.nz], &mut z[..grid.nz + 1]);
        for (j, val) in dcol.iter().enumerate() {
            d[j * grid.nx + i] = *val;
        }
    }
    Some(d)
}

/// Sentinel for the degenerate all-land / no-land distance cases.
/// Finite and much larger than any world feature.
const FAR: f64 = 1.0e9;

/// Signed distances used by the height stage.
///
/// `coast`: signed distance to the land edge in meters, positive on
/// land. `mountain`: distance inside the mountain mask to its outside,
/// in meters (0 outside the mask).
pub struct Distances {
    pub coast: Vec<f64>,
    pub mountain: Vec<f64>,
}

pub fn build_distances(grid: &Grid, masks: &Masks) -> Distances {
    // Coast distance: EDT of land for non-land samples, EDT of !land for
    // land samples; shift by half a cell so the coastline sits between
    // nodes, not on them.
    let not_land: Vec<u8> = masks.land.iter().map(|&l| 1 - l).collect();
    let to_not_land = squared_edt(grid, &not_land);
    let to_land = squared_edt(grid, &masks.land);
    let mut coast = vec![0.0; grid.len()];
    for i in 0..grid.len() {
        coast[i] = if masks.land[i] == 1 {
            match &to_not_land {
                Some(d2) => d2[i].sqrt() * grid.cell - grid.cell / 2.0,
                None => FAR, // all land
            }
        } else {
            match &to_land {
                Some(d2) => -(d2[i].sqrt() * grid.cell - grid.cell / 2.0),
                None => -FAR, // no land
            }
        };
    }
    let not_mountain: Vec<u8> = masks.mountain.iter().map(|&m| 1 - m).collect();
    let mut mountain = vec![0.0; grid.len()];
    if let Some(d2) = squared_edt(grid, &not_mountain) {
        for i in 0..grid.len() {
            mountain[i] = d2[i].sqrt() * grid.cell;
        }
    }
    Distances { coast, mountain }
}

/// Bilinear interpolation of a node grid at world point (x, z).
/// Bit-exact at nodes: written as `a + t*(b - a)`, so t = 0 returns `a`.
pub fn bilinear(grid: &Grid, plane: &[f64], x: f64, z: f64) -> f64 {
    let fx = ((x - grid.origin_x) / grid.cell).clamp(0.0, (grid.nx - 1) as f64);
    let fz = ((z - grid.origin_z) / grid.cell).clamp(0.0, (grid.nz - 1) as f64);
    let i = (fx.floor() as usize).min(grid.nx - 2);
    let j = (fz.floor() as usize).min(grid.nz - 2);
    let tx = fx - i as f64;
    let tz = fz - j as f64;
    let n = grid.nx;
    let a = plane[j * n + i];
    let b = plane[j * n + i + 1];
    let c = plane[(j + 1) * n + i];
    let d = plane[(j + 1) * n + i + 1];
    let top = a + tx * (b - a);
    let bot = c + tx * (d - c);
    top + tz * (bot - top)
}

/// World-function terrain height context: the coarse grid exists only to
/// supply the two distance planes; every other input is evaluated at the
/// world point directly, so `height_at` is defined at ANY (x, z) — the
/// coarse grid is this function sampled at nodes, and fine regions sample
/// it more densely. This is the S4 invariant.
pub struct HeightCtx {
    pub grid: Grid,
    pub coast: Vec<f64>,
    pub mountain: Vec<f64>,
    pub sea_level_m: f64,
    pub terrain: mb_schema::TerrainParams,
    pub profile: mb_schema::TerrainProfile,
    pub perlin_f: mb_geom::Perlin,
    pub perlin_r: mb_geom::Perlin,
    pub fbm_params: mb_geom::FbmParams,
}

impl HeightCtx {
    pub fn new(
        grid: Grid,
        dist: &Distances,
        sea_level_m: f64,
        project_seed: u64,
        terrain: mb_schema::TerrainParams,
        profile: mb_schema::TerrainProfile,
    ) -> Self {
        let fbm_seed = mb_seed::derive(project_seed, "terrain", "fbm");
        let ridged_seed = mb_seed::derive(project_seed, "terrain", "ridged");
        HeightCtx {
            grid,
            coast: dist.coast.clone(),
            mountain: dist.mountain.clone(),
            sea_level_m,
            terrain,
            profile,
            perlin_f: mb_geom::Perlin::new(fbm_seed),
            perlin_r: mb_geom::Perlin::new(ridged_seed),
            fbm_params: mb_geom::FbmParams {
                octaves: terrain.octaves,
                lacunarity: terrain.lacunarity,
                gain: terrain.gain,
                frequency: 1.0 / profile.feature_size_m,
            },
        }
    }

    /// Noise amplitude factor: 0 in the sea and at the coast.
    pub fn amplitude(&self, d: f64) -> f64 {
        if d >= 0.0 {
            smoothstep(0.0, self.profile.inland_width_m, d)
        } else {
            0.0
        }
    }

    /// Terrain height at any world point. Coast/mountain distance come
    /// from bilinear interpolation of the coarse planes (bit-exact at
    /// nodes); noise is evaluated directly at (x, z).
    pub fn height_at(&self, x: f64, z: f64) -> f64 {
        let p = &self.profile;
        let t = &self.terrain;
        let d = bilinear(&self.grid, &self.coast, x, z);
        let m_dist = bilinear(&self.grid, &self.mountain, x, z);
        let base = if d < 0.0 {
            -p.shelf_depth_m * smoothstep(0.0, p.shelf_width_m, -d)
        } else {
            p.coast_height_m
                + (p.inland_height_m - p.coast_height_m) * smoothstep(0.0, p.inland_width_m, d)
        };
        let a = self.amplitude(d);
        let m = smoothstep(0.0, p.mountain_falloff_m, m_dist);
        let n = (1.0 - t.ridged_mix) * mb_geom::fbm(&self.perlin_f, x, z, &self.fbm_params)
            + t.ridged_mix
                * (2.0 * mb_geom::ridged_fbm(&self.perlin_r, x, z, &self.fbm_params) - 1.0);
        self.sea_level_m
            + base
            + t.relief_m * a * n
            + p.mountain_height_m
                * m
                * a
                * mb_geom::ridged_fbm(&self.perlin_r, x, z, &self.fbm_params)
    }

    /// Coast distance at any world point (bilinear; for vertex colors).
    pub fn coast_at(&self, x: f64, z: f64) -> f64 {
        bilinear(&self.grid, &self.coast, x, z)
    }
}

/// Everything the height stage needs, resolved once.
pub struct HeightInputs<'a> {
    pub grid: &'a Grid,
    pub masks: &'a Masks,
    pub dist: &'a Distances,
    pub sea_level_m: f64,
    pub project_seed: u64,
    pub terrain: mb_schema::TerrainParams,
    pub profile: &'a mb_schema::TerrainProfile,
}

/// The height field plus its global min/max.
pub struct HeightField {
    pub h: Vec<f64>,
    pub h_min: f64,
    pub h_max: f64,
}

pub fn build_height(inp: &HeightInputs) -> HeightField {
    let g = inp.grid;
    let terrain = inp.terrain;
    let ctx = HeightCtx::new(
        *g,
        inp.dist,
        inp.sea_level_m,
        inp.project_seed,
        terrain,
        *inp.profile,
    );
    // The coarse grid is just height_at sampled at nodes. Node coords
    // come from the shared formula origin + i*spacing everywhere.
    let mut h = vec![0.0; g.len()];
    for j in 0..g.nz {
        for i in 0..g.nx {
            h[j * g.nx + i] = ctx.height_at(g.x(i), g.z(j));
        }
    }
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &v in &h {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    HeightField {
        h,
        h_min: lo,
        h_max: hi,
    }
}

/// Quantize a height to u16 over the shared [h_min, h_max] range.
/// If max == min everything is 0.
pub fn quantize(h: f64, h_min: f64, h_max: f64) -> u16 {
    if h_max <= h_min {
        return 0;
    }
    ((h - h_min) / (h_max - h_min) * 65535.0).round() as u16
}

/// Inverse of [`quantize`].
pub fn dequantize(q: u16, h_min: f64, h_max: f64) -> f64 {
    h_min + q as f64 * (h_max - h_min) / 65535.0
}

/// Quantization step in meters.
pub fn quant_step(h_min: f64, h_max: f64) -> f64 {
    (h_max - h_min) / 65535.0
}
