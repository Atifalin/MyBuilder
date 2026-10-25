//! Phase 3 step 2: instanced part kit (DoorLeaf, Chhatri, Jharokha,
//! Bracket) with content-addressed meshes.
use crate::{Bld, Lod, Mat, Prim};
use mb_schema::{Footprint, HouseSpec};
use mb_seed::Rng;

pub const PART_GENERATOR_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    DoorLeaf,
    Chhatri,
    Jharokha,
    Bracket,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PartInstance {
    pub key: String,
    pub kind: PartKind,
    /// Bucketed params the key was computed from (parts stage rebuild).
    pub params: String,
    /// 3x4 column-major affine, house-local.
    pub transform: [f32; 12],
    /// bitmask: 1 = LOD0, 2 = LOD1.
    pub lods: u8,
}

/// Bucketed params → content key.
pub fn part_key(kind: PartKind, params: &str) -> String {
    let mut b = Vec::new();
    b.push(kind as u8);
    b.extend_from_slice(params.as_bytes());
    b.extend_from_slice(&PART_GENERATOR_VERSION.to_le_bytes());
    hex16(&mb_seed::hash_bytes(&b))
}
fn hex16(b: &[u8; 32]) -> String {
    b[..8].iter().map(|x| format!("{x:02x}")).collect()
}

fn col_major(px: f32, py: f32, pz: f32, ry90: u8) -> [f32; 12] {
    // rotation about Y by ry90*90deg then translation (columns: X,Y,Z,T)
    let (c, s): (f32, f32) = match ry90 % 4 {
        0 => (1.0, 0.0),
        1 => (0.0, 1.0),
        2 => (-1.0, 0.0),
        _ => (0.0, -1.0),
    };
    [c, 0.0, -s, 0.0, 1.0, 0.0, s, 0.0, c, px, py, pz]
}

fn boxp(
    b: &mut Bld,
    mat: Mat,
    cx: f32,
    cy: f32,
    cz: f32,
    hx: f32,
    hy: f32,
    hz: f32,
    uv: &[[f32; 2]; 4],
) {
    let (x0, x1, y0, y1, z0, z1) = (cx - hx, cx + hx, cy - hy, cy + hy, cz - hz, cz + hz);
    let quads: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [0.0, 0.0, 1.0],
            [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
        ),
        (
            [0.0, 0.0, -1.0],
            [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]],
        ),
        (
            [1.0, 0.0, 0.0],
            [[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]],
        ),
        (
            [-1.0, 0.0, 0.0],
            [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]],
        ),
        (
            [0.0, 1.0, 0.0],
            [[x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [x0, y1, z0]],
        ),
        (
            [0.0, -1.0, 0.0],
            [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
        ),
    ];
    for (n, q) in quads {
        b.quad(mat, q[0], q[1], q[2], q[3], n, uv, false);
    }
}

// Unit-circle literals for domes (12-gon / 8-gon / 6-gon).
// Deliberate determinism literals (no runtime trig) — not approximations.
#[allow(clippy::approx_constant, clippy::excessive_precision)]
const C12: [f32; 13] = [
    1.0, 0.8660254, 0.5, 0.0, -0.5, -0.8660254, -1.0, -0.8660254, -0.5, 0.0, 0.5, 0.8660254, 1.0,
];
#[allow(clippy::approx_constant, clippy::excessive_precision)]
const S12: [f32; 13] = [
    0.0, 0.5, 0.8660254, 1.0, 0.8660254, 0.5, 0.0, -0.5, -0.8660254, -1.0, -0.8660254, -0.5, 0.0,
];
#[allow(clippy::approx_constant, clippy::excessive_precision)]
const C8: [f32; 9] = [
    1.0, 0.7071068, 0.0, -0.7071068, -1.0, -0.7071068, 0.0, 0.7071068, 1.0,
];
#[allow(clippy::approx_constant, clippy::excessive_precision)]
const S8: [f32; 9] = [
    0.0, 0.7071068, 1.0, 0.7071068, 0.0, -0.7071068, -1.0, -0.7071068, 0.0,
];
#[allow(clippy::approx_constant, clippy::excessive_precision)]
const C6: [f32; 7] = [1.0, 0.5, -0.5, -1.0, -0.5, 0.5, 1.0];
#[allow(clippy::approx_constant, clippy::excessive_precision)]
const S6: [f32; 7] = [0.0, 0.8660254, 0.8660254, 0.0, -0.8660254, -0.8660254, 0.0];

