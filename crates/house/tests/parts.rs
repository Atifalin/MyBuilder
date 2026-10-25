use mb_house::parts::{PartKind, build_part, part_key};
use mb_house::*;

const ALL: [(PartKind, &str); 8] = [
    (PartKind::DoorLeaf, "d"),
    (PartKind::Chhatri, "s1.6"),
    (PartKind::Chhatri, "s2"),
    (PartKind::Chhatri, "s2.4"),
    (PartKind::Jharokha, "w1.2"),
    (PartKind::Jharokha, "w1.5"),
    (PartKind::Jharokha, "w1.8"),
    (PartKind::Bracket, "b"),
];

fn tris(l: &Lod) -> usize {
    l.prims.iter().map(|p| p.indices.len() / 3).sum()
}

fn bytes(l: &Lod) -> Vec<u8> {
    let mut v = Vec::new();
    for p in &l.prims {
        for q in &p.positions {
            for c in q {
                v.extend_from_slice(&c.to_le_bytes());
            }
        }
        for i in &p.indices {
            v.extend_from_slice(&i.to_le_bytes());
        }
    }
    v
}

#[test]
fn part_meshes_are_well_formed() {
    for (kind, params) in ALL {
        for lod1 in [false, true] {
            let l = build_part(kind, params, lod1);
            assert!(tris(&l) > 0, "{kind:?} {params} empty");
            for p in &l.prims {
                for t in p.indices.chunks(3) {
                    let [a, b, c] = [0, 1, 2].map(|k| p.positions[t[k] as usize]);
                    let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                    let cr = [
                        e1[1] * e2[2] - e1[2] * e2[1],
                        e1[2] * e2[0] - e1[0] * e2[2],
                        e1[0] * e2[1] - e1[1] * e2[0],
                    ];
                    let n = p.normals[t[0] as usize];
                    let nl = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    assert!((nl - 1.0).abs() < 1e-4, "{kind:?} {params} normal {n:?}");
                    let dot = cr[0] * n[0] + cr[1] * n[1] + cr[2] * n[2];
                    assert!(
                        dot > 0.0,
                        "{kind:?} {params} lod1={lod1}: triangle against its normal"
                    );
                }
            }
        }
        let (l0, l1) = (
            build_part(kind, params, false),
            build_part(kind, params, true),
        );
        assert!(
            tris(&l1) <= tris(&l0),
            "{kind:?} {params}: lod1 {} > lod0 {}",
            tris(&l1),
            tris(&l0)
        );
        assert_eq!(
            bytes(&l0),
            bytes(&build_part(kind, params, false)),
            "deterministic"
        );
        assert_eq!(part_key(kind, params), part_key(kind, params));
    }
}

#[test]
fn keys_are_distinct_per_bucket() {
    let mut keys: Vec<String> = ALL.iter().map(|(k, p)| part_key(*k, p)).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), ALL.len());
}
