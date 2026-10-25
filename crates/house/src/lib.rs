//! Phase 3 step 1b: Mughal-style house geometry generator.
#![allow(
    clippy::needless_range_loop,
    clippy::collapsible_if,
    clippy::too_many_arguments,
    clippy::field_reassign_with_default
)]
//! Rectilinear plan geometry in integer mm; f32 only at mesh output.

use mb_schema::{Footprint, HouseSpec, Side};

pub mod glb;
pub mod parts;

pub const CURRENT_HOUSE_GENERATOR_VERSION: u32 = 2;

pub fn house_seed(project_seed: u64, id: &str) -> u64 {
    mb_seed::derive_path(project_seed, &[("house", id)])
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mat {
    #[default]
    Plaster,
    Stone,
    Roof,
    Floor,
    Opening,
    Wood,
}
pub(crate) const MATS_ORDER: [Mat; 6] = MATS;
const MATS: [Mat; 6] = [
    Mat::Plaster,
    Mat::Stone,
    Mat::Roof,
    Mat::Floor,
    Mat::Opening,
    Mat::Wood,
];

#[derive(Default)]
pub struct Prim {
    pub mat: Mat,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}
impl Clone for Prim {
    fn clone(&self) -> Self {
        Self {
            mat: self.mat,
            positions: self.positions.clone(),
            normals: self.normals.clone(),
            uvs: self.uvs.clone(),
            indices: self.indices.clone(),
        }
    }
}
impl std::fmt::Debug for Prim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Prim({:?},{})", self.mat, self.indices.len() / 3)
    }
}

pub struct Lod {
    pub prims: Vec<Prim>,
}
pub struct BoxCollider {
    pub center: [f32; 3],
    pub half: [f32; 3],
}
pub struct HouseMesh {
    /// Instanced part placements (Phase 3 step 2).
    pub instances: Vec<parts::PartInstance>,
    pub lods: [Lod; 3],
    pub colliders: Vec<BoxCollider>,
    pub plaster_rgb: [f32; 3],
    pub bounds: ([f32; 3], [f32; 3]),
}

pub struct OpeningInfo {
    pub piece: usize,
    pub storey: u32,
    pub u0: f64,
    pub u1: f64,
    pub v0: f64,
    pub v1: f64,
    pub kind: &'static str, // "window" | "door"
    pub arch: bool,
}

// ---- unit-circle literal tables (no runtime trig) ----
// Pointed arch arcs: angle theta from the spring (0) to the apex
// (acos(1/3) = 70.5288 deg), in 2/4/6 equal steps; cos/sin literals.
const AC2: [f64; 3] = [1.0, 0.8164966, 0.3333333];
const AS2: [f64; 3] = [0.0, 0.5773503, 0.9428090];
const AC4: [f64; 5] = [1.0, 0.9532012, 0.8164966, 0.6030227, 0.3333333];
const AS4: [f64; 5] = [0.0, 0.3023998, 0.5773503, 0.7977240, 0.9428090];

const MM: f64 = 1000.0;
pub(crate) fn mm(m: f64) -> i64 {
    mb_geom::snap(m)
}
pub(crate) fn m(mm_: i64) -> f32 {
    mm_ as f32 / 1000.0
}

type P2 = [i64; 2];

pub(crate) struct Bld {
    prims: [Prim; MATS.len()],
}
impl Bld {
    fn new() -> Self {
        let mut p: [Prim; MATS.len()] = Default::default();
        for (i, mt) in MATS.iter().enumerate() {
            p[i].mat = *mt;
        }
        Bld { prims: p }
    }
    /// Emit a polygon (convex, fan) with flat normal; uv given per vertex.
    pub(crate) fn poly(
        &mut self,
        mat: Mat,
        pts: &[[f32; 3]],
        n: [f32; 3],
        uv: &[[f32; 2]],
        rev: bool,
    ) {
        let pr = &mut self.prims[mat as usize];
        let base = pr.positions.len() as u32;
        pr.positions.extend_from_slice(pts);
        pr.normals.extend(std::iter::repeat_n(n, pts.len()));
        pr.uvs.extend_from_slice(uv);
        let k = pts.len() as u32;
        for i in 1..k - 1 {
            if rev {
                pr.indices
                    .extend_from_slice(&[base, base + i + 1, base + i]);
            } else {
                pr.indices
                    .extend_from_slice(&[base, base + i, base + i + 1]);
            }
        }
    }
    /// Quad with flat normal `n`, wound to face `n` whatever the corner
    /// order; UVs are world meters in the plane of the quad.
    fn quad_facing(&mut self, mat: Mat, q: [[f32; 3]; 4], n: [f32; 3]) {
        let e1 = [q[1][0] - q[0][0], q[1][1] - q[0][1], q[1][2] - q[0][2]];
        let e2 = [q[2][0] - q[1][0], q[2][1] - q[1][1], q[2][2] - q[1][2]];
        let cr = [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ];
        let rev = cr[0] * n[0] + cr[1] * n[1] + cr[2] * n[2] < 0.0;
        let uv: Vec<[f32; 2]> = q
            .iter()
            .map(|p| {
                if n[1].abs() > 0.5 {
                    [p[0], p[2]]
                } else {
                    [p[0] + p[2], p[1]]
                }
            })
            .collect();
        self.poly(mat, &q, n, &uv, rev);
    }
    /// Quad helper (a,b,c,d CCW as seen from `n`).
    #[allow(clippy::too_many_arguments)]
    fn quad(
        &mut self,
        mat: Mat,
        a: [f32; 3],
        b: [f32; 3],
        c: [f32; 3],
        d: [f32; 3],
        n: [f32; 3],
        uv: &[[f32; 2]],
        rev: bool,
    ) {
        self.poly(mat, &[a, b, c, d], n, uv, rev);
    }
}

/// Signed area of a CCW polygon (mm^2).
fn inward_offset(poly: &[P2], t: i64) -> Vec<P2> {
    // Inward normal of edge (a->b) for CCW poly: (-dz, dx)/|d|.
    let n = poly.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let a = poly[(i + n - 1) % n];
        let b = poly[i];
        let c = poly[(i + 1) % n];
        let norm = |p: P2, q: P2| -> (i64, i64) {
            let dx = q[0] - p[0];
            let dz = q[1] - p[1];
            let l = ((dx * dx + dz * dz) as f64).sqrt();
            (
                ((-dz) as f64 * t as f64 / l) as i64,
                (dx as f64 * t as f64 / l) as i64,
            )
        };
        let (n1x, n1z) = norm(a, b);
        let (n2x, n2z) = norm(b, c);
        out.push([b[0] + n1x + n2x, b[1] + n1z + n2z]);
    }
    out
}