fn dome(b: &mut Bld, cx: f32, y: f32, cz: f32, r: f32, h: f32, seg: usize) {
    // profile rings (radius, dy) — pointed bulb profile; coarse = 3 rings
    let prof_coarse: [(f32, f32); 3] = [(1.0, 0.0), (0.62, 0.82), (0.0, 1.15)];
    let prof_full: [(f32, f32); 5] = [
        (1.0, 0.0),
        (1.02, 0.28),
        (0.92, 0.55),
        (0.62, 0.82),
        (0.0, 1.15),
    ];
    let prof: &[(f32, f32)] = if seg > 6 { &prof_full } else { &prof_coarse };
    let (cos, sin) = match seg {
        0..=6 => (&C6[..], &S6[..]),
        7..=8 => (&C8[..], &S8[..]),
        _ => (&C12[..], &S12[..]),
    };
    let n = cos.len() - 1;
    for j in 0..prof.len() - 1 {
        let (r0, d0) = prof[j];
        let (r1, d1) = prof[j + 1];
        let apex = r1 == 0.0;
        for i in 0..n {
            let (c0, s0) = (cos[i], sin[i]);
            let (c1, s1) = (cos[i + 1], sin[i + 1]);
            // surface normal of a cone band: radial*(h*dd) + y*(r*(r0-r1))
            let sh = h * (d1 - d0);
            let sy = r * (r0 - r1);
            let mid = [(c0 + c1) * sh, sy, (s0 + s1) * sh];
            let ll = (mid[0] * mid[0] + mid[2] * mid[2] + mid[1] * mid[1])
                .sqrt()
                .max(1e-6);
            let nn = [mid[0] / ll, mid[1] / ll, mid[2] / ll];
            if apex {
                b.poly(
                    Mat::Plaster,
                    &[
                        [cx + r * r0 * c0, y + h * d0, cz + r * r0 * s0],
                        [cx + r * r0 * c1, y + h * d0, cz + r * r0 * s1],
                        [cx, y + h * d1, cz],
                    ],
                    nn,
                    &[[0.0; 2]; 3],
                    false,
                );
            } else {
                b.quad(
                    Mat::Plaster,
                    [cx + r * r0 * c0, y + h * d0, cz + r * r0 * s0],
                    [cx + r * r0 * c1, y + h * d0, cz + r * r0 * s1],
                    [cx + r * r1 * c1, y + h * d1, cz + r * r1 * s1],
                    [cx + r * r1 * c0, y + h * d1, cz + r * r1 * s0],
                    nn,
                    &[[0.0; 2]; 4],
                    false,
                );
            }
        }
    }
    // finial (skipped at lod1)
    if seg > 6 {
        boxp(
            b,
            Mat::Stone,
            cx,
            y + h * 1.15 + 0.05,
            cz,
            0.03,
            0.08,
            0.03,
            &[[0.0; 2]; 4],
        );
    }
}

