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
                luxuries: Vec::new(),
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
                luxuries: Vec::new(),
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
                luxuries: Vec::new(),
            },
        ],
        materials: Vec::new(),
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
    // full == incremental: every house export byte-identical.
    let snapshot = |d: &Path| -> Vec<(String, String)> {
        let m: serde_json::Value =
            serde_json::from_slice(&std::fs::read(d.join("exports/houses/manifest.json")).unwrap())
                .unwrap();
        let mut files: Vec<String> = vec!["houses/manifest.json".into()];
        for h in m["houses"].as_array().unwrap() {
            for l in ["lod0", "lod1", "lod2"] {
                files.push(h["files"][l].as_str().unwrap().into());
            }
        }
        for p in m["parts"].as_array().unwrap() {
            for l in ["lod0", "lod1"] {
                files.push(p["files"][l].as_str().unwrap().into());
            }
        }
        files
            .into_iter()
            .map(|f| (f.clone(), hash(&d.join("exports").join(&f))))
            .collect()
    };
    let incremental = snapshot(&d);
    let _ = std::fs::remove_dir_all(d.join("exports"));
    let mut o2 = opts(&d);
    o2.full = true;
    let _ = build_project(&d, &o2, &mb_pipeline::CancelToken::new()).unwrap();
    assert_eq!(
        incremental,
        snapshot(&d),
        "full rebuild differs from incremental"
    );
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
    } // All part meshes the reference houses use, in manifest (key) order.
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d.join("exports/houses/manifest.json")).unwrap())
            .unwrap();
    let mut parts = Vec::new();
    for p in m["parts"].as_array().unwrap() {
        for l in ["lod0", "lod1"] {
            parts.extend(
                std::fs::read(d.join("exports").join(p["files"][l].as_str().unwrap())).unwrap(),
            );
        }
    }
    assert!(!parts.is_empty(), "reference houses use no parts");
    mb_seed::golden::check_hash(env!("CARGO_MANIFEST_DIR"), "house_parts", &parts);
}

#[test]
fn house_materials_exported_and_cleaned() {
    use mb_house::materials::{SLOTS, texture_png};
    let d = tmp("mat");
    mkproj(&d);
    bld(&d);
    let mp = d.join("exports/houses/manifest.json");
    let mut m: serde_json::Value = serde_json::from_slice(&std::fs::read(&mp).unwrap()).unwrap();
    let mats = m["materials"].as_array().unwrap().clone();
    assert_eq!(mats.len(), SLOTS.len());
    for (e, s) in mats.iter().zip(SLOTS) {
        assert_eq!(e["slot"], s.name());
        assert_eq!(e["source"], "procedural");
        assert_eq!(e["tile_m"].as_f64().unwrap(), s.tile_m());
        let f = d.join("exports").join(e["file"].as_str().unwrap());
        assert_eq!(std::fs::read(f).unwrap(), texture_png(s));
    }
    // A material file listed by the previous manifest but no longer
    // produced is removed on the next build.
    let stale = d.join("exports/houses/materials/plaster.jpg");
    std::fs::write(&stale, b"\xFF\xD8\xFFold").unwrap();
    m["materials"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"slot": "plaster", "file": "houses/materials/plaster.jpg"}));
    std::fs::write(&mp, serde_json::to_vec(&m).unwrap()).unwrap();
    let r = bld_rep(&d);
    assert!(
        !stale.exists(),
        "stale material kept; deleted {:?}",
        r.write.deleted_paths
    );
    // Same for a part GLB the previous manifest listed.
    let old_part = d.join("exports/houses/parts/zz_old/lod0.glb");
    std::fs::create_dir_all(old_part.parent().unwrap()).unwrap();
    std::fs::write(&old_part, b"glTF").unwrap();
    let mut m: serde_json::Value = serde_json::from_slice(&std::fs::read(&mp).unwrap()).unwrap();
    m["parts"].as_array_mut().unwrap().push(serde_json::json!({
        "key": "zz_old",
        "files": {"lod0": "houses/parts/zz_old/lod0.glb", "lod1": "houses/parts/zz_old/lod1.glb"}
    }));
    std::fs::write(&mp, serde_json::to_vec(&m).unwrap()).unwrap();
    bld(&d);
    assert!(!old_part.exists(), "stale part kept");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn house_material_override() {
    let d = tmp("matov");
    mkproj(&d);
    bld(&d);
    let glb = hash(&d.join("exports/houses/rect/lod0.glb"));
    let png = mb_house::materials::texture_png(mb_house::materials::TexSlot::Stone);
    std::fs::create_dir_all(d.join("assets/textures")).unwrap();
    std::fs::write(d.join("assets/textures/plaster.png"), &png).unwrap();
    let rp = d.join("recipes/houses.json");
    let set = |ov: serde_json::Value| {
        let mut r: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&rp).unwrap()).unwrap();
        r["materials"] = ov;
        std::fs::write(&rp, serde_json::to_string_pretty(&r).unwrap()).unwrap();
    };
    set(
        serde_json::json!([{"slot": "plaster", "texture": "assets/textures/plaster.png", "tile_m": 1.5}]),
    );
    let manifest = |d: &Path| -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(d.join("exports/houses/manifest.json")).unwrap())
            .unwrap()
    };
    let rep = bld_rep(&d);
    let m = manifest(&d);
    assert_eq!(m["materials"][0]["source"], "user");
    assert_eq!(m["materials"][0]["tile_m"], 1.5);
    assert_eq!(
        std::fs::read(d.join("exports/houses/materials/plaster.png")).unwrap(),
        png
    );
    for p in &rep.write.written_paths {
        assert!(
            p == "exports/houses/materials/plaster.png" || p == "exports/houses/manifest.json",
            "unexpected write {p}"
        );
    }
    // New user bytes (now a JPEG) re-run only the materials stage.
    std::fs::write(
        d.join("assets/textures/plaster.jpg"),
        b"\xFF\xD8\xFF\xE0fake",
    )
    .unwrap();
    set(
        serde_json::json!([{"slot": "plaster", "texture": "assets/textures/plaster.jpg", "tile_m": 1.5}]),
    );
    let rep = bld_rep(&d);
    assert!(d.join("exports/houses/materials/plaster.jpg").exists());
    assert!(!d.join("exports/houses/materials/plaster.png").exists());
    assert!(
        rep.write
            .deleted_paths
            .iter()
            .any(|p| p.ends_with("plaster.png"))
    );
    assert_eq!(hash(&d.join("exports/houses/rect/lod0.glb")), glb);
    // A non-image fails the build before anything is written.
    std::fs::write(d.join("assets/textures/plaster.jpg"), b"not an image").unwrap();
    let mut o = opts(&d);
    o.full = false;
    assert!(build_project(&d, &o, &mb_pipeline::CancelToken::new()).is_err());
    assert!(d.join("exports/houses/materials/plaster.jpg").exists());
    // Clearing restores the procedural texture and removes the user copy.
    set(serde_json::json!([]));
    bld(&d);
    assert_eq!(manifest(&d)["materials"][0]["source"], "procedural");
    assert!(!d.join("exports/houses/materials/plaster.jpg").exists());
    assert_eq!(
        std::fs::read(d.join("exports/houses/materials/plaster.png")).unwrap(),
        mb_house::materials::texture_png(mb_house::materials::TexSlot::Plaster)
    );
    let _ = std::fs::remove_dir_all(&d);
}
