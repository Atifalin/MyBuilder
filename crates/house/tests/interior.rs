//! Step 3a: interior layout tests over a 200-spec sweep.

use mb_house::interior::*;
use mb_house::{house_seed, openings};
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
        luxuries: Vec::new(),
    }
}

fn sweep() -> Vec<HouseSpec> {
    let mut rng = mb_seed::Rng::new(99);
    let sides = [Side::S, Side::E, Side::N, Side::W];
    let mut out = Vec::new();
    for i in 0..200 {
        let w = 6.0 + rng.range_f64(0.0, 20.0);
        let d = 6.0 + rng.range_f64(0.0, 20.0);
        let fp = match i % 3 {
            0 => Footprint::Rect { w, d },
            1 => Footprint::L {
                w,
                d,
                cut_w: 2.0 + rng.range_f64(0.0, (w - 5.0).max(0.1)),
                cut_d: 2.0 + rng.range_f64(0.0, (d - 5.0).max(0.1)),
            },
            _ => Footprint::Courtyard {
                w,
                d,
                court_w: (3.0 + rng.range_f64(0.0, (w - 11.0).max(0.1)))
                    .min(w - 8.0)
                    .max(3.0),
                court_d: (3.0 + rng.range_f64(0.0, (d - 11.0).max(0.1)))
                    .min(d - 8.0)
                    .max(3.0),
            },
        };
        let mut s = spec(
            &format!("i{i:03}"),
            fp,
            1 + (rng.next_u64() % 4) as u32,
            (rng.next_u64() % 3) as u8,
        );
        s.front = sides[(rng.next_u64() % 4) as usize];
        if i % 5 == 0 {
            s.luxuries = vec![Luxury::PoojaCorner, Luxury::Ac, Luxury::Tv];
        }
        let rec = HouseRecipe {
            schema_version: 1,
            generator_version: 1,
            houses: vec![s.clone()],
        };
        if validate_houses(&rec)
            .iter()
            .all(|d| d.severity != Severity::Error)
        {
            out.push(s);
        }
    }
    assert!(out.len() >= 150, "sweep too small: {}", out.len());
    out
}

fn snap(v: f64) -> i64 {
    mb_geom::snap(v)
}

/// Independent geometry: usable-area predicate, its exact area, and the
/// room-side wall faces (runs along x, line, lo, hi).
struct Geo {
    inner: [i64; 4],
    notch: Option<[i64; 4]>,
    court: Option<[i64; 4]>,
    lines: Vec<(bool, i64, i64, i64)>,
}

fn geo(s: &HouseSpec) -> Geo {
    let t = snap(s.wall_thickness_m);
    let (hw, hd) = (snap(s.footprint.width()) / 2, snap(s.footprint.depth()) / 2);
    let inner = [-hw + t, -hd + t, hw - t, hd - t];
    let mut lines = vec![
        (true, inner[1], inner[0], inner[2]),
        (true, inner[3], inner[0], inner[2]),
        (false, inner[0], inner[1], inner[3]),
        (false, inner[2], inner[1], inner[3]),
    ];
    let mut notch = None;
    let mut court = None;
    match s.footprint {
        Footprint::L { cut_w, cut_d, .. } => {
            let (nx, nz) = (hw - snap(cut_w) - t, hd - snap(cut_d) - t);
            notch = Some([nx, nz, inner[2], inner[3]]);
            lines = vec![
                (true, inner[1], inner[0], inner[2]),
                (false, inner[2], inner[1], nz),
                (true, nz, nx, inner[2]),
                (false, nx, nz, inner[3]),
                (true, inner[3], inner[0], nx),
                (false, inner[0], inner[1], inner[3]),
            ];
        }
        Footprint::Courtyard {
            court_w, court_d, ..
        } => {
            let (cw, cd) = (snap(court_w) / 2 + t, snap(court_d) / 2 + t);
            let c = [-cw, -cd, cw, cd];
            court = Some(c);
            lines.extend([
                (true, c[1], c[0], c[2]),
                (true, c[3], c[0], c[2]),
                (false, c[0], c[1], c[3]),
                (false, c[2], c[1], c[3]),
            ]);
        }
        Footprint::Rect { .. } => {}
    }
    Geo {
        inner,
        notch,
        court,
        lines,
    }
}