/// Build the LOD(s) for one part. `lod1` selects the coarse variant.
pub fn build_part(kind: PartKind, params: &str, lod1: bool) -> Lod {
    let mut b = Bld::new();
    match kind {
        PartKind::DoorLeaf => {
            // w=1.4 h=2.6 arched double door, 6cm thick; local frame:
            // centred on opening, y up from sill, -z into the wall.
            let (w, h) = (1.4f32, 2.6f32);
            let half = w / 2.0;
            // two leaves meeting at centre (lod1: one solid panel)
            if lod1 {
                boxp(
                    &mut b,
                    Mat::Wood,
                    0.0,
                    h / 2.0,
                    0.0,
                    half,
                    h / 2.0,
                    0.03,
                    &[[0.0; 2]; 4],
                );
            } else {
                boxp(
                    &mut b,
                    Mat::Wood,
                    -half / 2.0,
                    h / 2.0,
                    0.0,
                    half / 2.0,
                    h / 2.0,
                    0.03,
                    &[[0.0; 2]; 4],
                );
                boxp(
                    &mut b,
                    Mat::Wood,
                    half / 2.0,
                    h / 2.0,
                    0.0,
                    half / 2.0,
                    h / 2.0,
                    0.03,
                    &[[0.0; 2]; 4],
                );
                boxp(
                    &mut b,
                    Mat::Stone,
                    0.0,
                    h - 0.05,
                    0.0,
                    half + 0.1,
                    0.05,
                    0.05,
                    &[[0.0; 2]; 4],
                );
            }
        }
        PartKind::Chhatri => {
            // params "s<side>": side in {1.6,2.0,2.4}; h=1.8*side
            let (sd, cd) = params
                .strip_prefix('s')
                .unwrap_or("2.0")
                .split_once('c')
                .unwrap_or((params.strip_prefix('s').unwrap_or("2.0"), "1"));
            let side: f32 = sd.parse().unwrap_or(2.0);
            let _ = cd; // dome detail is the same at every complexity (budget)
            let seg_full = 8;
            let h = 1.8 * side;
            let hs = side / 2.0;
            boxp(
                &mut b,
                Mat::Stone,
                0.0,
                0.15,
                0.0,
                hs,
                0.15,
                hs,
                &[[0.0; 2]; 4],
            ); // plinth
            let inset = hs - 0.15;
            // Columns only on LOD0 (sub-pixel from LOD1 distances), side
            // faces only: their ends are hidden by the plinth and chajja.
            if !lod1 {
                for (sx, sz) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                    let (x, z, r) = (sx * inset, sz * inset, 0.06);
                    let (y0, y1) = (0.3, h * 0.9);
                    for (n, q) in [
                        (
                            [0.0, 0.0, 1.0],
                            [
                                [x - r, y0, z + r],
                                [x + r, y0, z + r],
                                [x + r, y1, z + r],
                                [x - r, y1, z + r],
                            ],
                        ),
                        (
                            [0.0, 0.0, -1.0],
                            [
                                [x - r, y0, z - r],
                                [x + r, y0, z - r],
                                [x + r, y1, z - r],
                                [x - r, y1, z - r],
                            ],
                        ),
                        (
                            [1.0, 0.0, 0.0],
                            [
                                [x + r, y0, z - r],
                                [x + r, y0, z + r],
                                [x + r, y1, z + r],
                                [x + r, y1, z - r],
                            ],
                        ),
                        (
                            [-1.0, 0.0, 0.0],
                            [
                                [x - r, y0, z - r],
                                [x - r, y0, z + r],
                                [x - r, y1, z + r],
                                [x - r, y1, z - r],
                            ],
                        ),
                    ] {
                        b.quad(Mat::Stone, q[0], q[1], q[2], q[3], n, &[[0.0; 2]; 4], false);
                    }
                }
            }
            boxp(
                &mut b,
                Mat::Stone,
                0.0,
                h * 0.92,
                0.0,
                hs + 0.15,
                0.06,
                hs + 0.15,
                &[[0.0; 2]; 4],
            ); // chajja
            let r = hs * 0.9;
            let seg = if lod1 { 6 } else { seg_full };
            dome(&mut b, 0.0, h * 0.95, 0.0, r, h * 0.55, seg);
        }
        PartKind::Jharokha => {
            // params "w<width>": {1.2,1.5,1.8}, projection 0.7, front sill 0.9
            let w: f32 = params
                .strip_prefix('w')
                .and_then(|x| x.parse().ok())
                .unwrap_or(1.5);
            let hw = w / 2.0;
            // corbelled base: 3 stepped slabs (lod0), 1 slab at lod1
            let steps: &[(usize, f32)] = if lod1 {
                &[(2, 0.70)]
            } else {
                &[(0, 0.20), (1, 0.45), (2, 0.70)]
            };
            for &(i, p) in steps {
                boxp(
                    &mut b,
                    Mat::Stone,
                    0.0,
                    -0.45 + i as f32 * 0.15,
                    p / 2.0,
                    hw - 0.1 * i as f32,
                    0.07,
                    p / 2.0,
                    &[[0.0; 2]; 4],
                );
            }
            // sill wall front
            boxp(
                &mut b,
                Mat::Plaster,
                0.0,
                0.45,
                0.65,
                hw,
                0.45,
                0.04,
                &[[0.0; 2]; 4],
            );
            // two slender columns at the front corners
            boxp(
                &mut b,
                Mat::Stone,
                -hw + 0.05,
                0.75,
                0.62,
                0.04,
                0.75,
                0.04,
                &[[0.0; 2]; 4],
            );
            boxp(
                &mut b,
                Mat::Stone,
                hw - 0.05,
                0.75,
                0.62,
                0.04,
                0.75,
                0.04,
                &[[0.0; 2]; 4],
            );
            // Bangla curved roof: arched profile extruded across width
            let prof: [(f32, f32); 4] = [(0.0, 0.0), (0.5, 0.35), (0.9, 0.28), (1.0, 0.05)];
            for j in 0..prof.len() - 1 {
                let (z0, y0) = prof[j];
                let (z1, y1) = prof[j + 1];
                let (dz, dy) = ((z1 - z0) * 0.75, y1 - y0);
                let ll = (dz * dz + dy * dy).sqrt().max(1e-6);
                // top-facing normal, winding reversed to face up
                b.quad(
                    Mat::Roof,
                    [-hw, 1.5 + y0, z0 * 0.75],
                    [hw, 1.5 + y0, z0 * 0.75],
                    [hw, 1.5 + y1, z1 * 0.75],
                    [-hw, 1.5 + y1, z1 * 0.75],
                    [0.0, dz / ll, -dy / ll],
                    &[[0.0; 2]; 4],
                    true,
                );
            }
            // side returns
            boxp(
                &mut b,
                Mat::Plaster,
                -hw,
                0.75,
                0.35,
                0.03,
                0.75,
                0.32,
                &[[0.0; 2]; 4],
            );
            boxp(
                &mut b,
                Mat::Plaster,
                hw,
                0.75,
                0.35,
                0.03,
                0.75,
                0.32,
                &[[0.0; 2]; 4],
            );
        }
        PartKind::Bracket => {
            // stepped L-corbel 0.15 wide under a chajja
            boxp(
                &mut b,
                Mat::Stone,
                0.0,
                -0.10,
                0.10,
                0.075,
                0.05,
                0.10,
                &[[0.0; 2]; 4],
            );
        }
    }
    Lod {
        prims: b
            .prims
            .into_iter()
            .filter(|p: &Prim| !p.indices.is_empty())
            .map(crate::orient_to_normals)
            .collect(),
    }
}