fn plan(spec: &HouseSpec) -> (Vec<P2>, Option<Vec<P2>>) {
    match spec.footprint {
        Footprint::Rect { w, d } => {
            let (hw, hd) = (mm(w) / 2, mm(d) / 2);
            (vec![[-hw, -hd], [hw, -hd], [hw, hd], [-hw, hd]], None)
        }
        Footprint::L { w, d, cut_w, cut_d } => {
            // Rect minus the +X/+Z corner block.
            let (hw, hd, cw, cd) = (mm(w) / 2, mm(d) / 2, mm(cut_w), mm(cut_d));
            (
                vec![
                    [-hw, -hd],
                    [hw, -hd],
                    [hw, hd - cd],
                    [hw - cw, hd - cd],
                    [hw - cw, hd],
                    [-hw, hd],
                ],
                None,
            )
        }
        Footprint::Courtyard {
            w,
            d,
            court_w,
            court_d,
        } => {
            let (hw, hd) = (mm(w) / 2, mm(d) / 2);
            let (cw, cd) = (mm(court_w) / 2, mm(court_d) / 2);
            (
                vec![[-hw, -hd], [hw, -hd], [hw, hd], [-hw, hd]],
                Some(vec![[-cw, -cd], [cw, -cd], [cw, cd], [-cw, cd]]),
            )
        }
    }
}

/// Edge outward normal (unit, 2D) for a CCW polygon edge index.
fn edge_normal(a: P2, b: P2) -> (f64, f64) {
    let dx = (b[0] - a[0]) as f64;
    let dz = (b[1] - a[1]) as f64;
    let l = (dx * dx + dz * dz).sqrt();
    (dz / l, -dx / l) // right side of CCW edge = outward
}

fn side_dir(s: Side) -> (f64, f64) {
    match s {
        Side::S => (0.0, -1.0),
        Side::N => (0.0, 1.0),
        Side::E => (1.0, 0.0),
        Side::W => (-1.0, 0.0),
    }
}

/// Arch-topped opening outline in uv mm: rect sill..spring plus pointed
/// arch (two arcs, centres +-w/4 at spring, r=0.75w, apex at +0.7071w).
fn opening_poly(u0: i64, v0: i64, w: i64, h: i64, arch: bool, seg: usize) -> Vec<P2> {
    if !arch {
        return vec![[u0, v0], [u0 + w, v0], [u0 + w, v0 + h], [u0, v0 + h]];
    }
    let w2 = w as f64;
    // Full pointed-arch rise is sqrt(0.75^2 - 0.25^2) * w = w / sqrt(2);
    // on wide, short openings squash it vertically to 60% of the height.
    let full = w2 * std::f64::consts::FRAC_1_SQRT_2;
    let sy = (0.6 * h as f64 / full).min(1.0);
    let ha = (full * sy).round() as i64;
    let spring = v0 + h - ha;
    let cx = u0 as f64 + w2 / 2.0;
    let (cos_t, sin_t) = match seg {
        0..=2 => (&AC2[..], &AS2[..]),
        _ => (&AC4[..], &AS4[..]),
    };
    // Right arc: centre (cx - w/4, spring), points centre + r*(cos, sin),
    // from the right spring (u0 + w) up to the apex (cx).
    let mut pts = vec![[u0, v0], [u0 + w, v0], [u0 + w, spring]];
    // Right arc from right spring to apex (mirror of left arc).
    let mut right = Vec::new();
    for i in 0..cos_t.len() - 1 {
        let x = cx - w2 / 4.0 + (0.75 * w2) * cos_t[i];
        let y = spring as f64 + (0.75 * w2) * sin_t[i] * sy;
        right.push([x.round() as i64, y.round() as i64]);
    }
    pts.extend_from_slice(&right);
    pts.push([cx.round() as i64, spring + ha]); // apex
    // Left arc (mirror): centre (cx + w/4, spring), points
    // centre + r*(-cos, sin), from the apex down to the left spring (u0).
    for i in (0..cos_t.len() - 1).rev() {
        let x = cx + w2 / 4.0 - (0.75 * w2) * cos_t[i];
        let y = spring as f64 + (0.75 * w2) * sin_t[i] * sy;
        let px = [x.round() as i64, y.round() as i64];
        if pts.last() != Some(&px) {
            pts.push(px);
        }
    }
    pts.push([u0, spring]);
    pts.dedup();
    pts
}

