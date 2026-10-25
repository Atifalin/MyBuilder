#![allow(clippy::needless_range_loop)]
use mb_house::*;
use mb_schema::*;

fn spec(id: &str, fp: Footprint, floors: u32, cx: u8) -> HouseSpec {
    HouseSpec {
        id: id.into(),
        footprint: fp,
        floors,
        floor_height_m: 3.2,
        wall_thickness_m: 0.35,
        plinth_m: 0.45,
        front: Side::S,
        roof: Roof::default(),
        complexity: cx,
        placement: None,
    }
}

fn sweep() -> Vec<HouseSpec> {
    let mut rng = mb_seed::Rng::new(0xB00);
    let mut v = Vec::new();
    for i in 0..200 {
        let floors = rng.range_u64(3) as u32 + 1;
        let cx = rng.range_u64(3) as u8;
        let fp = match i % 3 {
            0 => Footprint::Rect {
                w: rng.range_f64(6.0, 20.0),
                d: rng.range_f64(6.0, 20.0),
            },
            1 => {
                let w = rng.range_f64(10.0, 22.0);
                let d = rng.range_f64(10.0, 22.0);
                Footprint::L {
                    w,
                    d,
                    cut_w: rng.range_f64(2.0, w - 4.0),
                    cut_d: rng.range_f64(2.0, d - 4.0),
                }
            }
            _ => {
                let w = rng.range_f64(11.0, 24.0);
                let d = rng.range_f64(11.0, 24.0);
                Footprint::Courtyard {
                    w,
                    d,
                    court_w: rng.range_f64(3.0, (w - 8.0).max(3.0)),
                    court_d: rng.range_f64(3.0, (d - 8.0).max(3.0)),
                }
            }
        };
        v.push(spec(&format!("h{i:03}"), fp, floors, cx));
    }
    v
}

fn tris(l: &Lod) -> usize {
    l.prims.iter().map(|p| p.indices.len() / 3).sum()
}

#[test]
fn budget_sweep() {
    let mut stats = [[0usize; 3], [usize::MAX; 3]];
    let mut meds: [Vec<usize>; 3] = Default::default();
    for s in sweep() {
        let h = generate(&s, house_seed(7, &s.id));
        for (l, t) in [tris(&h.lods[0]), tris(&h.lods[1]), tris(&h.lods[2])]
            .iter()
            .enumerate()
        {
            stats[0][l] = stats[0][l].max(*t);
            meds[l].push(*t);
        }
        assert!(
            tris(&h.lods[0]) <= 8000,
            "{} LOD0 {}",
            s.id,
            tris(&h.lods[0])
        );
        assert!(
            tris(&h.lods[1]) <= 1500,
            "{} LOD1 {}",
            s.id,
            tris(&h.lods[1])
        );
        let lim = if matches!(s.footprint, Footprint::Courtyard { .. }) {
            128
        } else {
            64
        };
        assert!(
            tris(&h.lods[2]) <= lim,
            "{} LOD2 {}",
            s.id,
            tris(&h.lods[2])
        );
        for l in &h.lods {
            assert!(l.prims.len() <= 6);
        }
    }
    for l in 0..3 {
        meds[l].sort();
        eprintln!(
            "LOD{l}: max={} median={}",
            stats[0][l],
            meds[l][meds[l].len() / 2]
        );
    }
}

