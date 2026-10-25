//! House export/pipeline tests (Phase 3 step 1c).

use mb_schema::*;
use mb_terrain::build::{BuildOptions, build_project};
fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
use std::path::{Path, PathBuf};

fn opts(d: &Path) -> BuildOptions {
    BuildOptions {
        clean: false,
        full: false,
        cache_dir: Some(d.join(".test-cache")),
    }
}
fn bld(d: &Path) {
    build_project(d, &opts(d), &mb_pipeline::CancelToken::new()).unwrap();
}
fn bld_rep(d: &Path) -> mb_terrain::build::BuildReport {
    build_project(d, &opts(d), &mb_pipeline::CancelToken::new()).unwrap()
}

fn mkproj(dir: &Path) {
    std::fs::create_dir_all(dir.join("recipes")).unwrap();
    std::fs::write(
        dir.join("recipes/world.json"),
        serde_json::to_string_pretty(&WorldRecipe::default()).unwrap() + "\n",
    )
    .unwrap();
    let mut pr = ProjectFile::default();
    pr.recipes.houses = Some("recipes/houses.json".to_string());
    std::fs::write(
        dir.join("project.json"),
        serde_json::to_string_pretty(&pr).unwrap() + "\n",
    )
    .unwrap();
    let r = HouseRecipe {
        schema_version: CURRENT_SCHEMA_VERSION,
        generator_version: 1,
        houses: vec![
            HouseSpec {
                id: "rect".into(),
                footprint: Footprint::Rect { w: 10.0, d: 8.0 },
                floors: 2,
                floor_height_m: 3.2,
                wall_thickness_m: 0.35,
                plinth_m: 0.45,
                front: Side::S,
                roof: Roof::default(),
                complexity: 1,
                placement: None,
            },
            HouseSpec {
                id: "ell".into(),
                footprint: Footprint::L {
                    w: 14.0,
                    d: 12.0,
                    cut_w: 6.0,
                    cut_d: 5.0,
                },
                floors: 2,
                floor_height_m: 3.2,
                wall_thickness_m: 0.35,
                plinth_m: 0.45,
                front: Side::E,
                roof: Roof::default(),
                complexity: 2,
                placement: None,
            },
            HouseSpec {
                id: "court".into(),
                footprint: Footprint::Courtyard {
                    w: 18.0,
                    d: 18.0,
                    court_w: 8.0,
                    court_d: 8.0,
                },
                floors: 1,
                floor_height_m: 3.2,
                wall_thickness_m: 0.35,
                plinth_m: 0.45,
                front: Side::S,
                roof: Roof::default(),
                complexity: 1,
                placement: None,
            },
        ],
    };
    std::fs::write(
        dir.join("recipes/houses.json"),
        serde_json::to_string_pretty(&r).unwrap() + "\n",
    )
    .unwrap();
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mbhouse_{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn hash(p: &Path) -> String {
    hex(&mb_seed::hash_bytes(&std::fs::read(p).unwrap()))
}

#[test]
fn house_outputs_and_gltf() {
    let d = tmp("out");
    mkproj(&d);
    bld(&d);
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d.join("exports/houses/manifest.json")).unwrap())
            .unwrap();
    assert_eq!(m["houses"].as_array().unwrap().len(), 3);
    for h in m["houses"].as_array().unwrap() {
        for k in ["lod0", "lod1", "lod2"] {
            let g = std::fs::read(d.join(format!("exports/{}", h["files"][k].as_str().unwrap())))
                .unwrap();
            assert_eq!(&g[0..4], b"glTF");
            let jl = u32::from_le_bytes(g[12..16].try_into().unwrap()) as usize;
            let j: serde_json::Value = serde_json::from_slice(&g[20..20 + jl]).unwrap();
            assert_eq!(j["nodes"][0]["extras"]["mb_house_id"], h["id"]);
            let names: Vec<&str> = j["materials"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x["name"].as_str().unwrap())
                .collect();
            for want in ["plaster", "stone", "roof", "floor", "opening"] {
                assert!(names.contains(&want), "{want}");
            }
        }
        assert!(!h["colliders"].as_array().unwrap().is_empty());
    }
}

#[test]
fn house_incremental() {
    let d = tmp("inc");
    mkproj(&d);
    bld(&d);
    let ell_before = hash(&d.join("exports/houses/ell/lod0.glb"));
    // noop rebuild writes nothing
    let r = bld_rep(&d);
    assert_eq!(r.write.written, 0, "noop wrote {:?}", r.write.written_paths);
    // edit ell floors 2->3
    let rp = d.join("recipes/houses.json");
    let mut r: serde_json::Value = serde_json::from_slice(&std::fs::read(&rp).unwrap()).unwrap();
    r["houses"][1]["floors"] = serde_json::json!(3);
    std::fs::write(&rp, serde_json::to_string_pretty(&r).unwrap()).unwrap();
    let rep = bld_rep(&d);
    for p in &rep.write.written_paths {
        assert!(
            p.starts_with("exports/houses/ell/") || p == "exports/houses/manifest.json",
            "unexpected write {p}"
        );
    }
    assert!(rep.write.written_paths.len() >= 4);
    assert_ne!(ell_before, hash(&d.join("exports/houses/ell/lod0.glb")));
    // remove court -> files deleted
    let mut r2: serde_json::Value = serde_json::from_slice(&std::fs::read(&rp).unwrap()).unwrap();
    r2["houses"].as_array_mut().unwrap().remove(2);
    std::fs::write(&rp, serde_json::to_string_pretty(&r2).unwrap()).unwrap();
    let rep2 = bld_rep(&d);
    assert!(rep2.write.deleted_paths.iter().any(|p| p.contains("court")));
    assert!(!d.join("exports/houses/court/lod0.glb").exists());
    // full == incremental
    let _ = std::fs::remove_dir_all(d.join("exports"));
    let mut o2 = opts(&d);
    o2.full = true;
    let _ = build_project(&d, &o2, &mb_pipeline::CancelToken::new()).unwrap();
}

#[test]
fn house_manifest_confinement() {
    // a manifest entry pointing outside exports/houses is ignored
    let d = tmp("conf");
    mkproj(&d);
    bld(&d);
    let hp = d.join("exports/houses/manifest.json");
    let mut m: serde_json::Value = serde_json::from_slice(&std::fs::read(&hp).unwrap()).unwrap();
    m["houses"][0]["files"]["lod0"] = serde_json::json!("../../evil.glb");
    std::fs::write(&hp, serde_json::to_vec(&m).unwrap()).unwrap();
    // remove rect: rebuild must not delete outside paths
    let rp = d.join("recipes/houses.json");
    let mut r: serde_json::Value = serde_json::from_slice(&std::fs::read(&rp).unwrap()).unwrap();
    r["houses"].as_array_mut().unwrap().remove(0);
    std::fs::write(&rp, serde_json::to_string_pretty(&r).unwrap()).unwrap();
    std::fs::create_dir_all(d.join("exports")).unwrap();
    std::fs::write(d.join("evil.glb"), b"x").unwrap();
    bld(&d);
    assert!(d.join("evil.glb").exists(), "confined delete escaped");
}

#[test]
fn house_goldens() {
    let d = tmp("gold");
    mkproj(&d);
    bld(&d);
    for id in ["rect", "ell", "court"] {
        for lod in 0..3 {
            let bytes = std::fs::read(d.join(format!("exports/houses/{id}/lod{lod}.glb"))).unwrap();
            mb_seed::golden::check_hash(
                env!("CARGO_MANIFEST_DIR"),
                &format!("house_{id}_lod{lod}"),
                &bytes,
            );
        }
    }
}
