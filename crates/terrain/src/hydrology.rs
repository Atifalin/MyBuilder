//! Generator 3 hydrology: priority-flood, D8 flow, lakes, rivers,
//! carving — all on the coarse node grid, deterministic (f64 +
//! total_cmp, ties by node index, no HashMap).

use crate::Grid;
use mb_schema::HydrologySection;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

const EPS: f64 = 1e-4;
/// Water-level sentinel: "no water here" is encoded as -inf so the
/// plane is byte-stable.
pub const NO_WATER: f64 = f64::NEG_INFINITY;

#[derive(Clone, Copy)]
struct Key(f64, usize);
impl PartialEq for Key {
    fn eq(&self, o: &Self) -> bool {
        self.0 == o.0 && self.1 == o.1
    }
}
impl Eq for Key {}
impl PartialOrd for Key {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Key {
    fn cmp(&self, o: &Self) -> Ordering {
        // MIN-heap: smaller filled_h first, ties by node index.
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

pub struct Hydro {
    /// Carved final heights (≤ raw h).
    pub final_h: Vec<f64>,
    /// final − h (≤ 0).
    pub delta: Vec<f64>,
    /// Lake level / river surface (bed + 0.8·depth), NO_WATER elsewhere.
    pub water: Vec<f64>,
    pub river: Vec<u8>,
    pub lake: Vec<u8>,
    /// Accumulated catchment area (m²).
    pub acc: Vec<f64>,
    /// (id, level, area_m2) lakes, in BFS label order.
    pub lakes: Vec<(u32, f64, f64)>,
    /// Receiver per node (usize::MAX for outlets/borders).
    pub recv: Vec<usize>,
    /// River bed heights (rivers only).
    pub bed: Vec<f64>,
    /// min(coast, distance to the nearest river/lake node) in meters —
    /// the splat "sand near water" input for gen >= 3.
    pub water_dist: Vec<f64>,
    /// Water level for every node: wet nodes carry their own level,
    /// dry nodes the nearest wet node's level (so water mesh edges
    /// don't sag). NO_WATER where no wet node exists at all.
    pub water_surface: Vec<f64>,
}

const N8: [(isize, isize); 8] = [
    (-1, 0),
    (1, 0),
    (0, -1),
    (0, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];

pub fn run(
    grid: &Grid,
    h: &[f64],
    masks_land: &[u8],
    sea: f64,
    t: &HydrologySection,
    coast: &[f64],
    poll: &dyn Fn() -> bool,
) -> Hydro {
    let (nx, nz, cell) = (grid.nx, grid.nz, grid.cell);
    let n = nx * nz;
    let timing = std::env::var("MB_HYDRO_TIME").is_ok();
    let tp = |name: &str, t: std::time::Instant| {
        if timing {
            eprintln!("[hydro] {name}: {:?}", t.elapsed());
        }
    };
    let t0 = std::time::Instant::now();
    // 1+2: priority-flood from outlets (h < sea OR border node).
    let mut filled = vec![f64::INFINITY; n];
    let mut heap = BinaryHeap::new();
    for j in 0..nz {
        for i in 0..nx {
            let c = j * nx + i;
            if h[c] < sea || i == 0 || j == 0 || i == nx - 1 || j == nz - 1 {
                filled[c] = h[c];
                heap.push(Key(filled[c], c));
            }
        }
    }
    let mut order = Vec::with_capacity(n);
    let mut visited = vec![false; n];
    let mut pops = 0usize;
    while let Some(Key(f, c)) = heap.pop() {
        if visited[c] {
            continue;
        }
        visited[c] = true;
        filled[c] = f;
        order.push(c);
        pops += 1;
        if pops.is_multiple_of(4096) && poll() {
            return empty(n);
        }
        let (ci, cj) = (c % nx, c / nx);
        for &(di, dj) in &N8 {
            let (i2, j2) = (ci as isize + di, cj as isize + dj);
            if i2 < 0 || j2 < 0 || i2 >= nx as isize || j2 >= nz as isize {
                continue;
            }
            let nn = j2 as usize * nx + i2 as usize;
            if visited[nn] {
                continue;
            }
            let nf = h[nn].max(f + EPS);
            if nf < filled[nn] {
                filled[nn] = nf;
                heap.push(Key(nf, nn));
            }
        }
    }
    tp("flood", t0);
    let t0 = std::time::Instant::now();
    // 3: D8 receiver by steepest descent on `filled`; ties by N8 order.
    let mut recv = vec![usize::MAX; n];
    for &c in &order {
        let (ci, cj) = (c % nx, c / nx);
        let mut best = 0.0f64;
        let mut bi = usize::MAX;
        for &(di, dj) in &N8 {
            let (i2, j2) = (ci as isize + di, cj as isize + dj);
            if i2 < 0 || j2 < 0 || i2 >= nx as isize || j2 >= nz as isize {
                continue;
            }
            let nn = j2 as usize * nx + i2 as usize;
            let dist = if di != 0 && dj != 0 {
                cell * std::f64::consts::SQRT_2
            } else {
                cell
            };
            let drop = (filled[c] - filled[nn]) / dist;
            if drop > best {
                best = drop;
                bi = nn;
            }
        }
        recv[c] = bi;
    }
    // 4: accumulation in reverse pop order (uphill → downhill).
    let mut acc = vec![0.0; n];
    let a = cell * cell;
    for &c in order.iter().rev() {
        acc[c] += a;
        let r = recv[c];
        if r != usize::MAX {
            acc[r] += acc[c];
        }
    }
    tp("d8+acc", t0);
    let t0 = std::time::Instant::now();
    if poll() {
        return empty(n);
    }
    // 5: lakes = land nodes with fill depth > min, components ≥ min area.
    let mut fill_depth = vec![0.0; n];
    let mut lake = vec![0u8; n];
    for c in 0..n {
        fill_depth[c] = filled[c] - h[c];
        if masks_land[c] == 1 && fill_depth[c] > t.lake_min_depth_m {
            lake[c] = 1;
        }
    }
    let mut lakes: Vec<(u32, f64, f64)> = Vec::new();
    let mut comp_id = vec![usize::MAX; n];
    let mut cid = 0u32;
    for start in 0..n {
        if lake[start] != 1 || comp_id[start] != usize::MAX {
            continue;
        }
        // BFS 8-conn in index order.
        let mut stack = vec![start];
        let mut members = Vec::new();
        comp_id[start] = cid as usize;
        while let Some(c) = stack.pop() {
            members.push(c);
            let (ci, cj) = (c % nx, c / nx);
            for &(di, dj) in &N8 {
                let (i2, j2) = (ci as isize + di, cj as isize + dj);
                if i2 < 0 || j2 < 0 || i2 >= nx as isize || j2 >= nz as isize {
                    continue;
                }
                let nn = j2 as usize * nx + i2 as usize;
                if lake[nn] == 1 && comp_id[nn] == usize::MAX {
                    comp_id[nn] = cid as usize;
                    stack.push(nn);
                }
            }
        }
        let area = members.len() as f64 * a;
        if area >= t.lake_min_area_m2 {
            let level = members.iter().map(|&m| filled[m]).fold(0.0, f64::max);
            for &m in &members {
                lake[m] = 2; // confirmed lake
            }
            lakes.push((cid, level, area));
            cid += 1;
        } else {
            for &m in &members {
                lake[m] = 0; // too small
            }
        }
        if poll() {
            return empty(n);
        }
    }
    tp("lakes", t0);
    let t0 = std::time::Instant::now();
    let is_lake = |c: usize| lake[c] == 2;
    // 6+7: rivers (land, acc ≥ threshold, not lake) + beds.
    let mut river = vec![0u8; n];
    let mut depth = vec![0.0; n];
    let mut halfw = vec![0.0; n];
    let mut bed = vec![0.0; n];
    for c in 0..n {
        if masks_land[c] == 1 && !is_lake(c) && acc[c] >= t.river_min_catchment_m2 {
            river[c] = 1;
            let q = (acc[c] / t.river_min_catchment_m2).sqrt();
            depth[c] = (t.carve_depth_m * q).min(4.0 * t.carve_depth_m);
            halfw[c] = (t.carve_width_m * q).min(t.max_half_width_m);
            bed[c] = filled[c] - depth[c];
        }
    }
    // Strict downhill beds along each river's receiver chain (reverse
    // pop order: donors first).
    for &c in order.iter().rev() {
        if river[c] != 1 {
            continue;
        }
        let r = recv[c];
        if r == usize::MAX || river[r] != 1 {
            continue; // lake/outlet receiver ends the river
        }
        let (ci, cj, ri, rj) = (c % nx, c / nx, r % nx, r / nx);
        let dist = if ci != ri && cj != rj {
            cell * std::f64::consts::SQRT_2
        } else {
            cell
        };
        let want = bed[c] - t.min_bed_slope * dist;
        if want < bed[r] {
            bed[r] = want;
        }
    }
    tp("rivers+beds", t0);
    let t0 = std::time::Instant::now();
    // 8: carving via nearest-river-site EDT (argmin variant of edt_1d).
    let (dist_cells, site) = nearest_site_edt(&river, nx, nz);
    let dist: Vec<f64> = dist_cells.iter().map(|d| d * cell).collect();
    // final_h: carve where within half-width of a river.
    let mut final_h = h.to_vec();
    let mut delta = vec![0.0; n];
    for c in 0..n {
        let s = site[c];
        if s == usize::MAX {
            continue;
        }
        let hw = halfw[s];
        if hw <= 0.0 || dist[c] >= hw {
            continue;
        }
        let tt = dist[c] / hw;
        let carved = bed[s] + (filled[s] - bed[s]) * tt * tt;
        if carved < final_h[c] {
            final_h[c] = carved;
            delta[c] = carved - h[c];
        }
        if poll() {
            return empty(n);
        }
    }
    tp("edt+carve", t0);
    let t0 = std::time::Instant::now();
    // water plane
    let mut water = vec![NO_WATER; n];
    for c in 0..n {
        if is_lake(c) {
            let lv = lakes[comp_id[c] as usize].1;
            water[c] = lv;
        } else if river[c] == 1 {
            water[c] = bed[c] + 0.8 * depth[c];
        }
    }
    tp("water", t0);
    Hydro {
        final_h,
        delta,
        water: water.clone(),
        river: river.clone(),
        lake: (0..n).map(|c| is_lake(c) as u8).collect(),
        acc,
        lakes,
        recv,
        bed,
        water_dist: {
            // wet mask = river OR confirmed lake
            let mut wet = vec![0u8; n];
            for c in 0..n {
                if river[c] == 1 || is_lake(c) {
                    wet[c] = 1;
                }
            }
            let (d_cells, _sites) = nearest_site_edt(&wet, nx, nz);
            (0..n)
                .map(|c| {
                    let dw = d_cells[c] * cell;
                    coast[c].min(dw)
                })
                .collect()
        },
        water_surface: {
            // Wet nodes keep their level; dry nodes take the nearest
            // wet node's level so water mesh borders don't sag.
            let mut wet = vec![0u8; n];
            for c in 0..n {
                if water[c] != NO_WATER {
                    wet[c] = 1;
                }
            }
            let (_d, sites) = nearest_site_edt(&wet, nx, nz);
            (0..n)
                .map(|c| {
                    if wet[c] == 1 {
                        water[c]
                    } else if sites[c] != usize::MAX {
                        water[sites[c]]
                    } else {
                        NO_WATER
                    }
                })
                .collect()
        },
    }
}

fn empty(n: usize) -> Hydro {
    Hydro {
        final_h: vec![0.0; n],
        delta: vec![0.0; n],
        water: vec![0.0; n],
        river: vec![0; n],
        lake: vec![0; n],
        acc: vec![0.0; n],
        lakes: vec![],
        recv: vec![usize::MAX; n],
        bed: vec![0.0; n],
        water_dist: vec![0.0; n],
        water_surface: vec![0.0; n],
    }
}

/// Exact Euclidean distance (in cells) from every node to the nearest
/// node where `mask != 0`, plus that nearest node's index (`usize::MAX`
/// and `INFINITY` when the mask is empty). Separable Felzenszwalb-
/// Huttenlocher: the column pass picks the row whose parabola wins, so
/// carrying that row's site gives the exact 2D nearest site.
pub fn nearest_site_edt(mask: &[u8], nx: usize, nz: usize) -> (Vec<f64>, Vec<usize>) {
    let n = nx * nz;
    let inf = f64::INFINITY;
    let mut d2 = vec![inf; n];
    let mut site = vec![usize::MAX; n];
    let m = nx.max(nz);
    let (mut f, mut ids) = (vec![inf; m], vec![usize::MAX; m]);
    let (mut d, mut out) = (vec![0.0; m], vec![usize::MAX; m]);
    for j in 0..nz {
        for i in 0..nx {
            let c = j * nx + i;
            f[i] = if mask[c] != 0 { 0.0 } else { inf };
            ids[i] = c;
        }
        edt_1d_sq_sites(&f[..nx], &ids[..nx], &mut d[..nx], &mut out[..nx]);
        for i in 0..nx {
            d2[j * nx + i] = d[i];
            site[j * nx + i] = out[i];
        }
    }
    for i in 0..nx {
        for j in 0..nz {
            f[j] = d2[j * nx + i];
            ids[j] = site[j * nx + i];
        }
        edt_1d_sq_sites(&f[..nz], &ids[..nz], &mut d[..nz], &mut out[..nz]);
        for j in 0..nz {
            d2[j * nx + i] = d[j];
            site[j * nx + i] = out[j];
        }
    }
    (d2.iter().map(|v| v.sqrt()).collect(), site)
}

/// 1D squared EDT over `f` (squared sample distances, `INFINITY` = not a
/// site) that also reports, per position, the id carried by the winning
/// parabola. Infinite entries are skipped so they never join the envelope.
fn edt_1d_sq_sites(f: &[f64], ids: &[usize], d: &mut [f64], out: &mut [usize]) {
    let n = f.len();
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let mut k: isize = -1;
    for q in 0..n {
        if f[q].is_infinite() {
            continue;
        }
        let qf = q as f64;
        loop {
            if k < 0 {
                k = 0;
                v[0] = q;
                z[0] = f64::NEG_INFINITY;
                z[1] = f64::INFINITY;
                break;
            }
            let p = v[k as usize];
            let pf = p as f64;
            let s = ((f[q] + qf * qf) - (f[p] + pf * pf)) / (2.0 * (qf - pf));
            if s <= z[k as usize] {
                k -= 1;
                continue;
            }
            k += 1;
            let ku = k as usize;
            v[ku] = q;
            z[ku] = s;
            z[ku + 1] = f64::INFINITY;
            break;
        }
    }
    if k < 0 {
        d.fill(f64::INFINITY);
        out.fill(usize::MAX);
        return;
    }
    let mut kk = 0usize;
    for q in 0..n {
        while z[kk + 1] < q as f64 {
            kk += 1;
        }
        let dx = q as f64 - v[kk] as f64;
        d[q] = dx * dx + f[v[kk]];
        out[q] = ids[v[kk]];
    }
}

#[cfg(test)]
mod tests {
    use super::nearest_site_edt;

    #[test]
    fn nearest_site_edt_matches_brute_force() {
        let (nx, nz) = (23, 17);
        for seed in 0..20u64 {
            let mut rng = mb_seed::Rng::new(seed);
            let mask: Vec<u8> = (0..nx * nz)
                .map(|_| (rng.range_u64(25) == 0) as u8)
                .collect();
            let (dist, site) = nearest_site_edt(&mask, nx, nz);
            let sites: Vec<usize> = (0..mask.len()).filter(|&c| mask[c] != 0).collect();
            for c in 0..nx * nz {
                let (ci, cj) = ((c % nx) as f64, (c / nx) as f64);
                let d2 = |s: usize| {
                    let (dx, dz) = ((s % nx) as f64 - ci, (s / nx) as f64 - cj);
                    dx * dx + dz * dz
                };
                let best = sites.iter().map(|&s| d2(s)).fold(f64::INFINITY, f64::min);
                if sites.is_empty() {
                    assert!(dist[c].is_infinite() && site[c] == usize::MAX);
                    continue;
                }
                assert!(
                    (dist[c] * dist[c] - best).abs() < 1e-9,
                    "seed {seed} node {c}"
                );
                assert_eq!(mask[site[c]], 1);
                assert!((d2(site[c]) - best).abs() < 1e-9, "site not nearest");
            }
        }
    }
}