fn inside(p: [i64; 2], r: [i64; 4]) -> bool {
    r[0] <= p[0] && p[0] < r[2] && r[1] <= p[1] && p[1] < r[3]
}

impl Geo {
    fn usable(&self, p: [i64; 2]) -> bool {
        inside(p, self.inner)
            && !self.notch.is_some_and(|n| inside(p, n))
            && !self.court.is_some_and(|c| inside(p, c))
    }
    fn area(&self) -> i64 {
        let a = |r: [i64; 4]| (r[2] - r[0]) * (r[3] - r[1]);
        a(self.inner) - self.notch.map_or(0, a) - self.court.map_or(0, a)
    }
    /// Wall-face length a room edge has on exterior/court walls.
    fn lit(&self, r: [i64; 4]) -> i64 {
        let mut l = 0;
        for &(h, line, lo, hi) in &self.lines {
            let (on, a0, a1) = if h {
                (r[1] == line || r[3] == line, r[0], r[2])
            } else {
                (r[0] == line || r[2] == line, r[1], r[3])
            };
            if on {
                l += (a1.min(hi) - a0.max(lo)).max(0);
            }
        }
        l
    }
}

/// Facade openings as (runs along x, room-side line, lo, hi).
fn ops(s: &HouseSpec, seed: u64, storey: u32) -> Vec<(bool, i64, i64, i64, bool)> {
    let t = snap(s.wall_thickness_m);
    let (hw, hd) = (snap(s.footprint.width()) / 2, snap(s.footprint.depth()) / 2);
    let outer: Vec<[i64; 2]> = match s.footprint {
        Footprint::L { cut_w, cut_d, .. } => {
            let (cw, cd) = (snap(cut_w), snap(cut_d));
            vec![
                [-hw, -hd],
                [hw, -hd],
                [hw, hd - cd],
                [hw - cw, hd - cd],
                [hw - cw, hd],
                [-hw, hd],
            ]
        }
        _ => vec![[-hw, -hd], [hw, -hd], [hw, hd], [-hw, hd]],
    };
    let co: Vec<[i64; 2]> = match s.footprint {
        Footprint::Courtyard {
            court_w, court_d, ..
        } => {
            let (a, b) = (snap(court_w) / 2 + t, snap(court_d) / 2 + t);
            vec![[-a, -b], [a, -b], [a, b], [-a, b]]
        }
        _ => vec![],
    };
    openings(s, seed)
        .into_iter()
        .filter(|o| o.storey == storey)
        .map(|o| {
            let ring = if o.court { &co } else { &outer };
            let (a, b) = (ring[o.piece], ring[(o.piece + 1) % ring.len()]);
            let (u0, u1) = (snap(o.u0), snap(o.u1));
            let off = if o.court { 0 } else { t };
            if a[1] == b[1] {
                let sg = (b[0] - a[0]).signum();
                let (p, q) = (a[0] + sg * u0, a[0] + sg * u1);
                (true, a[1] + sg * off, p.min(q), p.max(q), o.kind == "door")
            } else {
                let sg = (b[1] - a[1]).signum();
                let (p, q) = (a[1] + sg * u0, a[1] + sg * u1);
                (false, a[0] - sg * off, p.min(q), p.max(q), o.kind == "door")
            }
        })
        .collect()
}

fn kinds(f: &Floor, k: RoomKind) -> usize {
    f.rooms.iter().filter(|r| r.kind == k).count()
}

