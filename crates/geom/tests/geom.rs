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