/// Placement: instances for a whole house (house-local coords, metres).
pub fn place_parts(
    spec: &HouseSpec,
    rng: &mut Rng,
    front_door: Option<(f64, f64, f64, f64)>, // (cx, w, front_dir x,z) plan pos
    front_piece: Option<(f64, f64, f64, f64, f64)>, // (edge x0,z0,dir x,z,len)
    storey_y: &[f64],
    front_i_windows: &[(u32, f64, f64)], // (storey, u0,u1) central windows
) -> (Vec<PartInstance>, Vec<crate::BoxCollider>) {
    let mut inst = Vec::new();
    let mut cols = Vec::new();
    // door leaf on lod0+lod1
    if let Some((cx, w, fx, fz)) = front_door {
        let key = part_key(PartKind::DoorLeaf, "d");
        inst.push(PartInstance {
            key,
            kind: PartKind::DoorLeaf,
            params: "d".into(),
            transform: door_transform(cx, fx, fz),
            lods: 3,
        });
        let _ = w;
    }
    // chhatris on the roof
    if spec.complexity >= 1 {
        let (hw, hd) = outer_half(spec);
        let n = match spec.footprint {
            Footprint::Courtyard { .. } => {
                if spec.complexity == 2 {
                    4
                } else {
                    2
                }
            }
            _ => 1 + (rng.next_u64() % 2) as usize,
        };
        let side_b = [1.6f32, 2.0, 2.4][(rng.next_u64() % 3) as usize];
        let inset = 1.0f64;
        let corners = [
            (hw - inset, hd - inset),
            (-(hw - inset), hd - inset),
            (hw - inset, -(hd - inset)),
            (-(hw - inset), -(hd - inset)),
        ];
        let top = storey_y.last().copied().unwrap_or(3.2);
        for &(cx, cz) in corners.iter().take(n) {
            inst.push(PartInstance {
                key: part_key(PartKind::Chhatri, &format!("s{side_b}c{}", spec.complexity)),
                kind: PartKind::Chhatri,
                params: format!("s{side_b}c{}", spec.complexity),
                transform: col_major(cx as f32, top as f32, cz as f32, 0),
                lods: 3,
            });
            cols.push(crate::BoxCollider {
                center: [cx as f32, top as f32 + side_b * 0.9, cz as f32],
                half: [side_b / 2.0, side_b * 0.9, side_b / 2.0],
            });
        }
    }
    // jharokha on front upper storeys (replaces central window visually)
    if spec.complexity >= 1 {
        let wb = [1.2f32, 1.5, 1.8][(rng.next_u64() % 3) as usize];
        for &(st, u0, u1) in front_i_windows.iter() {
            if st == 0 {
                continue;
            }
            if let Some(f) = front_piece {
                // piece: edge from (ax,az) direction d, length len; door u mid
                let (ax, az, dx, dz, _len) = f;
                let umid = (u0 + u1) / 2.0;
                let px = ax + dx * umid;
                let pz = az + dz * umid;
                inst.push(PartInstance {
                    key: part_key(PartKind::Jharokha, &format!("w{wb}")),
                    kind: PartKind::Jharokha,
                    params: format!("w{wb}"),
                    transform: jhar_transform(px, storey_y[st as usize], pz, dx, dz),
                    lods: 3,
                });
                let (nx, nz) = (dz, -dx);
                let (hx, hz) = if nx.abs() > 0.5 {
                    (0.4, wb / 2.0)
                } else {
                    (wb / 2.0, 0.4)
                };
                cols.push(crate::BoxCollider {
                    center: [
                        (px + nx * 0.35) as f32,
                        storey_y[st as usize] as f32 + 0.7,
                        (pz + nz * 0.35) as f32,
                    ],
                    half: [hx, 1.0, hz],
                });
            }
        }
    }
    // brackets under chajjas at complexity 2
    if spec.complexity == 2 {
        let pitch = 3.0;
        let (hw, hd) = outer_half(spec);
        // one bracket per bay along each outer edge
        for i in 0..4 {
            let ((ax, az), (bx, bz)) = match i {
                0 => ((-hw, -hd), (hw, -hd)),
                1 => ((hw, -hd), (hw, hd)),
                2 => ((hw, hd), (-hw, hd)),
                _ => ((-hw, hd), (-hw, -hd)),
            };
            let len = (bx - ax).hypot(bz - az);
            let n = ((len - 1.2) / pitch).round().max(1.0) as i64;
            let bay = (len - 1.2) / n as f64;
            let (dx, dz) = ((bx - ax) / len, (bz - az) / len);
            for k in 0..n {
                let u = 0.6 + (k as f64 + 0.5) * bay;
                inst.push(PartInstance {
                    key: part_key(PartKind::Bracket, "b"),
                    kind: PartKind::Bracket,
                    params: "b".into(),
                    transform: col_major(
                        (ax + dx * u) as f32,
                        (storey_y.last().copied().unwrap_or(3.2) - 0.05) as f32,
                        (az + dz * u) as f32,
                        0,
                    ),
                    lods: 1,
                });
            }
        }
    }
    (inst, cols)
}

