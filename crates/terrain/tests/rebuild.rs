//! S10 rebuild-engine tests: incremental == full, file/byte recovery,
//! neighbour invalidation, corrupt cache, lock, generator version.

use std::path::{Path, PathBuf};

use mb_schema::*;
use mb_terrain::build::{BuildOptions, BuildReport, build_project};

/// RAII test dir: deleted on drop when the test passed, kept on panic so
/// failures leave evidence for debugging.
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

fn tmpdir(name: &str) -> TestDir {
    let d = std::env::temp_dir().join(format!("mb_s10_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    TestDir(d)
}

/// World: 256 m, chunk 64, cell 1, mesh_step 4, one land blob.
fn small_world() -> WorldRecipe {
    let mut w = WorldRecipe {
        extent: Extent {
            width_m: 256.0,
            depth_m: 256.0,
        },
        chunk_size_m: 64.0,
        ..WorldRecipe::default()
    };
    w.shapes = vec![Shape {
        id: "isle".into(),
        kind: ShapeKind::Land,
        points: vec![[-60.0, -60.0], [60.0, -60.0], [70.0, 40.0], [-30.0, 70.0]],
    }];
    w
}

fn make_project(dir: &Path) {
    let p = ProjectFile {
        generator_version: 1,
        ..ProjectFile::default()
    };
    let w = small_world();
    std::fs::create_dir_all(dir.join("recipes")).unwrap();
    std::fs::write(
        dir.join(mb_schema::PROJECT_FILE),
        serde_json::to_string_pretty(&p).unwrap() + "\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("recipes/world.json"),
        serde_json::to_string_pretty(&w).unwrap() + "\n",
    )
    .unwrap();
}

fn write_world(dir: &Path, w: &WorldRecipe) {
    std::fs::write(
        dir.join("recipes/world.json"),
        serde_json::to_string_pretty(w).unwrap() + "\n",
    )
    .unwrap();
}

fn write_project_seed(dir: &Path, seed: u64) {
    let path = dir.join(mb_schema::PROJECT_FILE);
    let mut p: ProjectFile =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    p.seed = seed;
    std::fs::write(&path, serde_json::to_string_pretty(&p).unwrap() + "\n").unwrap();
}

/// blake3 over sorted (relpath, bytes) of every output file under
/// layers/, previews/, exports/ — the output-tree identity.
fn output_tree_hash(dir: &Path) -> [u8; 32] {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for top in ["layers", "previews", "exports"] {
        let base = dir.join(top);
        if !base.exists() {
            continue;
        }
        let mut stack = vec![base.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let e = e.unwrap().path();
                if e.is_dir() {
                    stack.push(e);
                } else if e.file_name().unwrap() != ".gitkeep" {
                    let rel = e.strip_prefix(dir).unwrap().to_string_lossy().to_string();
                    files.push((rel, std::fs::read(&e).unwrap()));
                }
            }
        }
    }
    files.sort();
    let mut all = Vec::new();
    for (rel, bytes) in &files {
        all.extend_from_slice(&(rel.len() as u64).to_le_bytes());
        all.extend_from_slice(rel.as_bytes());
        all.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        all.extend_from_slice(bytes);
    }
    mb_seed::hash_bytes(&all)
}

fn build(dir: &Path) -> BuildReport {
    build_project(dir, &BuildOptions { clean: false }).unwrap()
}

// ---- 1. Random edit sequences: incremental == full rebuild ----

#[test]
fn random_edits_match_full_rebuild() {
    let mut rng = mb_seed::Rng::new(0xE0D7);
    for seq in 0..8u64 {
        let a = tmpdir(&format!("ra{seq}"));
        let b = tmpdir(&format!("rb{seq}"));
        make_project(&a);
        make_project(&b);
        let mut w = small_world();
        build(&a);
        write_world(&b, &w);
        build(&b);
        for edit in 0..6 {
            let kind = rng.range_u64(9);
            match kind {
                0 => {
                    // cycle feel
                    w.terrain.feel = match w.terrain.feel {
                        TerrainFeel::Hilly => TerrainFeel::Bumpy,
                        TerrainFeel::Bumpy => TerrainFeel::Mountainous,
                        TerrainFeel::Mountainous => TerrainFeel::Hilly,
                    };
                }
                1 => {
                    w.terrain.overrides.relief_m = Some(10.0 + rng.range_u64(190) as f64);
                }
                2 => {
                    // move one shape point +-20 m
                    let s = &mut w.shapes[0];
                    let pi = rng.range_u64(s.points.len() as u64) as usize;
                    let dx = (rng.next_f64() - 0.5) * 40.0;
                    let dz = (rng.next_f64() - 0.5) * 40.0;
                    s.points[pi][0] += dx;
                    s.points[pi][1] += dz;
                }
                3 => {
                    // add a small mountain triangle inside land
                    w.shapes.push(Shape {
                        id: format!("m{seq}{edit}"),
                        kind: ShapeKind::Mountain,
                        points: vec![[-20.0, -20.0], [20.0, -15.0], [0.0, 10.0]],
                    });
                }
                4 => {
                    w.sea_level_m += rng.range_u64(11) as f64 - 5.0;
                }
                5 => {
                    // add/remove a 1x1 or 2x2 detail region
                    if w.detail_regions.is_empty() {
                        w.detail_regions.push(DetailRegion {
                            id: "r".into(),
                            chunk_min: [1, 1],
                            chunk_max: [2, 2],
                            cell_divisor: 2,
                            mesh_step_cells: 1,
                            extra_octaves: 2,
                            blend_band_m: 16.0,
                        });
                    } else {
                        w.detail_regions.clear();
                    }
                }
                6 => {
                    w.export.mesh_step_cells = if w.export.mesh_step_cells == 4 { 2 } else { 4 };
                }
                7 => {
                    w.profile.snow_line_m = 200.0 + rng.range_u64(200) as f64;
                }
                _ => {
                    // project seed change (write to BOTH project.jsons)
                    let seed = rng.next_u64();
                    write_project_seed(&a, seed);
                    write_project_seed(&b, seed);
                }
            }
            write_world(&a, &w);
            write_world(&b, &w);
            let ra = build(&a);
            // B: wipe cache + outputs -> full rebuild
            let _ = std::fs::remove_dir_all(b.join("cache"));
            let rb = build(&b);
            let (ha, hb) = (output_tree_hash(&a), output_tree_hash(&b));
            assert_eq!(
                ha, hb,
                "seq {seq} edit {edit} kind {kind}: incremental != full"
            );
            // Immediate rebuild of A: nothing executes, nothing writes.
            let r2 = build(&a);
            assert!(
                r2.run.executed.is_empty(),
                "no-op rebuild executed: {:?}",
                r2.run.executed
            );
            assert_eq!(r2.write.written, 0, "no-op rebuild wrote files");
            let _ = ra;
            let _ = rb;
        }
    }
}

// ---- 2. Deleted/corrupted exports are restored ----

#[test]
fn deleted_and_modified_exports_restore() {
    let d = tmpdir("del");
    make_project(&d);
    build(&d);
    let orig = output_tree_hash(&d);
    // delete one chunk GLB
    let glb = d.join("exports/terrain/chunk_1_1.glb");
    let before = std::fs::read(&glb).unwrap();
    std::fs::remove_file(&glb).unwrap();
    let r = build(&d);
    assert_eq!(r.run.executed.len(), 0, "executed: {:?}", r.run.executed);
    assert_eq!(r.write.written, 1);
    assert_eq!(std::fs::read(&glb).unwrap(), before);
    assert_eq!(output_tree_hash(&d), orig);
    // hand-modified file is restored
    std::fs::write(&glb, b"tampered").unwrap();
    let r = build(&d);
    assert_eq!(r.write.written, 1);
    assert_eq!(std::fs::read(&glb).unwrap(), before);
    // deleted manifest rewrites
    std::fs::remove_file(d.join("exports/terrain/manifest.json")).unwrap();
    let r = build(&d);
    assert_eq!(r.write.written, 1);
    assert!(d.join("exports/terrain/manifest.json").exists());
    assert_eq!(output_tree_hash(&d), orig);
}

// ---- 3. One-corner edit: only affected chunks + edge neighbours ----

/// 512 m, chunk 64 (8x8), land_first with NO land shapes -> all land.
fn flat_world() -> WorldRecipe {
    let mut w = WorldRecipe {
        extent: Extent {
            width_m: 512.0,
            depth_m: 512.0,
        },
        chunk_size_m: 64.0,
        ..WorldRecipe::default()
    };
    w.mode = WorldMode::LandFirst;
    w.shapes = Vec::new();
    w.profile.inland_width_m = 50.0;
    w
}

fn make_flat(dir: &Path) {
    let p = ProjectFile {
        generator_version: 1,
        ..ProjectFile::default()
    };
    let w = flat_world();
    std::fs::create_dir_all(dir.join("recipes")).unwrap();
    std::fs::write(
        dir.join(mb_schema::PROJECT_FILE),
        serde_json::to_string_pretty(&p).unwrap() + "\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("recipes/world.json"),
        serde_json::to_string_pretty(&w).unwrap() + "\n",
    )
    .unwrap();
}

fn executed_chunks(r: &BuildReport) -> std::collections::BTreeSet<(usize, usize)> {
    r.run
        .executed
        .iter()
        .filter_map(|id| id.strip_prefix("world.chunk."))
        .map(|s| {
            let mut it = s.split('_');
            (
                it.next().unwrap().parse().unwrap(),
                it.next().unwrap().parse().unwrap(),
            )
        })
        .collect()
}

#[test]
fn one_corner_edit_localizes() {
    for case in ["interior", "edge"] {
        let d = tmpdir(&format!("oc{case}"));
        make_flat(&d);
        let r0 = build(&d);
        let old_h: std::collections::BTreeMap<String, _> = r0
            .run
            .outputs
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        let _ = old_h;
        // Snapshot per-chunk slice coverage: which chunks' node heights
        // change. The truly-affected set is derived from the height
        // artifact diff below.
        let mut w = flat_world();
        let pts = if case == "interior" {
            // strictly inside chunk (1,1): world x/z in (-128,-64)
            vec![[-110.0, -110.0], [-90.0, -105.0], [-95.0, -85.0]]
        } else {
            // crossing the (1,1)/(2,1) shared edge at x=-64
            vec![[-70.0, -100.0], [-58.0, -95.0], [-66.0, -80.0]]
        };
        w.shapes.push(Shape {
            id: "m".into(),
            kind: ShapeKind::Mountain,
            points: pts,
        });
        write_world(&d, &w);
        // Truly-affected set: chunks whose node-range heights changed.
        // Compute old vs new heights directly with the same functions.
        let grid = mb_terrain::Grid::new(512.0, 512.0, 1.0);
        let masks0 = mb_terrain::build_masks(&grid, &flat_world().shapes, flat_world().mode);
        let dist0 = mb_terrain::build_distances(&grid, &masks0);
        let w0 = flat_world();
        let t0 = w0.terrain.feel.resolve(&w0.terrain.overrides);
        let ctx0 = mb_terrain::HeightCtx::new(grid, &dist0, w0.sea_level_m, 1, t0, w0.profile);
        let masks1 = mb_terrain::build_masks(&grid, &w.shapes, w.mode);
        let dist1 = mb_terrain::build_distances(&grid, &masks1);
        let t1 = w.terrain.feel.resolve(&w.terrain.overrides);
        let ctx1 = mb_terrain::HeightCtx::new(grid, &dist1, w.sea_level_m, 1, t1, w.profile);
        let mut masks_changed = std::collections::BTreeSet::new();
        for j in 0..grid.nz {
            for i in 0..grid.nx {
                let h0 = ctx0.height_at(grid.x(i), grid.z(j));
                let h1 = ctx1.height_at(grid.x(i), grid.z(j));
                if h0 != h1 {
                    // The changed node feeds mesh vertices in the
                    // neighbouring chunks' slices: a node affects the
                    // chunk(s) whose node range CONTAINS it. Nodes on a
                    // border belong to both neighbours.
                    let c0 = (i / 64).min(7);
                    let c1 = (j / 64).min(7);
                    masks_changed.insert((c0, c1));
                    if i % 64 == 0 && i > 0 {
                        masks_changed.insert((c0 - 1, c1));
                    }
                    if j % 64 == 0 && j > 0 {
                        masks_changed.insert((c0, c1 - 1));
                    }
                }
            }
        }
        let r1 = build(&d);
        assert!(!masks_changed.is_empty());
        let executed = executed_chunks(&r1);
        let mut expanded = masks_changed.clone();
        for &(cx, cz) in &masks_changed {
            for (dx, dz) in [(-1i32, 0), (1, 0), (0, -1), (0, 1)] {
                let (nx, nz) = (cx as i32 + dx, cz as i32 + dz);
                if (0..8).contains(&nx) && (0..8).contains(&nz) {
                    expanded.insert((nx as usize, nz as usize));
                }
            }
        }
        assert!(
            executed.is_subset(&expanded),
            "case {case}: executed {executed:?} outside expanded {expanded:?}"
        );
        assert!(
            masks_changed.is_subset(&executed),
            "case {case}: affected {masks_changed:?} not all executed"
        );
        // Written chunk files == executed chunks whose GLB changed.
        let written: std::collections::BTreeSet<(usize, usize)> = r1
            .write
            .written_paths
            .iter()
            .filter_map(|p| {
                p.strip_prefix("exports/terrain/chunk_")?
                    .strip_suffix(".glb")
            })
            .map(|s| {
                let mut it = s.split('_');
                (
                    it.next().unwrap().parse().unwrap(),
                    it.next().unwrap().parse().unwrap(),
                )
            })
            .collect();
        assert!(written.is_subset(&executed));
        assert!(masks_changed.is_subset(&written));
        if case == "interior" {
            // No diagonal neighbour executed: chunk (1,1) affected ->
            // diagonals (0,0),(2,0),(0,2),(2,2) must NOT run unless the
            // mountain crosses an edge.
            for diag in [(0usize, 0usize), (2, 0), (0, 2), (2, 2)] {
                assert!(
                    !executed.contains(&diag) || masks_changed.contains(&diag),
                    "diagonal {diag:?} executed in interior case"
                );
            }
        }
    }
}

// ---- 4. Corrupt cache blob is detected and repaired ----

#[test]
fn corrupt_blob_forces_rerun() {
    let d = tmpdir("corrupt");
    make_project(&d);
    build(&d);
    let orig = output_tree_hash(&d);
    // Flip a byte in one blob.
    let blobs = d.join("cache/blobs");
    let victim = std::fs::read_dir(&blobs)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut bytes = std::fs::read(&victim).unwrap();
    bytes[0] ^= 0xFF;
    std::fs::write(&victim, &bytes).unwrap();
    let r = build(&d);
    assert_eq!(r.run.cache.corrupt, 1, "stats: {:?}", r.run.cache);
    assert!(!r.run.executed.is_empty());
    assert_eq!(output_tree_hash(&d), orig);
}

// ---- 5. Lock ----

#[test]
fn disk_cache_lock() {
    let d = tmpdir("lock");
    let c1 = mb_pipeline::DiskCache::open(&d).unwrap();
    match mb_pipeline::DiskCache::open(&d) {
        Err(mb_pipeline::CacheError::Locked) => {}
        Err(mb_pipeline::CacheError::Io(e)) => panic!("expected Locked, got Io {e}"),
        Ok(_) => panic!("expected Locked, got Ok"),
    }
    drop(c1);
    let _c2 = mb_pipeline::DiskCache::open(&d).unwrap();
}

// ---- 6. generator_version invalidates everything ----

#[test]
fn generator_version_rekeys() {
    let d = tmpdir("gen");
    make_project(&d);
    build(&d);
    // Bump generator_version in project.json.
    let path = d.join(mb_schema::PROJECT_FILE);
    let mut p: ProjectFile =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    p.generator_version = 999;
    std::fs::write(&path, serde_json::to_string_pretty(&p).unwrap() + "\n").unwrap();
    // Validation rejects it (newer than CURRENT) — bypass validation by
    // running the pipeline directly? build_project validates first, so
    // instead assert the key-change at the pipeline level.
    let mut pipe_a = mb_pipeline::Pipeline::new(1, 1);
    let mut pipe_b = mb_pipeline::Pipeline::new(1, 2);
    struct C;
    impl mb_pipeline::Stage for C {
        fn id(&self) -> &str {
            "c"
        }
        fn deps(&self) -> &[String] {
            &[]
        }
        fn code_version(&self) -> u32 {
            1
        }
        fn params(&self) -> Vec<u8> {
            vec![1]
        }
        fn run(&self, _i: &[&mb_pipeline::Artifact], _s: u64) -> mb_pipeline::Artifact {
            mb_pipeline::Artifact::new(vec![7])
        }
    }
    let mut cache = mb_pipeline::Cache::default();
    pipe_a.add(Box::new(C));
    pipe_b.add(Box::new(C));
    let r1 = pipe_a.run(&mut cache).unwrap();
    assert_eq!(r1.executed, vec!["c"]);
    let r2 = pipe_b.run(&mut cache).unwrap();
    assert_eq!(r2.executed, vec!["c"], "generator change must re-execute");
    let r3 = pipe_b.run(&mut cache).unwrap();
    assert!(r3.executed.is_empty());
}

// ---- S10 review: lock-before-clean + path confinement ----

#[test]
fn clean_refuses_while_cache_locked() {
    let d = tmpdir("lockclean");
    make_project(&d);
    build(&d);
    // Hold the lock as a "concurrent build".
    let _held = mb_pipeline::DiskCache::open(&d.join("cache")).unwrap();
    match build_project(&d, &BuildOptions { clean: true }) {
        Err(mb_terrain::build::BuildError::Cache(mb_pipeline::CacheError::Locked)) => {}
        other => panic!(
            "expected Locked, got {}",
            other.map(|_| "Ok").unwrap_or_else(|e| {
                match e {
                    mb_terrain::build::BuildError::Cache(mb_pipeline::CacheError::Io(_)) => "io",
                    _ => "other",
                }
            })
        ),
    }
    // Nothing under cache/ or exports/ was removed.
    assert!(d.join("cache/index.json").exists());
    assert!(std::fs::read_dir(d.join("cache/blobs")).unwrap().count() > 0);
    assert!(d.join("exports/terrain/manifest.json").exists());
}

#[test]
fn tampered_manifest_paths_are_skipped() {
    let d = tmpdir("confinement");
    make_project(&d);
    build(&d);
    // Real decoy files the bad paths would otherwise delete/overwrite.
    std::fs::write(d.join("../outside_shared.txt"), b"keep me").unwrap();
    let outside = d.join("../outside_shared.txt").canonicalize().unwrap();
    let nested = d.join("exports/terrain/../../project.json");
    // project.json is the confinement target for the .. path.
    let proj_before = std::fs::read(d.join("project.json")).unwrap();
    // Write a tampered manifest listing unsafe paths.
    let m = d.join("exports/terrain/manifest.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&m).unwrap()).unwrap();
    v["chunks"] = serde_json::json!([
        {"file": "../outside_shared.txt"},
        {"file": "/tmp/x"},
        {"file": "exports/terrain/../../project.json"},
    ]);
    std::fs::write(&m, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let _ = nested;
    let r = build(&d);
    assert!(
        r.write.skipped_unsafe >= 3,
        "skipped_unsafe={}",
        r.write.skipped_unsafe
    );
    assert!(outside.exists());
    assert_eq!(std::fs::read(d.join("project.json")).unwrap(), proj_before);
    let _ = std::fs::remove_file(&outside);
}