#[test]
fn sanity() {
    for s in sweep() {
        let h = generate(&s, house_seed(1, &s.id));
        for l in &h.lods {
            for p in &l.prims {
                for (k, v) in p.positions.iter().enumerate() {
                    for c in 0..3 {
                        assert!(
                            v[c] >= h.bounds.0[c] - 1e-4 && v[c] <= h.bounds.1[c] + 1e-4,
                            "{} bound",
                            s.id
                        );
                    }
                    let nl = (p.normals[k][0].powi(2)
                        + p.normals[k][1].powi(2)
                        + p.normals[k][2].powi(2))
                    .sqrt();
                    assert!((nl - 1.0).abs() < 1e-4, "{} normal {nl}", s.id);
                }
                for &i in &p.indices {
                    assert!((i as usize) < p.positions.len());
                }
                for t in p.indices.chunks(3) {
                    let (a, b, c) = (
                        p.positions[t[0] as usize],
                        p.positions[t[1] as usize],
                        p.positions[t[2] as usize],
                    );
                    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                    let cr = [
                        ab[1] * ac[2] - ab[2] * ac[1],
                        ab[2] * ac[0] - ab[0] * ac[2],
                        ab[0] * ac[1] - ab[1] * ac[0],
                    ];
                    let ar = (cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2]).sqrt();
                    assert!(ar > 1e-8, "{} degenerate tri", s.id);
                }
            }
        }
    }
}

#[test]
fn wall_uv_scale() {
    for s in sweep() {
        let h = generate(&s, house_seed(2, &s.id));
        for p in &h.lods[0].prims {
            if p.mat != Mat::Plaster {
                continue;
            }
            for t in p.indices.chunks(3) {
                for e in [[t[0], t[1]], [t[1], t[2]], [t[2], t[0]]] {
                    let (i, j) = (e[0] as usize, e[1] as usize);
                    let dp = (0..3)
                        .map(|k| p.positions[i][k] - p.positions[j][k])
                        .map(|x| x * x)
                        .sum::<f32>()
                        .sqrt();
                    let du = (p.uvs[i][0] - p.uvs[j][0]).abs();
                    let dv = (p.uvs[i][1] - p.uvs[j][1]).abs();
                    let duv = (du * du + dv * dv).sqrt();
                    assert!((dp - duv).abs() < 1e-3, "{} uv {dp} vs {duv}", s.id);
                }
            }
        }
    }
}

#[test]
fn door_gap_all_sides() {
    for side in [Side::S, Side::N, Side::E, Side::W] {
        let mut s = spec("door", Footprint::Rect { w: 10.0, d: 8.0 }, 2, 1);
        s.front = side;
        let h = generate(&s, house_seed(3, "door"));
        // find a 1.3m gap at the front side wall, y in [plinth+0.1, +2.4]
        let (min, _max) = h.bounds;
        let _ = min;
        // probe a strip of points on the front face plane
        let mut gap = false;
        for i in 0..40 {
            let x = -5.0 + 0.25 * i as f32;
            let (px, pz) = match side {
                Side::S => (x, -4.0),
                Side::N => (x, 4.0),
                Side::E => (5.0, x),
                Side::W => (-5.0, x),
            };
            let mut blocked = false;
            for c in &h.colliders {
                if (px - c.center[0]).abs() < c.half[0]
                    && (1.4 - c.center[1]).abs() < c.half[1]
                    && (pz - c.center[2]).abs() < c.half[2]
                {
                    blocked = true;
                }
            }
            if !blocked {
                gap = true;
            }
        }
        assert!(gap, "no door gap for side {side:?}");
    }
}

#[test]
fn opening_margins() {
    for s in sweep() {
        let os = openings(&s, house_seed(4, &s.id));
        for o in os {
            // need piece length: reconstruct via footprint edges — the
            // piece index counts outer edges; verify u0>=0.6 & len-u1>=0.6
            assert!(o.u0 >= 0.6 - 1e-6, "{} margin {} {}", s.id, o.kind, o.u0);
        }
    }
}

#[test]
fn determinism() {
    let s = spec(
        "det",
        Footprint::Courtyard {
            w: 18.0,
            d: 18.0,
            court_w: 8.0,
            court_d: 8.0,
        },
        2,
        2,
    );
    let bytes = |h: &HouseMesh| -> Vec<u8> {
        let mut v = Vec::new();
        for l in &h.lods {
            for p in &l.prims {
                for x in &p.positions {
                    v.extend_from_slice(&x[0].to_le_bytes());
                }
                v.extend_from_slice(
                    &p.indices
                        .iter()
                        .flat_map(|i| i.to_le_bytes())
                        .collect::<Vec<_>>(),
                );
            }
        }
        v
    };
    let a = generate(&s, house_seed(9, "det"));
    let b = generate(&s, house_seed(9, "det"));
    assert_eq!(bytes(&a), bytes(&b));
    let mut s2 = s.clone();
    s2.id = "other".into();
    let c = generate(&s2, house_seed(9, "other"));
    assert_ne!(a.plaster_rgb, c.plaster_rgb);
}

