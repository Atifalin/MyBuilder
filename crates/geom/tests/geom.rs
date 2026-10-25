use mb_geom::{FbmParams, Perlin, fbm, ridged, snap, to_meters};

#[test]
fn snap_round_half_away_from_zero() {
    assert_eq!(snap(1.0004), 1000);
    assert_eq!(snap(1.0005), 1001); // .5 mm rounds away from zero
    assert_eq!(snap(-1.0005), -1001);
    assert_eq!(snap(0.0), 0);
    assert_eq!(to_meters(snap(12.3456)), 12.346);
}

#[test]
fn noise_is_deterministic_and_smooth() {
    let a = Perlin::new(42);
    let b = Perlin::new(42);
    let c = Perlin::new(43);
    let mut any_diff = false;
    for i in 0..50 {
        let x = i as f64 * 0.37 - 9.0;
        let z = i as f64 * -0.11 + 4.0;
        assert_eq!(a.noise(x, z), b.noise(x, z));
        let v = a.noise(x, z);
        assert!((-1.1..=1.1).contains(&v));
        if a.noise(x, z) != c.noise(x, z) {
            any_diff = true;
        }
    }
    assert!(any_diff, "different seeds should give different fields");
}

#[test]
fn fbm_normalized_range() {
    let p = Perlin::new(7);
    let params = FbmParams {
        octaves: 5,
        lacunarity: 2.0,
        gain: 0.5,
        frequency: 0.01,
    };
    for i in 0..200 {
        let v = fbm(&p, i as f64 * 13.7, -300.0 + i as f64 * 1.3, &params);
        assert!((-1.0..=1.0).contains(&v), "fbm out of range: {v}");
        let r = ridged(v);
        assert!((0.0..=1.0).contains(&r));
    }
}

// Phase 2 step 1: shape-tool geometry.

fn circle_pts(n: usize, r: f64, noise: f64) -> Vec<[f64; 2]> {
    let mut pts = Vec::new();
    for i in 0..n {
        let t = i as f64 / n as f64 * 2.0 * std::f64::consts::PI;
        let rr = r + noise * (i as f64 * 0.7).sin() * 0.5;
        pts.push([rr * t.cos(), rr * t.sin()]);
    }
    pts
}

#[test]
fn dp_keeps_endpoints_and_within_epsilon() {
    let pts = circle_pts(200, 100.0, 0.4);
    let open = pts.clone();
    let eps = 0.8;
    let out = mb_geom::simplify_dp(&open, eps, false);
    assert_eq!(out.first(), open.first());
    assert_eq!(out.last(), open.last());
    assert!(out.len() < open.len(), "should simplify");
    // every removed point within eps of the simplified polyline
    for p in &open {
        let mut best = f64::MAX;
        for w in out.windows(2) {
            let (a, b) = (w[0], w[1]);
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let l2 = dx * dx + dy * dy;
            let t = if l2 == 0.0 {
                0.0
            } else {
                (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
            };
            let qx = a[0] + t * dx;
            let qy = a[1] + t * dy;
            let d = ((p[0] - qx).powi(2) + (p[1] - qy).powi(2)).sqrt();
            if d < best {
                best = d;
            }
        }
        assert!(best <= eps + 1e-9, "removed point {best} > eps");
    }
    // deterministic
    assert_eq!(out, mb_geom::simplify_dp(&open, eps, false));
    mb_seed::golden::check_hex(
        env!("CARGO_MANIFEST_DIR"),
        "simplify_dp",
        &mb_seed::hex_bytes(&mb_seed::hash_bytes(format!("{out:?}").as_bytes())),
    );
}

#[test]
fn chaikin_doubles_and_stays_in_hull() {
    let pts = circle_pts(8, 50.0, 0.0);
    let out = mb_geom::chaikin(&pts, 2, true);
    assert_eq!(out.len(), pts.len() * 4);
    // inside the convex hull of the input (rough check: radius bound)
    let max_r = pts
        .iter()
        .map(|p| p[0] * p[0] + p[1] * p[1])
        .fold(0.0, f64::max)
        .sqrt();
    for p in &out {
        let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
        assert!(r <= max_r + 1e-6, "chaikin escaped the hull");
    }
    assert_eq!(out, mb_geom::chaikin(&pts, 2, true));
    mb_seed::golden::check_hex(
        env!("CARGO_MANIFEST_DIR"),
        "chaikin",
        &mb_seed::hex_bytes(&mb_seed::hash_bytes(format!("{out:?}").as_bytes())),
    );
}

// ---- triangulate_with_holes ----
fn area2(poly: &[[i64; 2]]) -> i128 {
    let m = poly.len();
    (0..m)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % m]);
            a[0] as i128 * b[1] as i128 - b[0] as i128 * a[1] as i128
        })
        .sum::<i128>()
        .abs()
}

