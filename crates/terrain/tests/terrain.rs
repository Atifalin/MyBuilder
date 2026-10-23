//! S3 tests on a SMALL world so debug runs stay fast:
//! 512x512 m, chunk 128, cell 1, seed 1, mesh_step 4, land blob r~180,
//! one mountain shape, one water hole.

use std::path::{Path, PathBuf};

use mb_pipeline::{Cache, Pipeline};
use mb_schema::{Extent, ProjectFile, Shape, ShapeKind, WorldRecipe};
use mb_terrain::outputs::{FileBundle, build_export_bundle, write_outputs};
use mb_terrain::stages::{ExportStage, HeightStage, MasksStage};
use mb_terrain::{
    Distances, Grid, HeightField, HeightInputs, Masks, build_distances, build_height, build_masks,
    dequantize, quant_step, quantize,
};

const DIR: &str = env!("CARGO_MANIFEST_DIR");

fn small_project() -> ProjectFile {
    ProjectFile {
        seed: 1,
        ..ProjectFile::default()
    }
}

fn small_world() -> WorldRecipe {
    WorldRecipe {
        extent: Extent {
            width_m: 512.0,
            depth_m: 512.0,
        },
        chunk_size_m: 128.0,
        shapes: vec![
            Shape {
                id: "coast".into(),
                kind: ShapeKind::Land,
                points: vec![
                    [180.0, 0.0],
                    [130.0, 130.0],
                    [0.0, 180.0],
                    [-130.0, 130.0],
                    [-180.0, 0.0],
                    [-130.0, -130.0],
                    [0.0, -180.0],
                    [130.0, -180.0],
                ],
            },
            Shape {
                id: "mt".into(),
                kind: ShapeKind::Mountain,
                points: vec![[20.0, 20.0], [110.0, 30.0], [100.0, 120.0], [10.0, 110.0]],
            },
            Shape {
                id: "pond".into(),
                kind: ShapeKind::Water,
                points: vec![
                    [-60.0, -60.0],
                    [-20.0, -60.0],
                    [-20.0, -20.0],
                    [-60.0, -20.0],
                ],
            },
        ],
        ..WorldRecipe::default()
    }
}

fn grid() -> Grid {
    Grid::new(512.0, 512.0, 1.0)
}

fn node_at(grid: &Grid, x: f64, z: f64) -> usize {
    let i = ((x - grid.origin_x) / grid.cell).round() as usize;
    let j = ((z - grid.origin_z) / grid.cell).round() as usize;
    j * grid.nx + i
}

fn computed() -> (Grid, Masks, Distances, HeightField) {
    let g = grid();
    let w = small_world();
    let masks = build_masks(&g, &w.shapes, w.mode);
    let dist = build_distances(&g, &masks);
    let terrain = w.terrain.feel.resolve(&w.terrain.overrides);
    let hf = build_height(&HeightInputs {
        grid: &g,
        masks: &masks,
        dist: &dist,
        sea_level_m: w.sea_level_m,
        project_seed: 1,
        terrain,
        profile: &w.profile,
    });
    (g, masks, dist, hf)
}

fn bundle() -> FileBundle {
    let (g, masks, dist, hf) = computed();
    let w = small_world();
    let p = small_project();
    build_export_bundle(&g, &p, &w, &hf, &masks, &dist.coast)
}

fn file<'a>(b: &'a FileBundle, name: &str) -> &'a [u8] {
    &b.files.iter().find(|(p, _)| p == name).unwrap().1
}

// ---------- goldens ----------

#[test]
fn golden_height_r16() {
    let b = bundle();
    mb_seed::golden::check_hash(DIR, "height_r16", file(&b, "layers/height.r16"));
}

#[test]
fn golden_chunk_glb() {
    let b = bundle();
    mb_seed::golden::check_hash(
        DIR,
        "chunk_0_0_glb",
        file(&b, "exports/terrain/chunk_0_0.glb"),
    );
}

#[test]
fn golden_manifest() {
    let b = bundle();
    mb_seed::golden::check_hash(DIR, "manifest", file(&b, "exports/terrain/manifest.json"));
}

// ---------- property tests ----------