#[test]
fn courtyard_slab_clear() {
    let s = spec(
        "ct",
        Footprint::Courtyard {
            w: 18.0,
            d: 18.0,
            court_w: 8.0,
            court_d: 8.0,
        },
        1,
        1,
    );
    let h = generate(&s, house_seed(5, "ct"));
    for l in &h.lods {
        for p in &l.prims {
            if p.mat != Mat::Roof && p.mat != Mat::Floor {
                continue;
            }
            for t in p.indices.chunks(3) {
                let c = [
                    (p.positions[t[0] as usize][0]
                        + p.positions[t[1] as usize][0]
                        + p.positions[t[2] as usize][0])
                        / 3.0,
                    (p.positions[t[0] as usize][2]
                        + p.positions[t[1] as usize][2]
                        + p.positions[t[2] as usize][2])
                        / 3.0,
                ];
                assert!(
                    !(c[0] > -3.99 && c[0] < 3.99 && c[1] > -3.99 && c[1] < 3.99),
                    "{:?} slab tri centroid inside court",
                    p.mat
                );
            }
        }
    }
}

#[test]
fn bounds_stay_near_the_footprint() {
    for s in sweep() {
        let h = generate(&s, house_seed(1, &s.id));
        let (w, d) = match s.footprint {
            Footprint::Rect { w, d }
            | Footprint::L { w, d, .. }
            | Footprint::Courtyard { w, d, .. } => (w, d),
        };
        let mb_schema::Roof::Flat { parapet_m } = s.roof;
        let top = s.plinth_m + s.floors as f64 * s.floor_height_m + 0.2 + parapet_m + 0.5;
        let reach = (w.max(d) / 2.0 + 2.5) as f32;
        let (lo, hi) = h.bounds;
        assert!(
            lo[0] >= -reach && hi[0] <= reach && lo[2] >= -reach && hi[2] <= reach,
            "{} xz {:?}",
            s.id,
            h.bounds
        );
        assert!(
            lo[1] >= -1e-4 && hi[1] <= top as f32,
            "{} y {:?}",
            s.id,
            h.bounds
        );
    }
}

#[test]
fn winding_matches_normals() {
    let mut stats = std::collections::BTreeMap::<String, (u32, u32)>::new();
    for s in sweep() {
        let h = generate(&s, house_seed(1, &s.id));
        for (li, l) in h.lods.iter().enumerate() {
            for p in &l.prims {
                for t in p.indices.chunks(3) {
                    let [a, b, c] = [0, 1, 2].map(|k| p.positions[t[k] as usize]);
                    let (e1, e2) = (
                        [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                        [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
                    );
                    let cr = [
                        e1[1] * e2[2] - e1[2] * e2[1],
                        e1[2] * e2[0] - e1[0] * e2[2],
                        e1[0] * e2[1] - e1[1] * e2[0],
                    ];
                    let n = p.normals[t[0] as usize];
                    let dot = cr[0] * n[0] + cr[1] * n[1] + cr[2] * n[2];
                    let e = stats
                        .entry(format!(
                            "lod{li} {:?} n={:?}",
                            p.mat,
                            n.map(|v| v.round() as i32)
                        ))
                        .or_insert((0, 0));
                    if dot > 0.0 { e.0 += 1 } else { e.1 += 1 }
                }
            }
        }
    }
    let bad: Vec<_> = stats.iter().filter(|(_, v)| v.1 > 0).collect();
    for (k, v) in &bad {
        println!("{k}: ok={} bad={}", v.0, v.1);
    }
    assert!(bad.is_empty(), "triangles facing against their normals");
}