fn on_edge(r: [i64; 4], seg: [[i64; 2]; 2]) -> bool {
    let [[x0, z0], [x1, z1]] = seg;
    if z0 == z1 {
        (r[1] == z0 || r[3] == z0) && r[0] <= x0.min(x1) && x0.max(x1) <= r[2]
    } else {
        (r[0] == x0 || r[2] == x0) && r[1] <= z0.min(z1) && z0.max(z1) <= r[3]
    }
}

#[test]
fn rooms_tile_usable_area() {
    for s in sweep() {
        let g = geo(&s);
        let p = layout(&s, house_seed(1, &s.id));
        assert_eq!(p.floors.len() as u32, s.floors);
        for f in &p.floors {
            let sum: i64 = f
                .rooms
                .iter()
                .map(|r| (r.rect[2] - r.rect[0]) * (r.rect[3] - r.rect[1]))
                .sum();
            assert_eq!(sum, g.area(), "{} s{} area sum", s.id, f.storey);
            for r in &f.rooms {
                assert!(
                    r.rect[0] < r.rect[2] && r.rect[1] < r.rect[3],
                    "{} empty {r:?}",
                    s.id
                );
            }
            // Raster: every usable sample in exactly one room, no room
            // sample outside the usable area.
            let mut z = g.inner[1] - 263;
            while z < g.inner[3] + 263 {
                let mut x = g.inner[0] - 263;
                while x < g.inner[2] + 263 {
                    let n = f.rooms.iter().filter(|r| inside([x, z], r.rect)).count();
                    let want = g.usable([x, z]) as usize;
                    assert_eq!(
                        n, want,
                        "{} s{} sample ({x},{z}) in {n} rooms",
                        s.id, f.storey
                    );
                    x += 197;
                }
                z += 197;
            }
        }
    }
}

#[test]
fn essentials_and_stair() {
    for s in sweep() {
        let p = layout(&s, house_seed(1, &s.id));
        for f in &p.floors {
            if f.storey == 0 {
                assert_eq!(kinds(f, RoomKind::Living), 1, "{} living", s.id);
                assert!(kinds(f, RoomKind::Kitchen) >= 1, "{} no kitchen", s.id);
                assert!(kinds(f, RoomKind::Washroom) >= 1, "{} no wash", s.id);
                if s.floors == 1 {
                    assert!(kinds(f, RoomKind::Bedroom) >= 1, "{} no bed", s.id);
                }
                if s.luxuries.contains(&Luxury::PoojaCorner) {
                    assert!(kinds(f, RoomKind::Pooja) >= 1, "{} no pooja", s.id);
                }
            } else {
                assert!(
                    kinds(f, RoomKind::Bedroom) >= 1,
                    "{} s{} no bed",
                    s.id,
                    f.storey
                );
                assert!(
                    kinds(f, RoomKind::Washroom) >= 1,
                    "{} s{} no wash",
                    s.id,
                    f.storey
                );
            }
            let st: Vec<_> = f
                .rooms
                .iter()
                .filter(|r| r.kind == RoomKind::Stair)
                .collect();
            if s.floors >= 2 {
                assert_eq!(st.len(), 1, "{} s{} stair", s.id, f.storey);
                assert_eq!(Some(st[0].rect), p.stair, "{} stair must stack", s.id);
            } else {
                assert!(st.is_empty() && p.stair.is_none());
            }
            for (l, id) in &f.fixtures {
                let k = f.rooms[*id as usize].kind;
                match l {
                    Luxury::Ac => assert!(matches!(k, RoomKind::Bedroom | RoomKind::Living)),
                    Luxury::Tv | Luxury::Jhoola => assert_eq!(k, RoomKind::Living),
                    Luxury::WashingMachine => {
                        assert!(matches!(k, RoomKind::Washroom | RoomKind::Store))
                    }
                    Luxury::PoojaCorner => panic!("pooja is a room, not a fixture"),
                }
            }
        }
    }
}