struct Piece {
    a: P2,
    b: P2,
    ia: P2,
    ib: P2,
    len: i64,
    dir: (f64, f64),
    nrm: (f64, f64),
    openings: Vec<(Vec<P2>, &'static str)>, // uv polys + kind
}

#[allow(clippy::too_many_arguments)]
fn storey_pieces(
    outer: &[P2],
    inner: &[P2],
    front_dir: (f64, f64),
    storey: u32,
    _rng: &mut mb_seed::Rng,
    pitch: f64,
    kw: f64,
    spec: &HouseSpec,
    infos: &mut Vec<OpeningInfo>,
) -> Vec<Piece> {
    let t = mm(spec.wall_thickness_m);
    let _ = t;
    let n = outer.len();
    let seg = [2usize, 4, 4][spec.complexity.min(2) as usize];
    let mut pieces = Vec::new();
    // front-facing edge index with max length
    let mut front_i = usize::MAX;
    let mut front_len = 0i64;
    for i in 0..n {
        let (a, b) = (outer[i], outer[(i + 1) % n]);
        let (nx, nz) = edge_normal(a, b);
        let l = (b[0] - a[0]).abs() + (b[1] - a[1]).abs();
        if (nx - front_dir.0).abs() < 0.1 && (nz - front_dir.1).abs() < 0.1 && l > front_len {
            front_len = l;
            front_i = i;
        }
    }
    for i in 0..n {
        let (a, b) = (outer[i], outer[(i + 1) % n]);
        let (ia, ib) = (inner[i], inner[(i + 1) % n]);
        let len = ((b[0] - a[0]) as f64).hypot((b[1] - a[1]) as f64) as i64;
        let dir = (
            (b[0] - a[0]) as f64 / len as f64,
            (b[1] - a[1]) as f64 / len as f64,
        );
        let nrm = edge_normal(a, b);
        let mut pc = Piece {
            a,
            b,
            ia,
            ib,
            len,
            dir,
            nrm,
            openings: Vec::new(),
        };
        // Facade fitting.
        let usable = len - mm(1.2);
        if usable > mm(2.0) {
            let nbays = ((usable as f64 / (pitch * MM)).round() as i64).max(1);
            let bay = usable / nbays;
            let w = ((bay as f64 * kw).round() as i64)
                .min(bay - mm(0.2))
                .max(mm(0.6));
            for bi in 0..nbays {
                let u0 = mm(0.6) + bi * bay + (bay - w) / 2;
                if u0 < mm(0.6) || u0 + w > len - mm(0.6) {
                    continue;
                }
                pc.openings.push((
                    opening_poly(u0, mm(0.9), w, mm(1.5), spec.complexity >= 1, seg),
                    "window",
                ));
                infos.push(OpeningInfo {
                    piece: i,
                    storey,
                    u0: u0 as f64 / MM,
                    u1: (u0 + w) as f64 / MM,
                    v0: 0.9,
                    v1: 2.4,
                    kind: "window",
                    arch: spec.complexity >= 1,
                });
            }
        }
        pieces.push(pc);
    }
    // Door at the centre of the front piece; windows it would overlap
    // (with a 0.3 m pier on each side) are dropped.
    if front_i != usize::MAX && storey == 0 {
        let pc = &mut pieces[front_i];
        let dw = mm(1.4);
        let dh = mm(2.6);
        let u0 = (pc.len - dw) / 2;
        if u0 >= mm(0.6) && pc.len - (u0 + dw) >= mm(0.6) {
            let (lo, hi) = (u0 - mm(0.3), u0 + dw + mm(0.3));
            let clear = |a: i64, b: i64| b <= lo || a >= hi;
            pc.openings.retain(|(o, _)| {
                let (a, b) = (
                    o.iter().map(|p| p[0]).min().unwrap(),
                    o.iter().map(|p| p[0]).max().unwrap(),
                );
                clear(a, b)
            });
            infos.retain(|o| !(o.piece == front_i && o.storey == 0 && !clear(mm(o.u0), mm(o.u1))));
            pc.openings
                .push((opening_poly(u0, 0, dw, dh, true, seg), "door"));
            infos.push(OpeningInfo {
                piece: front_i,
                storey: 0,
                u0: u0 as f64 / MM,
                u1: (u0 + dw) as f64 / MM,
                v0: 0.0,
                v1: 2.6,
                kind: "door",
                arch: true,
            });
        }
    }
    pieces
}

/// Emit a wall face (with holes) for a piece edge.
/// `y0` base height, `h` storey height, direction flips for inner face.
#[allow(clippy::too_many_arguments)]
fn wall_face(
    b: &mut Bld,
    start: P2,
    dir: (f64, f64),
    y0: i64,
    h: i64,
    len: i64,
    holes: &[Vec<P2>],
    nrm: [f32; 3],
    mat: Mat,
) {
    // Openings that reach the bottom edge (doors) are notches in the
    // outline, not holes: walk the bottom edge left to right and detour
    // up and over each door, i.e. along its CCW outline in reverse.
    let (mut notches, holes): (Vec<&Vec<P2>>, Vec<&Vec<P2>>) =
        holes.iter().partition(|o| o.iter().any(|p| p[1] <= 0));
    notches.sort_by_key(|o| o.iter().map(|p| p[0]).min());
    let mut outer: Vec<P2> = vec![[0, 0]];
    for o in notches {
        let start = (0..o.len())
            .min_by_key(|&i| (o[i][1], o[i][0]))
            .expect("non-empty opening");
        outer.extend((0..o.len()).map(|k| o[(start + o.len() - k) % o.len()]));
    }
    outer.extend([[len, 0], [len, h], [0, h]]);
    outer.dedup();
    let holes: Vec<Vec<P2>> = holes.into_iter().cloned().collect();
    let tri = match mb_geom::triangulate_with_holes(&outer, &holes) {
        Ok(t) => t,
        Err(e) => panic!("wall face {e:?}: len={len} h={h} outer={outer:?} holes={holes:?}"),
    };
    let pr = &mut b.prims[mat as usize];
    let base = pr.positions.len() as u32;
    let mut emit = |u: i64, v: i64| {
        pr.positions.push([
            m(start[0]) + dir.0 as f32 * m(u),
            m(y0) + m(v),
            m(start[1]) + dir.1 as f32 * m(u),
        ]);
        pr.normals.push(nrm);
        pr.uvs.push([m(u), m(y0) + m(v)]);
    };
    for p in &outer {
        emit(p[0], p[1]);
    }
    for hole in &holes {
        for p in hole {
            emit(p[0], p[1]);
        }
    }
    for t in &tri {
        pr.indices
            .extend_from_slice(&[base + t[0], base + t[1], base + t[2]]);
    }
}

/// Reveal quads connecting an opening outline between two parallel
/// planes offset along `th` (thickness direction in plan).
#[allow(clippy::too_many_arguments)]
fn reveals(b: &mut Bld, poly: &[P2], a: P2, dir: (f64, f64), y0: i64, nxz: (f64, f64), t: f64) {
    // Reveals must follow the exact opening outline of the faces, or
    // the opening sides show gaps.
    let pts = poly;
    let n = pts.len();
    for i in 0..n {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        // quad: p_outer,q_outer,q_inner,p_inner
        let out_p = [
            m(a[0]) + dir.0 as f32 * m(p[0]),
            m(y0) + m(p[1]),
            m(a[1]) + dir.1 as f32 * m(p[0]),
        ];
        let out_q = [
            m(a[0]) + dir.0 as f32 * m(q[0]),
            m(y0) + m(q[1]),
            m(a[1]) + dir.1 as f32 * m(q[0]),
        ];
        let off = |p: [f32; 3]| {
            [
                p[0] - nxz.0 as f32 * t as f32,
                p[1],
                p[2] - nxz.1 as f32 * t as f32,
            ]
        };
        // normal perpendicular to the reveal edge in wall plane
        let eu = (q[0] - p[0]) as f64;
        let ev = (q[1] - p[1]) as f64;
        // The reveal faces the opening's centre: for a CCW outline in the
        // wall's (u along `dir`, v up) plane the inward edge normal is
        // (-ev, eu), i.e. -ev along `dir` plus eu straight up.
        let ll = (eu * eu + ev * ev).sqrt().max(1e-9);
        let nrm = [
            (-ev * dir.0 / ll) as f32,
            (eu / ll) as f32,
            (-ev * dir.1 / ll) as f32,
        ];
        // Meter UVs: u along the outline edge, v through the thickness.
        let (el, tt) = ((ll / MM) as f32, t as f32);
        let uv = [[0.0, 0.0], [el, 0.0], [el, tt], [0.0, tt]];
        b.poly(
            Mat::Stone,
            &[out_p, out_q, off(out_q), off(out_p)],
            nrm,
            &uv,
            false,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn slab(
    b: &mut Bld,
    outer: &[P2],
    hole: Option<&Vec<P2>>,
    y_top: i64,
    thick: i64,
    top_mat: Mat,
    colliders: &mut Vec<BoxCollider>,
    box_rects: &[(i64, i64, i64, i64)],
) {
    let holes: Vec<Vec<P2>> = hole.map(|h| vec![h.clone()]).unwrap_or_default();
    if let Ok(tris) = mb_geom::triangulate_with_holes(outer, &holes) {
        let pr_top = &mut b.prims[top_mat as usize];
        let base = pr_top.positions.len() as u32;
        let mut all = outer.to_vec();
        for h in &holes {
            all.extend_from_slice(h);
        }
        for p in &all {
            pr_top.positions.push([m(p[0]), m(y_top), m(p[1])]);
            pr_top.normals.push([0.0, 1.0, 0.0]);
            pr_top.uvs.push([m(p[0]), m(p[1])]);
        }
        for t in &tris {
            pr_top
                .indices
                .extend_from_slice(&[base + t[0], base + t[1], base + t[2]]);
        }
        // underside (reversed)
        let pr_bot = &mut b.prims[Mat::Stone as usize];
        let b2 = pr_bot.positions.len() as u32;
        for p in &all {
            pr_bot.positions.push([m(p[0]), m(y_top - thick), m(p[1])]);
            pr_bot.normals.push([0.0, -1.0, 0.0]);
            pr_bot.uvs.push([m(p[0]), m(p[1])]);
        }
        for t in &tris {
            pr_bot
                .indices
                .extend_from_slice(&[b2 + t[0], b2 + t[2], b2 + t[1]]);
        }
        // edge skirt
        for i in 0..outer.len() {
            let (p, q) = (outer[i], outer[(i + 1) % outer.len()]);
            let (nx, nz) = edge_normal(p, q);
            b.quad(
                Mat::Stone,
                [m(p[0]), m(y_top), m(p[1])],
                [m(q[0]), m(y_top), m(q[1])],
                [m(q[0]), m(y_top - thick), m(q[1])],
                [m(p[0]), m(y_top - thick), m(p[1])],
                [nx as f32, 0.0, nz as f32],
                &[[0.0; 2]; 4],
                false,
            );
        }
        if let Some(h) = hole {
            for i in 0..h.len() {
                let (p, q) = (h[(i + 1) % h.len()], h[i]);
                let (nx, nz) = edge_normal(p, q);
                b.quad(
                    Mat::Stone,
                    [m(p[0]), m(y_top), m(p[1])],
                    [m(q[0]), m(y_top), m(q[1])],
                    [m(q[0]), m(y_top - thick), m(q[1])],
                    [m(p[0]), m(y_top - thick), m(p[1])],
                    [nx as f32, 0.0, nz as f32],
                    &[[0.0; 2]; 4],
                    false,
                );
            }
        }
    }
    for &(x0, z0, x1, z1) in box_rects {
        colliders.push(BoxCollider {
            center: [
                m(x0 + x1) / 2.0,
                m(y_top) - thick as f32 / 2000.0,
                m(z0 + z1) / 2.0,
            ],
            half: [
                (x1 - x0) as f32 / 2000.0,
                thick as f32 / 2000.0,
                (z1 - z0) as f32 / 2000.0,
            ],
        });
    }
}

/// Decompose plan into axis-aligned rects for colliders.
fn plan_rects(outer: &[P2], hole: Option<&Vec<P2>>, spec: &HouseSpec) -> Vec<(i64, i64, i64, i64)> {
    let (minx, maxx) = (
        outer.iter().map(|p| p[0]).min().unwrap(),
        outer.iter().map(|p| p[0]).max().unwrap(),
    );
    let (minz, maxz) = (
        outer.iter().map(|p| p[1]).min().unwrap(),
        outer.iter().map(|p| p[1]).max().unwrap(),
    );
    match spec.footprint {
        Footprint::Rect { .. } => vec![(minx, minz, maxx, maxz)],
        Footprint::L { cut_w, cut_d, .. } => {
            // Cut at +X/+Z: full-depth strip on -X side + remainder.
            let cw = mm(cut_w);
            let cd = mm(cut_d);
            vec![
                (minx, minz, maxx - cw, maxz),
                (maxx - cw, minz, maxx, maxz - cd),
            ]
        }
        Footprint::Courtyard { .. } => {
            let Some(h) = hole else {
                return vec![(minx, minz, maxx, maxz)];
            };
            let (cx0, cx1) = (
                h.iter().map(|p| p[0]).min().unwrap(),
                h.iter().map(|p| p[0]).max().unwrap(),
            );
            let (cz0, cz1) = (
                h.iter().map(|p| p[1]).min().unwrap(),
                h.iter().map(|p| p[1]).max().unwrap(),
            );
            vec![
                (minx, minz, maxx, cz0),
                (minx, cz1, maxx, maxz),
                (minx, cz0, cx0, cz1),
                (cx1, cz0, maxx, cz1),
            ]
        }
    }
}

pub fn generate(spec: &HouseSpec, seed: u64) -> HouseMesh {
    let mut rng = mb_seed::Rng::new(seed);
    let mut infos: Vec<OpeningInfo> = Vec::new();
    let t = spec.wall_thickness_m;
    let floors = spec.floors;
    let fh = spec.floor_height_m;
    let plinth = spec.plinth_m;
    let front = side_dir(spec.front);
    let pitch = 3.0 * (1.0 + rng.range_f64(-0.1, 0.1));
    let kw = rng.range_f64(0.35, 0.5);
    let pv = rng.range_f64(-0.04, 0.04);
    let plaster_rgb = [
        (0.90 * (1.0 + pv)) as f32,
        (0.84 * (1.0 + pv)) as f32,
        (0.72 * (1.0 + pv)) as f32,
    ];
    let (outer, court) = plan(spec);
    let inner = inward_offset(&outer, mm(t));
    let court_out = court.as_ref().map(|c| inward_offset(c, mm(-t)));

    let mut lod = [Bld::new(), Bld::new(), Bld::new()];
    let mut colliders: Vec<BoxCollider> = Vec::new();

    for s in 0..floors {
        let y0 = mm(plinth + s as f64 * fh);
        let h = mm(fh);
        let pieces = storey_pieces(
            &outer, &inner, front, s, &mut rng, pitch, kw, spec, &mut infos,
        );
        for (i, pc) in pieces.iter().enumerate() {
            let nrm3 = [pc.nrm.0 as f32, 0.0, pc.nrm.1 as f32];
            let holes: Vec<Vec<P2>> = pc.openings.iter().map(|(o, _)| o.clone()).collect();
            // LOD0 outer + inner faces with holes
            wall_face(
                &mut lod[0],
                pc.a,
                pc.dir,
                y0,
                h,
                pc.len,
                &holes,
                nrm3,
                Mat::Plaster,
            );
            wall_face(
                &mut lod[0],
                pc.ia,
                pc.dir,
                y0,
                h,
                pc.len,
                &holes,
                [-nrm3[0], 0.0, -nrm3[2]],
                Mat::Plaster,
            );
            // reveals per opening
            for (o, _k) in &pc.openings {
                reveals(&mut lod[0], o, pc.a, pc.dir, y0, pc.nrm, t);
            }
            // top cap on top storey
            if s == floors - 1 {
                lod[0].quad(
                    Mat::Stone,
                    [m(pc.a[0]), m(y0) + m(h), m(pc.a[1])],
                    [m(pc.b[0]), m(y0) + m(h), m(pc.b[1])],
                    [m(pc.ib[0]), m(y0) + m(h), m(pc.ib[1])],
                    [m(pc.ia[0]), m(y0) + m(h), m(pc.ia[1])],
                    [0.0, 1.0, 0.0],
                    &[[0.0; 2]; 4],
                    false,
                );
            }
            // LOD1: solid faces + dark inset opening quads
            wall_face(
                &mut lod[1],
                pc.a,
                pc.dir,
                y0,
                h,
                pc.len,
                &[],
                nrm3,
                Mat::Plaster,
            );
            wall_face(
                &mut lod[1],
                pc.ia,
                pc.dir,
                y0,
                h,
                pc.len,
                &[],
                [-nrm3[0], 0.0, -nrm3[2]],
                Mat::Plaster,
            );
            for (o, _k) in &pc.openings {
                if let Ok(tris) = mb_geom::triangulate_with_holes(o, &[]) {
                    let pr = &mut lod[1].prims[Mat::Opening as usize];
                    let base = pr.positions.len() as u32;
                    for p in o {
                        pr.positions.push([
                            m(pc.a[0]) + pc.dir.0 as f32 * m(p[0]) + pc.nrm.0 as f32 * 0.01,
                            m(y0) + m(p[1]),
                            m(pc.a[1]) + pc.dir.1 as f32 * m(p[0]) + pc.nrm.1 as f32 * 0.01,
                        ]);
                        pr.normals.push(nrm3);
                        pr.uvs.push([m(p[0]), m(y0) + m(p[1])]);
                    }
                    for tr in &tris {
                        pr.indices
                            .extend_from_slice(&[base + tr[0], base + tr[1], base + tr[2]]);
                    }
                }
            }
            // collider box for this piece
            let cx = m(pc.a[0] + pc.b[0]) / 2.0;
            let cz = m(pc.a[1] + pc.b[1]) / 2.0;
            let icx = m(pc.ia[0] + pc.ib[0]) / 2.0;
            let icz = m(pc.ia[1] + pc.ib[1]) / 2.0;
            let door = pc.openings.iter().find(|(_, k)| *k == "door");
            if let Some((o, _)) = door {
                // split box around the door span
                let u0 = o[0][0];
                let u1 = o[1][0];
                for (s0, s1, vy0, vh) in [
                    (0i64, u0, 0i64, h),
                    (u1, pc.len, 0i64, h),
                    (u0, u1, mm(2.6), h - mm(2.6)),
                ] {
                    if s1 - s0 <= 0 || vh <= 0 {
                        continue;
                    }
                    let mid = m(s0 + s1) / 2.0;
                    let hl = m(s1 - s0) / 2.0;
                    colliders.push(BoxCollider {
                        center: [
                            cx + pc.dir.0 as f32 * (mid - m(pc.len) / 2.0) + (icx - cx) / 2.0,
                            m(y0) + m(vy0) + vh as f32 / 2000.0,
                            cz + pc.dir.1 as f32 * (mid - m(pc.len) / 2.0) + (icz - cz) / 2.0,
                        ],
                        half: [
                            (hl * pc.dir.0.abs() as f32 + t as f32 / 2.0 * pc.dir.1.abs() as f32)
                                .max(
                                    t as f32 / 2.0 * pc.dir.0.abs() as f32
                                        + hl * pc.dir.1.abs() as f32,
                                ),
                            vh as f32 / 2000.0,
                            (hl * pc.dir.1.abs() as f32 + t as f32 / 2.0 * pc.dir.0.abs() as f32)
                                .max(
                                    t as f32 / 2.0 * pc.dir.1.abs() as f32
                                        + hl * pc.dir.0.abs() as f32,
                                ),
                        ],
                    });
                }
            } else {
                colliders.push(BoxCollider {
                    center: [(cx + icx) / 2.0, m(y0) + fh as f32 / 2.0, (cz + icz) / 2.0],
                    half: [
                        (m(pc.len) / 2.0 * pc.dir.0.abs() as f32
                            + t as f32 / 2.0 * pc.dir.1.abs() as f32)
                            .max(
                                t as f32 / 2.0 * pc.dir.0.abs() as f32
                                    + m(pc.len) / 2.0 * pc.dir.1.abs() as f32,
                            ),
                        fh as f32 / 2.0,
                        (m(pc.len) / 2.0 * pc.dir.1.abs() as f32
                            + t as f32 / 2.0 * pc.dir.0.abs() as f32)
                            .max(
                                t as f32 / 2.0 * pc.dir.1.abs() as f32
                                    + m(pc.len) / 2.0 * pc.dir.0.abs() as f32,
                            ),
                    ],
                });
            }
            let _ = i;
        }
        // Courtyard walls: `co` (room side) is the outer edge of each
        // piece, `c` (court side) the inner one.
        if court.is_some() {
            if let (Some(c), Some(co)) = (&court, &court_out) {
                let pieces = storey_pieces(co, c, front, s, &mut rng, pitch, kw, spec, &mut infos);
                for pc in &pieces {
                    // pc.nrm points away from the court, into the rooms; the face on
                    // `co` faces the rooms, the face on `c` faces the court.
                    let nrm3 = [pc.nrm.0 as f32, 0.0, pc.nrm.1 as f32];
                    let holes: Vec<Vec<P2>> = pc.openings.iter().map(|(o, _)| o.clone()).collect();
                    wall_face(
                        &mut lod[0],
                        pc.a,
                        pc.dir,
                        y0,
                        h,
                        pc.len,
                        &holes,
                        nrm3,
                        Mat::Plaster,
                    );
                    wall_face(
                        &mut lod[0],
                        pc.ia,
                        pc.dir,
                        y0,
                        h,
                        pc.len,
                        &holes,
                        [-nrm3[0], 0.0, -nrm3[2]],
                        Mat::Plaster,
                    );
                    for (o, _) in &pc.openings {
                        reveals(&mut lod[0], o, pc.a, pc.dir, y0, pc.nrm, t);
                    }
                    wall_face(
                        &mut lod[1],
                        pc.a,
                        pc.dir,
                        y0,
                        h,
                        pc.len,
                        &[],
                        nrm3,
                        Mat::Plaster,
                    );
                    wall_face(
                        &mut lod[1],
                        pc.ia,
                        pc.dir,
                        y0,
                        h,
                        pc.len,
                        &[],
                        [-nrm3[0], 0.0, -nrm3[2]],
                        Mat::Plaster,
                    );
                    colliders.push(BoxCollider {
                        center: [
                            (m(pc.a[0] + pc.b[0]) / 2.0 + m(pc.ia[0] + pc.ib[0]) / 2.0) / 2.0,
                            m(y0) + fh as f32 / 2.0,
                            (m(pc.a[1] + pc.b[1]) / 2.0 + m(pc.ia[1] + pc.ib[1]) / 2.0) / 2.0,
                        ],
                        half: [
                            (m(pc.len) / 2.0 * pc.dir.0.abs() as f32
                                + t as f32 / 2.0 * pc.dir.1.abs() as f32)
                                .max(
                                    t as f32 / 2.0 * pc.dir.0.abs() as f32
                                        + m(pc.len) / 2.0 * pc.dir.1.abs() as f32,
                                ),
                            fh as f32 / 2.0,
                            (m(pc.len) / 2.0 * pc.dir.1.abs() as f32
                                + t as f32 / 2.0 * pc.dir.0.abs() as f32)
                                .max(
                                    t as f32 / 2.0 * pc.dir.1.abs() as f32
                                        + m(pc.len) / 2.0 * pc.dir.0.abs() as f32,
                                ),
                        ],
                    });
                }
            }
        }
        // slab at storey floor (s>0) — ground floor is plinth top
        if s > 0 {
            slab(
                &mut lod[0],
                &outer,
                court.as_ref(),
                y0,
                mm(0.2),
                Mat::Floor,
                &mut colliders,
                &plan_rects(&outer, court.as_ref(), spec),
            );
            slab(
                &mut lod[1],
                &outer,
                court.as_ref(),
                y0,
                mm(0.2),
                Mat::Floor,
                &mut Vec::new(),
                &[],
            );
        }
        // chajja at storey top
        let ytop = mm(plinth + (s + 1) as f64 * fh);
        let outw = inward_offset(&outer, mm(-0.6));
        for i in 0..outer.len() {
            let (a, b_) = (outer[i], outer[(i + 1) % outer.len()]);
            let (oa, ob) = (outw[i], outw[(i + 1) % outw.len()]);
            let (nx, nz) = edge_normal(a, b_);
            for l in 0..2 {
                lod[l].quad(
                    Mat::Stone,
                    [m(a[0]), m(ytop), m(a[1])],
                    [m(b_[0]), m(ytop), m(b_[1])],
                    [m(ob[0]), m(ytop), m(ob[1])],
                    [m(oa[0]), m(ytop), m(oa[1])],
                    [0.0, 1.0, 0.0],
                    &[[0.0; 2]; 4],
                    false,
                );
                lod[l].quad(
                    Mat::Stone,
                    [m(oa[0]), m(ytop - mm(0.12)), m(oa[1])],
                    [m(ob[0]), m(ytop - mm(0.12)), m(ob[1])],
                    [m(b_[0]), m(ytop - mm(0.12)), m(b_[1])],
                    [m(a[0]), m(ytop - mm(0.12)), m(a[1])],
                    [0.0, -1.0, 0.0],
                    &[[0.0; 2]; 4],
                    false,
                );
                lod[l].quad(
                    Mat::Stone,
                    [m(oa[0]), m(ytop), m(oa[1])],
                    [m(ob[0]), m(ytop), m(ob[1])],
                    [m(ob[0]), m(ytop - mm(0.12)), m(ob[1])],
                    [m(oa[0]), m(ytop - mm(0.12)), m(oa[1])],
                    [nx as f32, 0.0, nz as f32],
                    &[[0.0; 2]; 4],
                    false,
                );
            }
        }
        // courtyard-side chajja
        if spec.complexity >= 1 {
            if let Some(c) = &court {
                let cin = inward_offset(c, mm(0.6)); // into the court
                for i in 0..c.len() {
                    let (a, b_) = (c[i], c[(i + 1) % c.len()]);
                    let (oa, ob) = (cin[i], cin[(i + 1) % cin.len()]);
                    for l in 0..2 {
                        lod[l].quad(
                            Mat::Stone,
                            [m(a[0]), m(ytop), m(a[1])],
                            [m(b_[0]), m(ytop), m(b_[1])],
                            [m(ob[0]), m(ytop), m(ob[1])],
                            [m(oa[0]), m(ytop), m(oa[1])],
                            [0.0, 1.0, 0.0],
                            &[[0.0; 2]; 4],
                            false,
                        );
                        lod[l].quad(
                            Mat::Stone,
                            [m(oa[0]), m(ytop - mm(0.12)), m(oa[1])],
                            [m(ob[0]), m(ytop - mm(0.12)), m(ob[1])],
                            [m(b_[0]), m(ytop - mm(0.12)), m(b_[1])],
                            [m(a[0]), m(ytop - mm(0.12)), m(a[1])],
                            [0.0, -1.0, 0.0],
                            &[[0.0; 2]; 4],
                            false,
                        );
                    }
                }
            }
        }
    }

    // roof slab at top
    let yr = mm(plinth + floors as f64 * fh);
    slab(
        &mut lod[0],
        &outer,
        court.as_ref(),
        yr,
        mm(0.2),
        Mat::Roof,
        &mut colliders,
        &plan_rects(&outer, court.as_ref(), spec),
    );
    slab(
        &mut lod[1],
        &outer,
        court.as_ref(),
        yr,
        mm(0.2),
        Mat::Roof,
        &mut Vec::new(),
        &[],
    );

    // plinth: expanded prism 0..plinth + steps at the door
    let plx = inward_offset(&outer, mm(-0.4));
    for i in 0..plx.len() {
        let (p, q) = (plx[i], plx[(i + 1) % plx.len()]);
        let (nx, nz) = edge_normal(p, q);
        for l in 0..2 {
            lod[l].quad(
                Mat::Stone,
                [m(p[0]), m(mm(plinth)), m(p[1])],
                [m(q[0]), m(mm(plinth)), m(q[1])],
                [m(q[0]), 0.0, m(q[1])],
                [m(p[0]), 0.0, m(p[1])],
                [nx as f32, 0.0, nz as f32],
                &[[0.0; 2]; 4],
                false,
            );
        }
    }
    // plinth top
    if let Ok(tris) = mb_geom::triangulate_with_holes(&plx, &[]) {
        for l in 0..2 {
            let pr = &mut lod[l].prims[Mat::Stone as usize];
            let base = pr.positions.len() as u32;
            for p in &plx {
                pr.positions.push([m(p[0]), m(mm(plinth)), m(p[1])]);
                pr.normals.push([0.0, 1.0, 0.0]);
                pr.uvs.push([m(p[0]), m(p[1])]);
            }
            for t2 in &tris {
                pr.indices
                    .extend_from_slice(&[base + t2[0], base + t2[1], base + t2[2]]);
            }
        }
    }
    colliders.extend(
        plan_rects(&plx, None, spec)
            .iter()
            .map(|&(x0, z0, x1, z1)| BoxCollider {
                center: [m(x0 + x1) / 2.0, plinth as f32 / 2.0, m(z0 + z1) / 2.0],
                half: [
                    (x1 - x0) as f32 / 2000.0,
                    plinth as f32 / 2.0,
                    (z1 - z0) as f32 / 2000.0,
                ],
            }),
    );
    // Steps at the main door: two treads stepping down outward from the
    // plinth edge (0.4 m beyond the wall face), each 0.6 m deep.
    if let Some(di) = infos
        .iter()
        .find(|o| o.kind == "door" && o.piece < outer.len())
    {
        let (a, b) = (outer[di.piece], outer[(di.piece + 1) % outer.len()]);
        let (ex, ez) = ((b[0] - a[0]) as f64, (b[1] - a[1]) as f64);
        let len = (ex * ex + ez * ez).sqrt();
        let dir = ((b[0] - a[0]) as f64 / len, (b[1] - a[1]) as f64 / len);
        let nrm = edge_normal(a, b);
        let um = (di.u0 + di.u1) / 2.0;
        let (px, pz) = (m(a[0]) as f64 + dir.0 * um, m(a[1]) as f64 + dir.1 * um);
        let half_w = (di.u1 - di.u0) / 2.0 + 0.3;
        let at = |t: f64, d: f64, y: f64| -> [f32; 3] {
            [
                (px + dir.0 * t + nrm.0 * d) as f32,
                y as f32,
                (pz + dir.1 * t + nrm.1 * d) as f32,
            ]
        };
        for k in 0..2 {
            let (d0, d1) = (0.4 + 0.6 * k as f64, 1.0 + 0.6 * k as f64);
            let h = plinth * (2 - k) as f64 / 3.0;
            let top = [
                at(-half_w, d0, h),
                at(half_w, d0, h),
                at(half_w, d1, h),
                at(-half_w, d1, h),
            ];
            lod[0].quad_facing(Mat::Stone, top, [0.0, 1.0, 0.0]);
            let riser = [
                at(-half_w, d1, 0.0),
                at(half_w, d1, 0.0),
                at(half_w, d1, h),
                at(-half_w, d1, h),
            ];
            lod[0].quad_facing(Mat::Stone, riser, [nrm.0 as f32, 0.0, nrm.1 as f32]);
            let (lo, hi) = (at(-half_w, d0, 0.0), at(half_w, d1, h));
            let c = |i: usize| (lo[i] + hi[i]) / 2.0;
            let hh = |i: usize| (hi[i] - lo[i]).abs() / 2.0;
            colliders.push(BoxCollider {
                center: [c(0), c(1), c(2)],
                half: [hh(0), hh(1), hh(2)],
            });
        }
    }

    // parapet
    let par = match spec.roof {
        mb_schema::Roof::Flat { parapet_m } => parapet_m,
    };
    let parapet_rings: Vec<(&Vec<P2>, &Vec<P2>)> = {
        let mut v = vec![(&outer, &inner)];
        if let (Some(c), Some(co)) = (&court, &court_out) {
            v.push((co, c)); // court walls face inward
        }
        v
    };
    let pt_mm = mm(0.25);
    for (o_ring, i_ring) in parapet_rings {
        let pin = inward_offset(o_ring, pt_mm);
        let _ = i_ring;
        for i in 0..o_ring.len() {
            let (a, b) = (o_ring[i], o_ring[(i + 1) % o_ring.len()]);
            let (ia, ib) = (pin[i], pin[(i + 1) % pin.len()]);
            let len = ((b[0] - a[0]) as f64).hypot((b[1] - a[1]) as f64) as i64;
            let (nx, nz) = edge_normal(a, b);
            let ph = mm(par);
            if spec.complexity == 2 && len > mm(1.2) {
                // castellated outer face: rectilinear merlon outline
                let mut poly: Vec<P2> = vec![[0, 0], [len, 0]];
                let nmer = ((len as f64) / 600.0).floor() as i64;
                let pitchm = len / nmer.max(1);
                let mut x = len;
                for _ in 0..nmer {
                    poly.push([x, ph + mm(0.15)]);
                    x -= pitchm / 2;
                    poly.push([x, ph + mm(0.15)]);
                    poly.push([x, ph]);
                    x -= pitchm / 2;
                    poly.push([x, ph]);
                }
                poly.push([0, ph]);
                let dir = (
                    (b[0] - a[0]) as f64 / len as f64,
                    (b[1] - a[1]) as f64 / len as f64,
                );
                wall_face(
                    &mut lod[0],
                    a,
                    dir,
                    yr,
                    mm(par) + mm(0.15),
                    len,
                    &[],
                    [nx as f32, 0.0, nz as f32],
                    Mat::Stone,
                );
                // custom castellated polygon face
                if let Ok(tris) = mb_geom::triangulate_with_holes(&poly, &[]) {
                    let pr = &mut lod[0].prims[Mat::Stone as usize];
                    let base = pr.positions.len() as u32;
                    for p in &poly {
                        pr.positions.push([
                            m(a[0]) + dir.0 as f32 * m(p[0]),
                            m(yr) + m(p[1]),
                            m(a[1]) + dir.1 as f32 * m(p[0]),
                        ]);
                        pr.normals.push([nx as f32, 0.0, nz as f32]);
                        pr.uvs.push([m(p[0]), m(yr) + m(p[1])]);
                    }
                    for t2 in &tris {
                        pr.indices
                            .extend_from_slice(&[base + t2[0], base + t2[1], base + t2[2]]);
                    }
                }
            } else {
                let dir = (
                    (b[0] - a[0]) as f64 / len as f64,
                    (b[1] - a[1]) as f64 / len as f64,
                );
                for l in 0..2 {
                    wall_face(
                        &mut lod[l],
                        a,
                        dir,
                        yr,
                        ph,
                        len,
                        &[],
                        [nx as f32, 0.0, nz as f32],
                        Mat::Stone,
                    );
                }
            }
            // inner face + top cap
            let idir = (
                (ib[0] - ia[0]) as f64 / len as f64,
                (ib[1] - ia[1]) as f64 / len as f64,
            );
            let _ = idir;
            for l in 0..2 {
                lod[l].quad(
                    Mat::Stone,
                    [m(a[0]), m(yr + ph), m(a[1])],
                    [m(b[0]), m(yr + ph), m(b[1])],
                    [m(ib[0]), m(yr + ph), m(ib[1])],
                    [m(ia[0]), m(yr + ph), m(ia[1])],
                    [0.0, 1.0, 0.0],
                    &[[0.0; 2]; 4],
                    false,
                );
            }
        }
    }

    // LOD2: footprint prism + parapet ring (court keeps its hole)
    {
        let b2 = &mut lod[2];
        for i in 0..outer.len() {
            let (p, q) = (outer[i], outer[(i + 1) % outer.len()]);
            let (nx, nz) = edge_normal(p, q);
            b2.quad(
                Mat::Plaster,
                [m(p[0]), 0.0, m(p[1])],
                [m(q[0]), 0.0, m(q[1])],
                [m(q[0]), m(yr + mm(par)), m(q[1])],
                [m(p[0]), m(yr + mm(par)), m(p[1])],
                [nx as f32, 0.0, nz as f32],
                &[[0.0; 2]; 4],
                false,
            );
        }
        if let Some(c) = &court {
            for i in 0..c.len() {
                let (p, q) = (c[(i + 1) % c.len()], c[i]);
                let (nx, nz) = edge_normal(p, q);
                b2.quad(
                    Mat::Plaster,
                    [m(p[0]), 0.0, m(p[1])],
                    [m(q[0]), 0.0, m(q[1])],
                    [m(q[0]), m(yr + mm(par)), m(q[1])],
                    [m(p[0]), m(yr + mm(par)), m(p[1])],
                    [nx as f32, 0.0, nz as f32],
                    &[[0.0; 2]; 4],
                    false,
                );
            }
            // roof top with hole
            let hole2 = vec![c.clone()];
            if let Ok(tris) = mb_geom::triangulate_with_holes(&outer, &hole2) {
                let pr = &mut b2.prims[Mat::Roof as usize];
                let base = pr.positions.len() as u32;
                for p in outer.iter().chain(c.iter()) {
                    pr.positions.push([m(p[0]), m(yr + mm(par)), m(p[1])]);
                    pr.normals.push([0.0, 1.0, 0.0]);
                    pr.uvs.push([m(p[0]), m(p[1])]);
                }
                for t2 in &tris {
                    pr.indices
                        .extend_from_slice(&[base + t2[0], base + t2[1], base + t2[2]]);
                }
            }
        } else if let Ok(tris) = mb_geom::triangulate_with_holes(&outer, &[]) {
            let pr = &mut b2.prims[Mat::Roof as usize];
            let base = pr.positions.len() as u32;
            for p in &outer {
                pr.positions.push([m(p[0]), m(yr + mm(par)), m(p[1])]);
                pr.normals.push([0.0, 1.0, 0.0]);
                pr.uvs.push([m(p[0]), m(p[1])]);
            }
            for t2 in &tris {
                pr.indices
                    .extend_from_slice(&[base + t2[0], base + t2[1], base + t2[2]]);
            }
        }
    }

    // bounds
    let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
    for l in &lod {
        for pr in &l.prims {
            for p in &pr.positions {
                for k in 0..3 {
                    mn[k] = mn[k].min(p[k]);
                    mx[k] = mx[k].max(p[k]);
                }
            }
        }
    }

    // ---- part kit placement (step 2) ----
    let mut rng2 = mb_seed::Rng::new(seed ^ 0x9e3779b97f4a7c15u64);
    // The longest outer piece facing the front carries the door and the
    // jharokhas (pieces are indexed like the outer polygon's edges).
    let mut fedge = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut fpiece = usize::MAX;
    let mut flen = -1.0f64;
    for i in 0..outer.len() {
        let (a, b) = (outer[i], outer[(i + 1) % outer.len()]);
        let (nx, nz) = edge_normal(a, b);
        if (nx - front.0).abs() < 0.1 && (nz - front.1).abs() < 0.1 {
            let l = ((b[0] - a[0]).abs() + (b[1] - a[1]).abs()) as f64;
            if l > flen {
                flen = l;
                fpiece = i;
                fedge = (
                    a[0] as f64 / MM,
                    a[1] as f64 / MM,
                    (b[0] - a[0]) as f64 / l,
                    (b[1] - a[1]) as f64 / l,
                    l / MM,
                );
            }
        }
    }
    let dooru = infos
        .iter()
        .find(|o| o.kind == "door")
        .map(|o| (o.u0 + o.u1) / 2.0);
    let door_plan = dooru.map(|cx| (cx, 1.4, front.0, front.1));
    // One jharokha per upper storey: the window nearest the piece centre.
    let mut fwin: Vec<(u32, f64, f64)> = Vec::new();
    for st in 1..floors {
        let best = infos
            .iter()
            .filter(|o| o.kind == "window" && o.storey == st && o.piece == fpiece)
            .min_by(|a, b| {
                let c = fedge.4 / 2.0;
                ((a.u0 + a.u1) / 2.0 - c)
                    .abs()
                    .total_cmp(&((b.u0 + b.u1) / 2.0 - c).abs())
            });
        if let Some(o) = best {
            fwin.push((o.storey, o.u0, o.u1));
        }
    }
    let mut top_y: Vec<f64> = (0..floors).map(|s| plinth + s as f64 * fh).collect();
    top_y.push(plinth + floors as f64 * fh);
    let (mut inst, mut pcols) =
        parts::place_parts(spec, &mut rng2, door_plan, Some(fedge), &top_y, &fwin);
    for pi in &mut inst {
        if pi.kind == parts::PartKind::DoorLeaf {
            if let Some(cx) = dooru {
                let px = fedge.0 + fedge.2 * cx - front.0 * 0.10;
                let pz = fedge.1 + fedge.3 * cx - front.1 * 0.10;
                let ry = match (front.0 as i64, front.1 as i64) {
                    (0, -1) => 2u8,
                    (1, 0) => 3,
                    (0, 1) => 0,
                    _ => 1,
                };
                pi.transform = parts::place_transform(px as f32, plinth as f32, pz as f32, ry);
            }
        }
    }
    colliders.append(&mut pcols);
    let instances = inst;
    let prims = |b: Bld| -> Vec<Prim> {
        b.prims
            .into_iter()
            .filter(|p| !p.indices.is_empty())
            .map(orient_to_normals)
            .collect()
    };
    let [b0, b1, b2] = lod;
    HouseMesh {
        lods: [
            Lod { prims: prims(b0) },
            Lod { prims: prims(b1) },
            Lod { prims: prims(b2) },
        ],
        instances,
        colliders,
        plaster_rgb,
        bounds: (mn, mx),
    }
}

/// Every face carries an explicit flat normal; wind each triangle so its
/// front face (counter-clockwise) agrees with that normal. Engines cull
/// back faces, so a wrongly wound triangle would be invisible.
pub(crate) fn orient_to_normals(mut p: Prim) -> Prim {
    for t in p.indices.chunks_mut(3) {
        let [a, b, c] = [0, 1, 2].map(|k| p.positions[t[k] as usize]);
        let (e1, e2) = (
            [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
            [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
        );
        let n = p.normals[t[0] as usize];
        let dot = (e1[1] * e2[2] - e1[2] * e2[1]) * n[0]
            + (e1[2] * e2[0] - e1[0] * e2[2]) * n[1]
            + (e1[0] * e2[1] - e1[1] * e2[0]) * n[2];
        if dot < 0.0 {
            t.swap(1, 2);
        }
    }
    p
}

/// Debug: openings with their piece/u-v spans (for tests).
pub fn openings(spec: &HouseSpec, seed: u64) -> Vec<OpeningInfo> {
    let mut rng = mb_seed::Rng::new(seed);
    let pitch = 3.0 * (1.0 + rng.range_f64(-0.1, 0.1));
    let kw = rng.range_f64(0.35, 0.5);
    let _ = rng.range_f64(-0.04, 0.04);
    let mut infos = Vec::new();
    let (outer, court) = plan(spec);
    let inner = inward_offset(&outer, mm(spec.wall_thickness_m));
    let court_out = court
        .as_ref()
        .map(|c| inward_offset(c, mm(-spec.wall_thickness_m)));
    let front = side_dir(spec.front);
    for s in 0..spec.floors {
        let _ = storey_pieces(
            &outer, &inner, front, s, &mut rng, pitch, kw, spec, &mut infos,
        );
        if spec.complexity >= 1 {
            if let (Some(c), Some(co)) = (&court, &court_out) {
                let _ = storey_pieces(co, c, front, s, &mut rng, pitch, kw, spec, &mut infos);
            }
        }
    }
    infos
}

#[cfg(test)]
mod arch_tests {
    use super::*;

    fn area2(p: &[P2]) -> i128 {
        (0..p.len())
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                a[0] as i128 * b[1] as i128 - b[0] as i128 * a[1] as i128
            })
            .sum()
    }

    #[test]
    fn arch_outline_is_simple_and_inside_the_opening() {
        for seg in [2usize, 4] {
            for (w, h) in [
                (1400i64, 2600i64),
                (800, 1500),
                (600, 1500),
                (1700, 1500),
                (2400, 1500),
            ] {
                let (u0, v0) = (5000, 900);
                let p = opening_poly(u0, v0, w, h, true, seg);
                assert!(area2(&p) > 0, "outline must be CCW");
                for q in &p {
                    assert!(q[0] >= u0 && q[0] <= u0 + w, "x outside opening: {q:?}");
                    assert!(q[1] >= v0 && q[1] <= v0 + h + 1, "y outside opening: {q:?}");
                }
                // Apex at the top centre; arch sides symmetric.
                let top = p.iter().max_by_key(|q| q[1]).unwrap();
                assert!((top[0] - (u0 + w / 2)).abs() <= 1 && (top[1] - (v0 + h)).abs() <= 1);
                let n = p.len() as u32;
                let tris = mb_geom::triangulate_with_holes(&p, &[]).unwrap();
                assert_eq!(tris.len() as u32, n - 2, "outline must be simple");
            }
        }
    }
}
