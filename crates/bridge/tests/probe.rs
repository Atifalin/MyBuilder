#[test]
fn golden_probe() {
    mb_seed::golden::check_hex(env!("CARGO_MANIFEST_DIR"), "probe", &mb_bridge::probe());
}

#[test]
fn version_is_set() {
    assert!(!mb_bridge::version().is_empty());
}

#[test]
fn derive_seed_hex_round_trip() {
    let h = mb_bridge::derive_seed_hex("0000000000000001", &[("region", "r0")]).unwrap();
    assert_eq!(h.len(), 16);
    assert!(mb_bridge::derive_seed_hex("zz", &[]).is_err());
}

#[test]
fn sample_fbm_in_range() {
    let v = mb_bridge::sample_fbm(42, 10.5, -20.25, 5, 0.5, 2.0, 0.01);
    assert!((-1.0..=1.0).contains(&v));
}