fn check(outer: &[[i64; 2]], holes: &[Vec<[i64; 2]>]) {
    let tris = mb_geom::triangulate_with_holes(outer, holes).unwrap();
    let mut pts: Vec<[i64; 2]> = outer.to_vec();
    for h in holes {
        pts.extend_from_slice(h);
    }
    let (n, hn) = (pts.len(), holes.len());
    assert_eq!(tris.len(), n + 2 * hn - 2, "n={n} h={hn}");
    let mut tri_a: i128 = 0;
    for t in &tris {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let o = ((b[0] - a[0]) as i128) * ((c[1] - a[1]) as i128)
            - ((b[1] - a[1]) as i128) * ((c[0] - a[0]) as i128);
        assert!(o > 0, "non-positive triangle {t:?}");
        tri_a += o;
    }
    let holes_a: i128 = holes.iter().map(|h| area2(h)).sum();
    assert_eq!(tri_a, area2(outer) - holes_a, "area mismatch");
    assert_eq!(tris, mb_geom::triangulate_with_holes(outer, holes).unwrap());
}

#[test]
fn tri_square_two_holes() {
    check(
        &[[0, 0], [20, 0], [20, 20], [0, 20]],
        &[
            vec![[3, 3], [7, 3], [7, 7], [3, 7]],
            vec![[12, 12], [16, 12], [16, 16], [12, 16]],
        ],
    );
}

#[test]
fn tri_concave() {
    check(&[[0, 0], [10, 0], [10, 4], [4, 4], [4, 10], [0, 10]], &[]);
    check(
        &[
            [0, 0],
            [12, 0],
            [12, 8],
            [8, 8],
            [8, 4],
            [4, 4],
            [4, 8],
            [0, 8],
        ],
        &[],
    );
    // Clockwise input is normalised.
    check(&[[0, 10], [4, 10], [4, 4], [10, 4], [10, 0], [0, 0]], &[]);
}

#[test]
fn tri_courtyard_ring() {
    check(
        &[[0, 0], [18, 0], [18, 18], [0, 18]],
        &[vec![[5, 5], [13, 5], [13, 13], [5, 13]]],
    );
    // L-shaped slab with a hole, holes given counter-clockwise.
    check(
        &[[0, 0], [30, 0], [30, 12], [12, 12], [12, 30], [0, 30]],
        &[
            vec![[3, 3], [9, 3], [9, 9], [3, 9]],
            vec![[3, 15], [9, 15], [9, 25], [3, 25]],
        ],
    );
}

#[test]
fn tri_wall_arch_openings() {
    // 12 m x 3.2 m wall face (decimetres): two windows, two windows sharing
    // a max-x column, and a pointed-arch door.
    let arch = vec![
        [80, 3],
        [92, 3],
        [92, 20],
        [91, 24],
        [89, 27],
        [86, 30],
        [83, 27],
        [81, 24],
        [80, 20],
    ];
    check(
        &[[0, 0], [120, 0], [120, 32], [0, 32]],
        &[
            vec![[10, 9], [16, 9], [16, 24], [10, 24]],
            vec![[60, 9], [66, 9], [66, 15], [60, 15]],
            vec![[60, 18], [66, 18], [66, 26], [60, 26]],
            arch,
        ],
    );
}

#[test]
fn tri_rectilinear_wall_with_notch() {
    // Wall face whose outline steps down (parapet notch) with openings
    // whose corners line up with outline vertices.
    check(
        &[
            [0, 0],
            [100, 0],
            [100, 30],
            [70, 30],
            [70, 36],
            [30, 36],
            [30, 30],
            [0, 30],
        ],
        &[
            vec![[30, 10], [40, 10], [40, 25], [30, 25]],
            vec![[60, 10], [70, 10], [70, 25], [60, 25]],
        ],
    );
}

#[test]
fn tri_random_walls_with_openings() {
    // Deterministic LCG; each case is a wall face with openings placed
    // in disjoint bays, some with pointed-arch tops.
    let mut s: u64 = 0x9e3779b97f4a7c15;
    let mut rnd = |m: i64| {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((s >> 33) % m as u64) as i64
    };
    for _ in 0..1000 {
        let bays = 1 + rnd(7);
        let bay_w = 20 + rnd(30);
        let (w, h) = (bays * bay_w, 40 + rnd(20));
        let mut holes = Vec::new();
        for b in 0..bays {
            if rnd(4) == 0 {
                continue;
            }
            let x0 = b * bay_w + 2 + rnd(4);
            let x1 = (b + 1) * bay_w - 2 - rnd(4);
            let y0 = if rnd(3) == 0 { 1 } else { 5 + rnd(10) };
            let y1 = h - 6 - rnd(8);
            if x1 - x0 < 4 || y1 - y0 < 6 {
                continue;
            }
            if rnd(2) == 0 {
                holes.push(vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]);
            } else {
                let mid = (x0 + x1) / 2;
                holes.push(vec![
                    [x0, y0],
                    [x1, y0],
                    [x1, y1 - 3],
                    [mid + 1, y1 - 1],
                    [mid, y1],
                    [x0, y1 - 3],
                ]);
            }
        }
        check(&[[0, 0], [w, 0], [w, h], [0, h]], &holes);
    }
}