#[test]
fn reachability() {
    for s in sweep() {
        let p = layout(&s, house_seed(1, &s.id));
        for f in &p.floors {
            let start = if f.storey == 0 {
                u32::MAX
            } else {
                f.rooms
                    .iter()
                    .find(|r| r.kind == RoomKind::Stair)
                    .unwrap()
                    .id
            };
            let mut seen = vec![start];
            let mut q = vec![start];
            while let Some(cur) = q.pop() {
                for d in &f.doors {
                    let o = if d.a == cur {
                        d.b
                    } else if d.b == cur {
                        d.a
                    } else {
                        continue;
                    };
                    if !seen.contains(&o) {
                        seen.push(o);
                        q.push(o);
                    }
                }
            }
            for r in &f.rooms {
                assert!(
                    seen.contains(&r.id),
                    "{} s{} {:?} {:?} unreachable",
                    s.id,
                    f.storey,
                    r.kind,
                    r.rect
                );
            }
        }
    }
}

#[test]
fn doors_on_shared_walls_and_front_door() {
    for s in sweep() {
        let g = geo(&s);
        let p = layout(&s, house_seed(1, &s.id));
        for f in &p.floors {
            let mut outside = 0;
            for d in &f.doors {
                let len = (d.seg[1][0] - d.seg[0][0]).abs() + (d.seg[1][1] - d.seg[0][1]).abs();
                assert_eq!(len, 900, "{} door len", s.id);
                let ra = f.rooms[d.a as usize].rect;
                assert!(
                    on_edge(ra, d.seg),
                    "{} door off room {ra:?} {:?}",
                    s.id,
                    d.seg
                );
                if d.b == u32::MAX {
                    outside += 1;
                    let [[x0, z0], [x1, z1]] = d.seg;
                    let on_wall = g.lines.iter().any(|&(h, l, lo, hi)| {
                        if h {
                            z0 == l && z1 == l && lo <= x0.min(x1) && x0.max(x1) <= hi
                        } else {
                            x0 == l && x1 == l && lo <= z0.min(z1) && z0.max(z1) <= hi
                        }
                    });
                    assert!(on_wall, "{} outside door not on a wall {:?}", s.id, d.seg);
                } else {
                    let rb = f.rooms[d.b as usize].rect;
                    assert!(on_edge(rb, d.seg), "{} door off room {rb:?}", s.id);
                    // >= 150 mm clear of the shared wall's ends.
                    let [[x0, z0], [x1, z1]] = d.seg;
                    let (lo, hi, a, b) = if z0 == z1 {
                        (ra[0].max(rb[0]), ra[2].min(rb[2]), x0.min(x1), x0.max(x1))
                    } else {
                        (ra[1].max(rb[1]), ra[3].min(rb[3]), z0.min(z1), z0.max(z1))
                    };
                    assert!(
                        a - lo >= 150 && hi - b >= 150,
                        "{} door too close to a corner",
                        s.id
                    );
                }
            }
            if f.storey == 0 {
                let front = ops(&s, house_seed(1, &s.id), 0)
                    .into_iter()
                    .find(|o| o.4 && g.lines.iter().take(6).any(|l| l.0 == o.0 && l.1 == o.1))
                    .expect("front door opening");
                let liv = f.rooms.iter().find(|r| r.kind == RoomKind::Living).unwrap();
                let (lo, hi) = if front.0 {
                    (liv.rect[0], liv.rect[2])
                } else {
                    (liv.rect[1], liv.rect[3])
                };
                let on = if front.0 {
                    liv.rect[1] == front.1 || liv.rect[3] == front.1
                } else {
                    liv.rect[0] == front.1 || liv.rect[2] == front.1
                };
                assert!(
                    on && lo <= front.2 && front.3 <= hi,
                    "{} front door not in Living",
                    s.id
                );
                let want = 1 + s.footprint.court().is_some() as usize;
                assert_eq!(outside, want, "{} outside doors", s.id);
            } else {
                assert_eq!(outside, 0);
            }
        }
    }
}