fn door_transform(cx: f64, fx: f64, fz: f64) -> [f32; 12] {
    // leaf local +z faces out of wall; rotate to face the front dir
    let ry = match (fx.round() as i64, fz.round() as i64) {
        (0, -1) => 0u8,
        (1, 0) => 1,
        (0, 1) => 2,
        _ => 3,
    };
    // door centre position on the front face (cx unused for now —
    // caller passes plan centre of the opening)
    let _ = cx;
    col_major(0.0, 0.45, 0.0, ry)
}

/// Jharokha frame: local +Z (its projection) along the piece's outward
/// normal (dz, -dx) for a CCW edge direction (dx, dz).
fn jhar_transform(px: f64, y: f64, pz: f64, dx: f64, dz: f64) -> [f32; 12] {
    col_major(px as f32, y as f32, pz as f32, ry_for_normal(dz, -dx))
}

/// Quarter turns about +Y taking local +Z to the axis normal (nx, nz).
fn ry_for_normal(nx: f64, nz: f64) -> u8 {
    if nz > 0.5 {
        0
    } else if nx > 0.5 {
        1
    } else if nz < -0.5 {
        2
    } else {
        3
    }
}

fn outer_half(spec: &HouseSpec) -> (f64, f64) {
    match spec.footprint {
        Footprint::Rect { w, d }
        | Footprint::L { w, d, .. }
        | Footprint::Courtyard { w, d, .. } => (w / 2.0, d / 2.0),
    }
}

/// Public column-major transform helper (translation + 90deg turns).
pub fn place_transform(px: f32, py: f32, pz: f32, ry90: u8) -> [f32; 12] {
    col_major(px, py, pz, ry90)
}
