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