#[test]
fn pier_rule() {
    let (mut chosen, mut fixed, mut ends) = (0usize, 0usize, 0usize);
    let mut by_kind = [0usize; 3];
    for s in sweep() {
        let seed = house_seed(1, &s.id);
        let p = layout(&s, seed);
        let wing_corners: Vec<[i64; 2]> = p
            .wings
            .iter()
            .flat_map(|w| [[w[0], w[1]], [w[2], w[1]], [w[0], w[3]], [w[2], w[3]]])
            .collect();
        for f in &p.floors {
            let os = ops(&s, seed, f.storey);
            for r in &f.rooms {
                for pt in [
                    [r.rect[0], r.rect[1]],
                    [r.rect[2], r.rect[1]],
                    [r.rect[0], r.rect[3]],
                    [r.rect[2], r.rect[3]],
                ] {
                    if os
                        .iter()
                        .any(|&(h, l, ..)| if h { pt[1] == l } else { pt[0] == l })
                    {
                        ends += 1;
                    }
                    let hit = os.iter().any(|&(h, l, lo, hi, _)| {
                        let (on, c) = if h {
                            (pt[1] == l, pt[0])
                        } else {
                            (pt[0] == l, pt[1])
                        };
                        on && c > lo - 100 && c < hi + 100
                    });
                    if !hit {
                        continue;
                    }
                    if wing_corners.contains(&pt) {
                        fixed += 1;
                    } else {
                        chosen += 1;
                        by_kind[match s.footprint {
                            Footprint::Rect { .. } => 0,
                            Footprint::L { .. } => 1,
                            Footprint::Courtyard { .. } => 2,
                        }] += 1;
                        eprintln!("{} s{} wall end {pt:?} inside an opening", s.id, f.storey);
                    }
                }
            }
        }
    }
    println!(
        "pier clashes: chosen cuts {chosen} (rect/L/court {by_kind:?}), footprint-fixed wing corners {fixed}, of {ends} wall ends on facades"
    );
    // Windows give way to interior walls, so no wall may end in one.
    assert_eq!(chosen + fixed, 0, "interior walls ending inside openings");
}

#[test]
fn light_stacking_and_determinism() {
    let (mut lit, mut hab) = (0usize, 0usize);
    let mut windowed = 0usize;
    let (mut stacked, mut uppers) = (0usize, 0usize);
    let (mut kw, mut grounds) = (0usize, 0usize);
    let mut differ = 0;
    for s in sweep() {
        let g = geo(&s);
        let seed = house_seed(1, &s.id);
        let p1 = layout(&s, seed);
        let p2 = layout(&s, seed);
        let ser = |p: &FloorPlan| format!("{:?}", p.floors);
        assert_eq!(ser(&p1), ser(&p2), "{} nondeterministic", s.id);
        let mut other = s.clone();
        other.id.push('x');
        if ser(&layout(&other, house_seed(1, &other.id))) != ser(&p1) {
            differ += 1;
        }
        let ground_wash: Vec<[i64; 4]> = p1.floors[0]
            .rooms
            .iter()
            .filter(|r| r.kind == RoomKind::Washroom)
            .map(|r| r.rect)
            .collect();
        for f in &p1.floors {
            let os = ops(&s, seed, f.storey);
            for r in &f.rooms {
                if matches!(r.kind, RoomKind::Living | RoomKind::Bedroom) {
                    hab += 1;
                    let [x0, z0, x1, z1] = r.rect;
                    let has = os.iter().any(|&(h, l, lo, hi, _)| {
                        if h {
                            (l == z0 || l == z1) && lo >= x0 && hi <= x1
                        } else {
                            (l == x0 || l == x1) && lo >= z0 && hi <= z1
                        }
                    });
                    windowed += has as usize;
                    if g.lit(r.rect) >= 1000 {
                        lit += 1;
                    } else {
                        eprintln!("{} s{} dark {:?} {:?}", s.id, f.storey, r.kind, r.rect);
                    }
                }
            }
            if f.storey > 0 {
                for r in f.rooms.iter().filter(|r| r.kind == RoomKind::Washroom) {
                    uppers += 1;
                    if ground_wash.iter().any(|w| {
                        w[0] < r.rect[2] && r.rect[0] < w[2] && w[1] < r.rect[3] && r.rect[1] < w[3]
                    }) {
                        stacked += 1;
                    }
                }
            } else {
                grounds += 1;
                let k: Vec<_> = f
                    .rooms
                    .iter()
                    .filter(|r| r.kind == RoomKind::Kitchen)
                    .collect();
                let adj = f
                    .rooms
                    .iter()
                    .filter(|r| r.kind == RoomKind::Washroom)
                    .any(|w| {
                        k.iter().any(|k| {
                            let (a, b) = (k.rect, w.rect);
                            ((a[2] == b[0] || b[2] == a[0]) && a[1].max(b[1]) < a[3].min(b[3]))
                                || ((a[3] == b[1] || b[3] == a[1])
                                    && a[0].max(b[0]) < a[2].min(b[2]))
                        })
                    });
                kw += adj as usize;
            }
        }
    }
    println!(
        "light: living+bedroom {lit}/{hab}, with a window or door {windowed}/{hab}; washroom stacked over ground {stacked}/{uppers}; \
         kitchen next to washroom {kw}/{grounds}; id changes plan {differ}"
    );
    assert_eq!(
        lit, hab,
        "every living room and bedroom needs an exterior wall"
    );
    assert!(differ > 0, "house id never changes the plan");
    assert!(
        windowed * 100 >= hab * 90,
        "rooms without a window: {windowed}/{hab}"
    );
}