#[test]
fn mask_land_water() {
    let (g, masks, _d, _h) = computed();
    // Center of the blob is land; a corner is sea; the pond is a hole.
    assert_eq!(masks.land[node_at(&g, 0.0, 0.0)], 1);
    assert_eq!(masks.land[node_at(&g, -240.0, -240.0)], 0);
    assert_eq!(masks.land[node_at(&g, -40.0, -40.0)], 0);
    assert_eq!(masks.mountain[node_at(&g, 60.0, 60.0)], 1);
    assert_eq!(masks.mountain[node_at(&g, 0.0, 0.0)], 0);
}

#[test]
fn coast_heights() {
    let (g, _m, dist, hf) = computed();
    // Well inland (d > 100) height exceeds sea level; far offshore it's below.
    let mut saw_inland = false;
    let mut saw_sea = false;
    for j in 0..g.nz {
        for i in 0..g.nx {
            let idx = j * g.nx + i;
            let d = dist.coast[idx];
            if d > 100.0 {
                assert!(hf.h[idx] > 0.0, "inland {d} gave h={}", hf.h[idx]);
                saw_inland = true;
            }
            if d < -100.0 {
                assert!(hf.h[idx] < 0.0, "offshore {d} gave h={}", hf.h[idx]);
                saw_sea = true;
            }
        }
    }
    assert!(saw_inland && saw_sea);
}

#[test]
fn quantize_round_trip() {
    let (_g, _m, _d, hf) = computed();
    let step = quant_step(hf.h_min, hf.h_max);
    for &h in hf.h.iter().step_by(997) {
        let q = quantize(h, hf.h_min, hf.h_max);
        let back = dequantize(q, hf.h_min, hf.h_max);
        assert!((h - back).abs() <= step / 2.0 + 1e-9, "{h} vs {back}");
    }
}

#[test]
fn chunk_seams_bit_identical() {
    let (g, masks, dist, hf) = computed();
    let normals = mb_terrain::mesh::normals(&g, &hf);
    let cells_per_chunk = 128usize;
    let step = 4usize;
    let _w = small_world();
    // Neighbor pair: chunks (0,0) and (1,0) share the border column.
    let left = mb_terrain::mesh::chunk_mesh(
        &g,
        &hf,
        &normals,
        &dist.coast,
        0.0,
        cells_per_chunk,
        step,
        0,
        0,
    );
    let right = mb_terrain::mesh::chunk_mesh(
        &g,
        &hf,
        &normals,
        &dist.coast,
        0.0,
        cells_per_chunk,
        step,
        1,
        0,
    );
    let vside = 128 / step + 1;
    for vj in 0..vside {
        let li = vj * vside + (vside - 1); // rightmost column of left
        let ri = vj * vside; // leftmost column of right
        let wp_l = [
            left.origin[0] + left.positions[li][0] as f64,
            left.positions[li][1] as f64,
            left.origin[2] + left.positions[li][2] as f64,
        ];
        let wp_r = [
            right.origin[0] + right.positions[ri][0] as f64,
            right.positions[ri][1] as f64,
            right.origin[2] + right.positions[ri][2] as f64,
        ];
        assert_eq!(wp_l, wp_r, "seam vertex {vj} position differs");
        assert_eq!(
            left.normals[li], right.normals[ri],
            "seam vertex {vj} normal differs"
        );
    }
    let _ = masks;
}

