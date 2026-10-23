use mb_seed::{EntityId, Rng, derive, derive_path};

const DIR: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn golden_rng_stream() {
    let mut rng = Rng::new(0x00C0FFEE);
    let mut bytes = Vec::with_capacity(4096 * 8);
    for _ in 0..4096 {
        bytes.extend_from_slice(&rng.next_u64().to_le_bytes());
    }
    mb_seed::golden::check_hash(DIR, "rng_stream_00c0ffee", &bytes);
}

#[test]
fn golden_derive_path() {
    let paths: [&[(&str, &str)]; 8] = [
        &[("region", "north")],
        &[("region", "south")],
        &[("region", "north"), ("settlement", "village-1")],
        &[("region", "north"), ("settlement", "village-2")],
        &[
            ("world", "w"),
            ("region", "r"),
            ("district", "d"),
            ("lot", "l"),
        ],
        &[("stage", "terrain.heightfield")],
        &[("house", "h-1"), ("room", "kitchen")],
        &[("road", "r-7"), ("segment", "42")],
    ];
    let mut bytes = Vec::new();
    for p in paths {
        bytes.extend_from_slice(&derive_path(0xDEADBEEF, p).to_le_bytes());
    }
    mb_seed::golden::check_hash(DIR, "derive_paths", &bytes);
}

#[test]
fn rng_at_matches_stream_order() {
    let seed = 0x1234_5678_9ABC_DEF0;
    let mut rng = Rng::new(seed);
    for i in 0..1000u64 {
        assert_eq!(rng.next_u64(), Rng::at(seed, i));
    }
}

#[test]
fn fork_is_independent() {
    let a = Rng::new(1);
    let mut f1 = a.fork("region", "north");
    let mut f2 = a.fork("region", "south");
    let mut f1_again = a.fork("region", "north");
    assert_ne!(f1.next_u64(), f2.next_u64());
    // A fork is reproducible: same kind+key, same stream.
    let mut f1b = a.fork("region", "north");
    assert_eq!(f1_again.next_u64(), f1b.next_u64());
    // Forking does not disturb the parent stream.
    let mut p1 = Rng::new(1);
    let _ = p1.fork("region", "north");
    let mut p2 = Rng::new(1);
    assert_eq!(p1.next_u64(), p2.next_u64());
}

#[test]
fn entity_id_hex_round_trip() {
    let id = EntityId::from_path(42, &[("region", "r"), ("lot", "l-9")]);
    let s = id.to_string();
    assert_eq!(s.len(), 16);
    assert_eq!(s, s.to_lowercase());
    assert_eq!(s.parse::<EntityId>().unwrap(), id);
    assert_eq!("0000000000000000".parse::<EntityId>().unwrap(), EntityId(0));
    assert_eq!(
        "ffffffffffffffff".parse::<EntityId>().unwrap(),
        EntityId(u64::MAX)
    );
}

#[test]
fn derive_is_framed() {
    // Length prefixes prevent (kind, key) framing collisions.
    assert_ne!(derive(0, "ab", "c"), derive(0, "a", "bc"));
    assert_ne!(derive(0, "", "ab"), derive(0, "a", "b"));
}

#[test]
fn next_f64_range_and_determinism() {
    let mut a = Rng::new(7);
    let mut b = Rng::new(7);
    for _ in 0..100 {
        let x = a.next_f64();
        assert!((0.0..1.0).contains(&x));
        assert_eq!(x, b.next_f64());
    }
    for _ in 0..100 {
        let r = a.range_u64(97);
        assert!(r < 97);
        assert!(a.range_f64(-5.0, 5.0) >= -5.0);
    }
}