#[test]
fn png_renders_rooms() {
    let s = spec("rect", Footprint::Rect { w: 10.0, d: 8.0 }, 2, 1);
    let p = layout(&s, house_seed(1, &s.id));
    let png = floor_plan_png(&s, &p, 0, 30);
    let dec = png::Decoder::new(std::io::Cursor::new(&png));
    let mut rd = dec.read_info().unwrap();
    let mut buf = vec![0; rd.output_buffer_size().unwrap()];
    let info = rd.next_frame(&mut buf).unwrap();
    assert_eq!((info.width, info.height), (360, 300));
    let liv = p.floors[0]
        .rooms
        .iter()
        .find(|r| r.kind == RoomKind::Living)
        .unwrap();
    let (cx, cz) = (
        (liv.rect[0] + liv.rect[2]) / 2,
        (liv.rect[1] + liv.rect[3]) / 2,
    );
    let (px, py) = (
        (cx + 5000 + 1000) * 30 / 1000,
        (4000 + 1000 - cz) * 30 / 1000,
    );
    let i = (py as usize * info.width as usize + px as usize) * 4;
    assert_eq!(
        &buf[i..i + 3],
        &[245, 220, 140],
        "living colour at its centre"
    );
}

#[test]
fn reference_pngs() {
    let specs = [
        spec("rect", Footprint::Rect { w: 10.0, d: 8.0 }, 2, 1),
        spec(
            "ell",
            Footprint::L {
                w: 14.0,
                d: 12.0,
                cut_w: 6.0,
                cut_d: 5.0,
            },
            2,
            2,
        ),
        spec(
            "haveli",
            Footprint::Courtyard {
                w: 20.0,
                d: 20.0,
                court_w: 8.0,
                court_d: 8.0,
            },
            2,
            2,
        ),
    ];
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gates/house");
    std::fs::create_dir_all(&dir).unwrap();
    for s in &specs {
        let p = layout(s, house_seed(1, &s.id));
        for f in &p.floors {
            let png = floor_plan_png(s, &p, f.storey, 30);
            assert!(png.len() > 200 && &png[1..4] == b"PNG");
            std::fs::write(dir.join(format!("plan_{}_{}.png", s.id, f.storey)), &png).unwrap();
        }
    }
}