#[test]
fn glb_structure() {
    let b = bundle();
    let glb = file(&b, "exports/terrain/chunk_0_0.glb");
    assert_eq!(&glb[0..4], b"glTF");
    assert_eq!(u32::from_le_bytes(glb[4..8].try_into().unwrap()), 2);
    let total = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
    assert_eq!(total, glb.len());
    // JSON chunk.
    let jlen = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    assert_eq!(&glb[16..20], b"JSON");
    assert_eq!(jlen % 4, 0);
    let json: serde_json::Value = serde_json::from_slice(&glb[20..20 + jlen]).unwrap();
    // BIN chunk.
    let boff = 20 + jlen;
    let blen = u32::from_le_bytes(glb[boff..boff + 4].try_into().unwrap()) as usize;
    assert_eq!(&glb[boff + 4..boff + 8], b"BIN\0");
    assert_eq!(blen % 4, 0);
    assert_eq!(boff + 8 + blen, glb.len());
    // Accessor counts + bufferView ranges in bounds.
    let bvs = json["bufferViews"].as_array().unwrap();
    let bin_len = json["buffers"][0]["byteLength"].as_u64().unwrap() as usize;
    for (ai, acc) in json["accessors"].as_array().unwrap().iter().enumerate() {
        let bvi = acc["bufferView"].as_u64().unwrap() as usize;
        let bv = &bvs[bvi];
        let off = bv["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = bv["byteLength"].as_u64().unwrap() as usize;
        assert!(off + len <= bin_len, "accessor {ai} out of bounds");
        assert!(acc["count"].as_u64().unwrap() > 0);
    }
    // Primitive references exist.
    let prim = &json["meshes"][0]["primitives"][0];
    assert_eq!(prim["attributes"]["POSITION"], 0);
    assert_eq!(prim["indices"], 4);
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = Path::new(DIR)
        .join("../../target/s3-tests")
        .join(format!("{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn write_policy_idempotent() {
    let b = bundle();
    let dir = tmpdir("idem");
    let r1 = write_outputs(&dir, &b).unwrap();
    assert!(!r1.written.is_empty());
    let r2 = write_outputs(&dir, &b).unwrap();
    assert_eq!(r2.written.len(), 0, "second build must write nothing");
    assert_eq!(r2.unchanged.len(), r1.written.len());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_policy_partial_rebuild() {
    let dir = tmpdir("partial");
    write_outputs(&dir, &bundle()).unwrap();
    // Same world but a different terrain feel -> heights, chunks,
    // preview and manifest change; masks stay identical.
    let mut w = small_world();
    w.terrain.feel = mb_schema::TerrainFeel::Mountainous;
    let (g, masks, dist, _hf0) = computed();
    let terrain = w.terrain.feel.resolve(&w.terrain.overrides);
    let hf = build_height(&HeightInputs {
        grid: &g,
        masks: &masks,
        dist: &dist,
        sea_level_m: w.sea_level_m,
        project_seed: 1,
        terrain,
        profile: &w.profile,
    });
    let b2 = build_export_bundle(&g, &small_project(), &w, &hf, &masks, &dist.coast);
    let r = write_outputs(&dir, &b2).unwrap();
    assert!(
        r.written
            .contains(&"exports/terrain/manifest.json".to_string())
    );
    assert!(r.written.contains(&"layers/height.r16".to_string()));
    assert!(r.written.iter().any(|f| f.ends_with(".glb")));
    assert!(r.unchanged.contains(&"layers/land.u8".to_string()));
    assert!(r.unchanged.contains(&"layers/mountain.u8".to_string()));
    assert!(r.unchanged.contains(&"layers/forest.u8".to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- pipeline plumbing ----------

#[test]
fn pipeline_stages_produce_bundle() {
    let g = grid();
    let p = small_project();
    let w = small_world();
    let mut pipe = Pipeline::new(p.seed);
    pipe.add(Box::new(MasksStage {
        code_version: 1,
        grid: g,
        mode: w.mode,
        shapes: w.shapes.clone(),
        cell_size_m: 1.0,
        extent: w.extent,
    }));
    pipe.add(Box::new(mb_terrain::stages::DistanceStage::new(g, 1.0, 1)));
    pipe.add(Box::new(HeightStage::new(g, p.clone(), w.clone(), 1)));
    pipe.add(Box::new(ExportStage::new(g, p, w, 1)));
    let mut cache = Cache::default();
    let r = pipe.run(&mut cache).unwrap();
    assert_eq!(r.executed.len(), 4);
    let pipe_bundle = FileBundle::decode(&cache.get("world.export").unwrap().bytes);
    let direct = bundle();
    assert_eq!(pipe_bundle.files.len(), direct.files.len());
    // Same inputs -> identical bytes through the pipeline path too.
    assert_eq!(
        mb_seed::hash_bytes(file(&pipe_bundle, "layers/height.r16")),
        mb_seed::hash_bytes(file(&direct, "layers/height.r16")),
    );
}
