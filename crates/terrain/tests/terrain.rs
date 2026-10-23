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
    build_export_bundle(&g, &p, &w, &hf, &masks, &dist.coast, &[])
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
        300.0,
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
        300.0,
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

struct TestDir(PathBuf);
impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl std::ops::Deref for TestDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn tmpdir(tag: &str) -> TestDir {
    let d = Path::new(DIR)
        .join("../../target/s3-tests")
        .join(format!("{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    TestDir(d)
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
    let b2 = build_export_bundle(&g, &small_project(), &w, &hf, &masks, &dist.coast, &[]);
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
    let mut pipe = Pipeline::new(p.seed, p.generator_version);
    pipe.add(Box::new(MasksStage {
        code_version: 1,
        grid: g,
        mode: w.mode,
        shapes: w.shapes.clone(),
        cell_size_m: 1.0,
        extent: w.extent,
        generator_version: p.generator_version,
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

// ---------- S4 detail regions ----------

/// Small world plus a region covering coarse chunks [1,1]..[2,2]
/// (world x/z in [-128, 128]), divisor 2, fine mesh_step 1, band 16 m.
fn world_with_region() -> WorldRecipe {
    let mut w = small_world();
    w.detail_regions = vec![mb_schema::DetailRegion {
        id: "village-1".into(),
        chunk_min: [1, 1],
        chunk_max: [2, 2],
        cell_divisor: 2,
        mesh_step_cells: 1,
        extra_octaves: 2,
        blend_band_m: 16.0,
    }];
    w
}

struct RegionSetup {
    ctx: mb_terrain::HeightCtx,
    hf: HeightField,
    normals: Vec<[f32; 3]>,
    rb: mb_terrain::region::RegionBuild,
    rnorms: Vec<[f32; 3]>,
    region: mb_schema::DetailRegion,
}

fn region_setup() -> RegionSetup {
    let (g, _masks, dist, hf) = computed();
    let w = world_with_region();
    region_setup_for(g, &dist, hf, &w)
}

fn region_setup_for(g: Grid, dist: &Distances, hf: HeightField, w: &WorldRecipe) -> RegionSetup {
    let terrain = w.terrain.feel.resolve(&w.terrain.overrides);
    let ctx = mb_terrain::HeightCtx::new(g, dist, w.sea_level_m, 1, terrain, w.profile);
    let normals = mb_terrain::mesh::normals(&g, &hf);
    let region = w.detail_regions[0].clone();
    let rb = mb_terrain::region::build_region_height(&ctx, &hf, &region, 128, 4);
    let rnorms = mb_terrain::region::region_normals(&ctx, &normals, &rb, 4);
    RegionSetup {
        ctx,
        hf,
        normals,
        rb,
        rnorms,
        region,
    }
}

/// Fine-grid index for a fine node at the given distance (in meters)
/// inside the region along +x on the border row z = -128.
fn fine_idx(rb: &mb_terrain::region::RegionBuild, x: f64, z: f64) -> (usize, usize) {
    let i = ((x - rb.fine.origin_x) / rb.fine.spacing).round() as usize;
    let j = ((z - rb.fine.origin_z) / rb.fine.spacing).round() as usize;
    (i, j)
}

#[test]
fn region_border_bit_equal_to_coarse() {
    let s = region_setup();
    let g = s.ctx.grid;
    // Coarse chunk (1,1) exported mesh.
    let coarse = mb_terrain::mesh::chunk_mesh(
        &g,
        &s.hf,
        &s.normals,
        &s.ctx.coast,
        0.0,
        300.0,
        128,
        4,
        1,
        1,
    );
    // Fine chunk (1,1) of the region.
    let fine = mb_terrain::region::region_chunk_mesh(
        &s.ctx, &s.region, 128, &s.rb, &s.rnorms, 0.0, 300.0, 1, 1,
    );
    let fvside = 128 * 2 + 1; // per side at fine mesh_step 1
    let cvside = 128 / 4 + 1;
    // Region border for chunk (1,1) is its west edge: fine col 0 of the
    // region grid = x = -128, coinciding with coarse mesh vertices every
    // 4 cells = every 8 fine verts.
    for cv in 0..cvside {
        let ci = cv * cvside; // west column of coarse chunk 1_1
        let fi_row = cv * 4 * 2; // fine row index (v direction): 4 coarse cells * div 2
        let fi = fi_row * fvside;
        let cwx = coarse.origin[0] + coarse.positions[ci][0] as f64;
        let cwz = coarse.origin[2] + coarse.positions[ci][2] as f64;
        let fwx = fine.origin[0] + fine.positions[fi][0] as f64;
        let fwz = fine.origin[2] + fine.positions[fi][2] as f64;
        assert_eq!(cwx, fwx, "border vertex {cv} x differs");
        assert_eq!(cwz, fwz, "border vertex {cv} z differs");
        assert_eq!(
            coarse.positions[ci][1].to_bits(),
            fine.positions[fi][1].to_bits(),
            "border vertex {cv} height not bit-equal"
        );
        assert_eq!(
            coarse.normals[ci], fine.normals[fi],
            "border vertex {cv} normal not bit-equal"
        );
    }
    let _ = s;
}

#[test]
fn region_border_inbetweens_on_edge() {
    let s = region_setup();
    let g = s.ctx.grid;
    let region = &s.region;
    let rb = &s.rb;
    // West border x = -128: fine nodes between coarse mesh vertices.
    for fv in 1..8usize {
        // fine vertex 0.5m*fv south of coarse vertex at -128,-128
        let x = -128.0;
        let z = -128.0 + fv as f64 * 0.5;
        let (i, j) = fine_idx(rb, x, z);
        let h = rb.h[j * rb.fine.nx + i];
        let (cx0, cz0) = (-128.0, -128.0);
        let t = (z - cz0) / 4.0;
        let ha = mb_terrain::region::coarse_mesh_height(&g, &s.hf, 4, cx0, cz0);
        let hb = mb_terrain::region::coarse_mesh_height(&g, &s.hf, 4, cx0, cz0 + 4.0);
        let expected = ha + t * (hb - ha);
        assert!(
            (h - expected).abs() < 1e-4,
            "border vertex at z={z}: h={h} expected~{expected}"
        );
        // Normal equals n_coarse (w = 0 on the border).
        let nc = mb_terrain::region::coarse_mesh_normal(&g, &s.normals, 4, x, z);
        let n = s.rnorms[j * rb.fine.nx + i];
        for k in 0..3 {
            assert!(
                (n[k] as f64 - nc[k]).abs() < 1e-3,
                "normal mismatch on border at z={z}"
            );
        }
        let _ = region;
    }
}

#[test]
fn region_normal_continuity() {
    let s = region_setup();
    let rb = &s.rb;
    // Walk inward along x at fixed z through the blend band; adjacent
    // fine normals must not jump. Bound: 0.35 rad (~20 deg) — generous
    // for a blend over 16 m at 0.5 m spacing where detail noise adds at
    // most a few degrees per step; a real discontinuity would be >>1 rad.
    let z = 0.0;
    let j = ((z - rb.fine.origin_z) / rb.fine.spacing).round() as usize;
    let i0 = ((-128.0 - rb.fine.origin_x) / rb.fine.spacing).round() as usize;
    let i1 = ((-96.0 - rb.fine.origin_x) / rb.fine.spacing).round() as usize;
    for i in i0 + 1..i1 {
        let a = s.rnorms[j * rb.fine.nx + i - 1];
        let b = s.rnorms[j * rb.fine.nx + i];
        let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
        let ang = (dot as f64).acos();
        assert!(ang < 0.35, "normal jump {ang} rad at fine i={i}");
    }
    let _ = s;
}

#[test]
fn height_at_bilinear_exact_and_continuous() {
    let (g, _m, dist, hf) = computed();
    let w = world_with_region();
    let terrain = w.terrain.feel.resolve(&w.terrain.overrides);
    let ctx = mb_terrain::HeightCtx::new(g, &dist, w.sea_level_m, 1, terrain, w.profile);
    // At coarse nodes, height_at equals the coarse grid bit-for-bit.
    for j in (0..g.nz).step_by(37) {
        for i in (0..g.nx).step_by(37) {
            assert_eq!(
                ctx.height_at(g.x(i), g.z(j)).to_bits(),
                hf.h[j * g.nx + i].to_bits(),
                "height_at not bit-exact at node ({i},{j})"
            );
        }
    }
    // Continuity: 0.05 m steps along a diagonal crossing several cells.
    const MAX_SLOPE: f64 = 2.0;
    let mut prev = ctx.height_at(-200.0, -200.0);
    for k in 1..400 {
        let x = -200.0 + k as f64 * 0.05;
        let z = -200.0 + k as f64 * 0.05;
        let h = ctx.height_at(x, z);
        assert!(
            (h - prev).abs() <= MAX_SLOPE * 0.05 + 1e-9,
            "jump {} at ({x},{z})",
            h - prev
        );
        prev = h;
    }
}

#[test]
fn region_interior_matches_true() {
    let s = region_setup();
    let rb = &s.rb;
    let region = &s.region;
    let base_oct = s.ctx.fbm_params.octaves;
    let norm_n = mb_geom::fbm_norm(&s.ctx.fbm_params, base_oct);
    // Center of the region is far past the 16 m band -> w = 1.
    for (x, z) in [(0.0, 0.0), (20.0, -30.0), (-50.0, 60.0)] {
        let (i, j) = fine_idx(rb, x, z);
        let idx = j * rb.fine.nx + i;
        assert_eq!(rb.w[idx], 1.0, "w != 1 at interior ({x},{z})");
        let d = s.ctx.coast_at(x, z);
        let detail = s.ctx.terrain.relief_m
            * s.ctx.amplitude(d)
            * mb_geom::fbm_octave_sum(
                &s.ctx.perlin_f,
                x,
                z,
                &s.ctx.fbm_params,
                base_oct,
                base_oct + region.extra_octaves,
            )
            / norm_n;
        let htrue = s.ctx.height_at(x, z) + detail;
        assert_eq!(rb.h[idx], htrue, "interior h_final != h_true");
    }
}

// ---- region output plumbing ----

fn region_bundle() -> FileBundle {
    let (g, masks, dist, hf) = computed();
    let w = world_with_region();
    let p = small_project();
    let region = &w.detail_regions[0];
    let terrain = w.terrain.feel.resolve(&w.terrain.overrides);
    let ctx = mb_terrain::HeightCtx::new(g, &dist, w.sea_level_m, 1, terrain, w.profile);
    let normals = mb_terrain::mesh::normals(&g, &hf);
    let rb = mb_terrain::region::build_region_height(&ctx, &hf, region, 128, 4);
    let rnorms = mb_terrain::region::region_normals(&ctx, &normals, &rb, 4);
    let mut files = Vec::new();
    let mut entries = Vec::new();
    for cz in 1u32..=2 {
        for cx in 1u32..=2 {
            let mesh = mb_terrain::region::region_chunk_mesh(
                &ctx,
                region,
                128,
                &rb,
                &rnorms,
                w.sea_level_m,
                w.profile.snow_line_m,
                cx as usize,
                cz as usize,
            );
            let id = mb_seed::EntityId::from_path(
                1,
                &[
                    ("world", "main"),
                    ("region", "village-1"),
                    ("chunk", &format!("{cx}_{cz}")),
                ],
            );
            let glb = mb_terrain::glb::chunk_glb(
                &mesh,
                &format!("chunk_{cx}_{cz}"),
                &id.to_string(),
                cx as usize,
                cz as usize,
            );
            entries.push(serde_json::json!({
                "id": id.to_string(), "cx": cx, "cz": cz, "region": region.id,
                "file": format!("regions/{}/chunk_{cx}_{cz}.glb", region.id),
                "origin": mesh.origin,
                "vertex_count": mesh.vertex_count,
                "triangle_count": mesh.triangle_count,
                "blake3": mb_seed::hex_bytes(&mb_seed::hash_bytes(&glb)),
            }));
            files.push((
                format!("exports/terrain/regions/village-1/chunk_{cx}_{cz}.glb"),
                glb,
            ));
        }
    }
    let ro = mb_terrain::outputs::RegionOutput {
        region: region.clone(),
        entries: serde_json::Value::Array(entries),
        files,
    };
    build_export_bundle(&g, &p, &w, &hf, &masks, &dist.coast, &[ro])
}

#[test]
fn golden_region_chunk() {
    let b = region_bundle();
    mb_seed::golden::check_hash(
        DIR,
        "region_chunk_1_1_glb",
        file(&b, "exports/terrain/regions/village-1/chunk_1_1.glb"),
    );
}

#[test]
fn golden_manifest_with_region() {
    let b = region_bundle();
    mb_seed::golden::check_hash(
        DIR,
        "manifest_region",
        file(&b, "exports/terrain/manifest.json"),
    );
    // Superseded markers present.
    let m: serde_json::Value =
        serde_json::from_slice(file(&b, "exports/terrain/manifest.json")).unwrap();
    let sup = m["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c.get("superseded_by").is_some())
        .count();
    assert_eq!(sup, 4);
    assert_eq!(m["regions"][0]["id"], "village-1");
}

/// S10: the sliced region-chunk path must be byte-identical to the
/// whole-region path it replaced.
#[test]
fn sliced_region_chunk_matches_full_region() {
    let (g, _m, dist, hf) = computed();
    let w = world_with_region();
    let region = &w.detail_regions[0];
    let normals_full = mb_terrain::mesh::normals(&g, &hf);
    let terrain = w.terrain.feel.resolve(&w.terrain.overrides);
    let ctx = mb_terrain::HeightCtx::new(g, &dist, w.sea_level_m, 1, terrain, w.profile);
    let rb = mb_terrain::region::build_region_height(&ctx, &hf, region, 128, 4);
    let rnorms = mb_terrain::region::region_normals(&ctx, &normals_full, &rb, 4);
    for (cx, cz) in [(1usize, 1usize), (2, 1), (1, 2), (2, 2)] {
        let old = mb_terrain::region::region_chunk_mesh(
            &ctx,
            region,
            128,
            &rb,
            &rnorms,
            w.sea_level_m,
            w.profile.snow_line_m,
            cx,
            cz,
        );
        let key = format!("{cx}_{cz}");
        let id = mb_seed::EntityId::from_path(
            1,
            &[("world", "main"), ("region", &region.id), ("chunk", &key)],
        );
        let old_glb =
            mb_terrain::glb::chunk_glb(&old, &format!("chunk_{cx}_{cz}"), &id.to_string(), cx, cz);
        // Sliced path: same GLB from the window slice.
        let slice = mb_terrain::slice::encode_region_slice(
            &g,
            &hf,
            &dist.coast,
            &dist.mountain,
            128,
            4,
            cx,
            cz,
        );
        let rs = mb_terrain::slice::decode_region_slice(&slice);
        let art = mb_terrain::region_slice::run_region_chunk(&g, &rs, &w, region, 1, cx, cz);
        assert_eq!(
            art.bytes[mb_terrain::stages::CHUNK_HEADER..],
            old_glb[..],
            "chunk {cx},{cz}: sliced != whole-region"
        );
    }
    // And the coarse chunk slice path matches chunk_mesh + glb.
    let cs = mb_terrain::slice::encode_chunk_slice(&g, &hf, &dist.coast, 128, 0, 0);
    let cdec = mb_terrain::slice::decode_chunk_slice(&cs);
    let h_at = |i: usize, j: usize| cdec.h_at(i, j);
    let n_at =
        |i: usize, j: usize| mb_terrain::mesh::node_normal(&h_at, i, j, g.nx - 1, g.nz - 1, g.cell);
    let c_at = |i: usize, j: usize| cdec.coast_at(i, j);
    let new_mesh =
        mb_terrain::mesh::chunk_mesh_view(&g, h_at, n_at, c_at, 0.0, 300.0, 128, 4, 0, 0);
    let old_mesh = mb_terrain::mesh::chunk_mesh(
        &g,
        &hf,
        &normals_full,
        &dist.coast,
        0.0,
        300.0,
        128,
        4,
        0,
        0,
    );
    let id = mb_seed::EntityId::from_path(1, &[("world", "main"), ("chunk", "0_0")]);
    assert_eq!(
        mb_terrain::glb::chunk_glb(&new_mesh, "chunk_0_0", &id.to_string(), 0, 0),
        mb_terrain::glb::chunk_glb(&old_mesh, "chunk_0_0", &id.to_string(), 0, 0),
        "sliced coarse chunk != full-grid chunk"
    );
}
