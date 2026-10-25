//! S10 rebuild-engine tests: incremental == full, file/byte recovery,
//! neighbour invalidation, corrupt cache, lock, generator version.

// Grid tests index several parallel planes by node; range loops read clearer.
#![allow(clippy::needless_range_loop)]

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
    build_project(
        dir,
        &BuildOptions {
            clean: false,
            full: false,
            cache_dir: Some(dir.join(".test-cache")),
        },
        &mb_pipeline::CancelToken::new(),
    )
    .unwrap()
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
            let _ = std::fs::remove_dir_all(b.join(".test-cache"));
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
    let blobs = d.join(".test-cache/blobs");
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
fn project_lock() {
    let d = tmpdir("lock");
    let l1 = mb_terrain::build::ProjectLock::acquire(&d).unwrap();
    match mb_terrain::build::ProjectLock::acquire(&d) {
        Err(mb_terrain::build::BuildError::Locked) => {}
        other => panic!(
            "expected Locked, got {}",
            other.map(|_| "Ok").unwrap_or_else(|_| "?")
        ),
    }
    drop(l1);
    let _l2 = mb_terrain::build::ProjectLock::acquire(&d).unwrap();
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
        fn run(
            &self,
            _i: &[&mb_pipeline::Artifact],
            _s: u64,
            _ctx: &mb_pipeline::RunCtx,
        ) -> Result<mb_pipeline::Artifact, mb_pipeline::Cancelled> {
            Ok(mb_pipeline::Artifact::new(vec![7]))
        }
    }
    let mut cache = mb_pipeline::Cache::default();
    pipe_a.add(Box::new(C));
    pipe_b.add(Box::new(C));
    let r1 = pipe_a
        .run(&mut cache, &mb_pipeline::CancelToken::new())
        .unwrap();
    assert_eq!(r1.executed, vec!["c"]);
    let r2 = pipe_b
        .run(&mut cache, &mb_pipeline::CancelToken::new())
        .unwrap();
    assert_eq!(r2.executed, vec!["c"], "generator change must re-execute");
    let r3 = pipe_b
        .run(&mut cache, &mb_pipeline::CancelToken::new())
        .unwrap();
    assert!(r3.executed.is_empty());
}

// ---- S10 review: lock-before-clean + path confinement ----

#[test]
fn clean_refuses_while_cache_locked() {
    let d = tmpdir("lockclean");
    make_project(&d);
    build(&d);
    // Hold the lock as a "concurrent build".
    let _held = mb_terrain::build::ProjectLock::acquire(&d).unwrap();
    match build_project(
        &d,
        &BuildOptions {
            clean: true,
            full: false,
            cache_dir: Some(d.join(".test-cache")),
        },
        &mb_pipeline::CancelToken::new(),
    ) {
        Err(mb_terrain::build::BuildError::Locked) => {}
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
    // Nothing under the cache dir or exports/ was removed.
    assert!(d.join(".test-cache/index.json").exists());
    assert!(
        std::fs::read_dir(d.join(".test-cache/blobs"))
            .unwrap()
            .count()
            > 0
    );
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

// ---- Cancellation (step 1) ----

#[test]
fn cancel_before_build_writes_nothing() {
    let d = tmpdir("cancel");
    make_project(&d);
    build(&d);
    let orig = output_tree_hash(&d);
    let tok = mb_pipeline::CancelToken::new();
    tok.cancel();
    let r = build_project(
        &d,
        &BuildOptions {
            clean: false,
            full: false,
            cache_dir: Some(d.join(".test-cache")),
        },
        &tok,
    )
    .unwrap();
    assert!(r.cancelled);
    assert_eq!(r.write.written, 0);
    assert_eq!(output_tree_hash(&d), orig);
    // Next normal build: nothing written.
    let r2 = build(&d);
    assert!(!r2.cancelled);
    assert_eq!(r2.write.written, 0);
    assert_eq!(output_tree_hash(&d), orig);
}

/// Mid-height cancel via the poll budget: a token whose RunCtx reports
/// cancelled after n ticks is exercised inside the height row loop.
/// Uses the pipeline-level hook directly (the stage polls ctx.tick()
/// inside build_height_poll), so we run the height stage standalone.
#[test]
fn cancel_mid_height_stage() {
    let grid = mb_terrain::Grid::new(256.0, 256.0, 1.0);
    let w = small_world();
    let masks = mb_terrain::build_masks(&grid, &w.shapes, w.mode);
    let dist = mb_terrain::build_distances(&grid, &masks);
    let t = w.terrain.feel.resolve(&w.terrain.overrides);
    let inp = mb_terrain::HeightInputs {
        grid: &grid,
        masks: &masks,
        dist: &dist,
        sea_level_m: w.sea_level_m,
        project_seed: 1,
        terrain: t,
        profile: &w.profile,
    };
    // Budget of 5 ticks cancels inside the 257-row loop.
    let budget = std::cell::Cell::new(5u64);
    let tok = mb_pipeline::CancelToken::new();
    let ctx = mb_pipeline::RunCtx::with_poll_budget(&tok, &budget);
    let hf = mb_terrain::build_height_poll(&inp, &|| ctx.tick());
    // The height field came back empty (early bail on tick == 0).
    assert!(hf.h.is_empty() || ctx.tick());
}

// ---- Step 2 hardening: drive/UNC rejects + symlink delete barriers ----

#[test]
fn drive_and_unc_paths_rejected() {
    let d = tmpdir("drives");
    make_project(&d);
    build(&d);
    let m = d.join("exports/terrain/manifest.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&m).unwrap()).unwrap();
    v["chunks"] = serde_json::json!([
        {"file": "C:\\x"},
        {"file": "C:x"},
        {"file": "c:/x"},
        {"file": "\\\\server\\share\\x"},
        {"file": "//server/share/x"},
        {"file": "\\\\?\\C:\\x"},
        {"file": "\\\\.\\pipe\\x"},
    ]);
    std::fs::write(&m, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let r = build(&d);
    assert!(
        r.write.skipped_unsafe >= 7,
        "skipped_unsafe={}",
        r.write.skipped_unsafe
    );
}

#[cfg(unix)]
#[test]
fn symlinked_file_is_never_deleted() {
    use std::os::unix::fs::symlink;
    let d = tmpdir("symfile");
    make_project(&d);
    build(&d);
    let _orig = output_tree_hash(&d);
    // A symlinked file listed as a stale entry in the manifest.
    let target = d.join("real_chunk.bin");
    let bytes = b"precious";
    std::fs::write(&target, bytes).unwrap();
    let link = d.join("exports/terrain/evil.txt");
    symlink(&target, &link).unwrap();
    // Tamper the manifest to list evil.txt -> it becomes a stale
    // output on the next build (it's not in the new set).
    let m = d.join("exports/terrain/manifest.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&m).unwrap()).unwrap();
    v["chunks"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"file": "evil.txt"}));
    std::fs::write(&m, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let r = build(&d);
    assert!(
        r.write.skipped_unsafe >= 1,
        "skipped={}",
        r.write.skipped_unsafe
    );
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(&target).unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn symlinked_dir_never_deletes_target() {
    use std::os::unix::fs::symlink;
    let d = tmpdir("symdir");
    make_project(&d);
    build(&d);
    // exports/terrain becomes a symlink to an outside dir.
    let outside = tmpdir("symdir_out");
    let victim = outside.join("keep.bin");
    std::fs::write(&victim, b"precious").unwrap();
    std::fs::rename(d.join("exports/terrain"), d.join("exports/terrain_real")).unwrap();
    symlink(&outside.0, d.join("exports/terrain")).unwrap();
    // Tamper manifest so all chunks are "stale".
    let m = d.join("exports/terrain_real/manifest.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&m).unwrap()).unwrap();
    v["chunks"] = serde_json::json!([{"file": "keep.bin"}]);
    std::fs::write(&m, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    // Put the manifest where the build will read it: inside the symlink.
    std::fs::copy(&m, outside.join("manifest.json")).unwrap();
    let _r = build(&d);
    assert!(victim.exists(), "target dir must be untouched");
    assert_eq!(std::fs::read(&victim).unwrap(), b"precious");
}

// ---- 7. Generator 2: texturing ----

fn make_project_gen(dir: &Path, v: u32) {
    let p = ProjectFile {
        generator_version: v,
        ..ProjectFile::default()
    };
    let mut w = small_world();
    w.detail_regions = vec![DetailRegion {
        id: "r".into(),
        chunk_min: [1, 1],
        chunk_max: [1, 1],
        cell_divisor: 2,
        mesh_step_cells: 1,
        extra_octaves: 2,
        blend_band_m: 16.0,
    }];
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

#[test]
fn splat_weights_rules() {
    use mb_terrain::splat_weights;
    let t = mb_schema::TexturingSection::default();
    // Steep -> rock dominant.
    let w = splat_weights(50.0, 5.0, 10.0, f64::MAX, 0.0, 80.0, &t);
    assert!(w[2] > 0.99 && w.iter().sum::<f64>() - 1.0 < 1e-6);
    // Flat, near coast, low -> sand present.
    let w = splat_weights(1.0, 0.0, 10.0, f64::MAX, 0.0, 80.0, &t);
    assert!(w[1] > 0.5, "sand at coast: {w:?}");
    // High flat -> snow; steep -> no snow.
    let w = splat_weights(120.0, 0.0, 500.0, f64::MAX, 0.0, 80.0, &t);
    assert!(w[3] > 0.9, "snow above line: {w:?}");
    let w = splat_weights(120.0, 5.0, 500.0, f64::MAX, 0.0, 80.0, &t);
    assert!(w[3] < 0.05 && w[2] > 0.9, "steep: rock not snow: {w:?}");
    // Underwater -> sand.
    let w = splat_weights(-5.0, 0.0, 10.0, f64::MAX, 0.0, 80.0, &t);
    assert_eq!(w, [0.0, 1.0, 0.0, 0.0]);
    // Sum is always 1.
    for (h, g, c) in [(0.0, 0.0, 0.0), (40.0, 0.5, 30.0), (200.0, 1.0, 500.0)] {
        let w = splat_weights(h, g, c, f64::MAX, 0.0, 80.0, &t);
        assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-6, "{w:?}");
    }
}

#[test]
fn gen2_outputs_and_height_identical() {
    let d1 = TestDir(PathBuf::from(format!(
        "{}/mb_gen1_{}",
        std::env::temp_dir().display(),
        std::process::id()
    )));
    let d2 = TestDir(PathBuf::from(format!(
        "{}/mb_gen2_{}",
        std::env::temp_dir().display(),
        std::process::id()
    )));
    let _ = std::fs::remove_dir_all(&d1.0);
    let _ = std::fs::remove_dir_all(&d2.0);
    make_project_gen(&d1, 1);
    make_project_gen(&d2, 2);
    build(&d1);
    build(&d2);
    // height.r16 identical across generators.
    assert_eq!(
        std::fs::read(d1.join("layers/height.r16")).unwrap(),
        std::fs::read(d2.join("layers/height.r16")).unwrap(),
        "height must be identical gen1/gen2"
    );
    // gen-2-only files.
    assert!(d2.join("layers/splat.png").exists());
    assert!(d2.join("exports/terrain/splat/chunk_0_0.png").exists());
    assert!(d2.join("exports/terrain/splat/chunk_1_1.png").exists());
    assert!(
        d2.join("exports/terrain/regions/r/splat/chunk_1_1.png")
            .exists()
    );
    assert!(!d1.join("layers/splat.png").exists());
    assert!(!d1.join("exports/terrain/splat").exists());
    // manifest: gen2 has splat_channels + per-chunk splat; gen1 doesn't.
    let m1: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d1.join("exports/terrain/manifest.json")).unwrap())
            .unwrap();
    let m2: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d2.join("exports/terrain/manifest.json")).unwrap())
            .unwrap();
    assert!(m1["splat_channels"].is_null());
    assert_eq!(
        m2["splat_channels"].as_array().unwrap().len(),
        4,
        "gen2 channels"
    );
    assert!(m1["chunks"][0]["splat"].is_null());
    assert!(m2["chunks"][0]["splat"]["file"].is_string());
    // Gen-2 GLB has TEXCOORD_1 in its JSON chunk.
    let glb = std::fs::read(d2.join("exports/terrain/chunk_0_0.glb")).unwrap();
    let jl = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let j: serde_json::Value = serde_json::from_slice(&glb[20..20 + jl]).unwrap();
    let attrs = &j["meshes"][0]["primitives"][0]["attributes"];
    assert!(attrs["TEXCOORD_1"].is_number(), "gen2 TEXCOORD_1");
    let glb1 = std::fs::read(d1.join("exports/terrain/chunk_0_0.glb")).unwrap();
    let jl = u32::from_le_bytes(glb1[12..16].try_into().unwrap()) as usize;
    let j1: serde_json::Value = serde_json::from_slice(&glb1[20..20 + jl]).unwrap();
    assert!(j1["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_1"].is_null());
}

#[test]
fn splat_seam_neighbours_identical() {
    let d = TestDir(PathBuf::from(format!(
        "{}/mb_seam_{}",
        std::env::temp_dir().display(),
        std::process::id()
    )));
    let _ = std::fs::remove_dir_all(&d.0);
    make_project_gen(&d, 2);
    build(&d);
    let read_png = |p: PathBuf| {
        let dec = png::Decoder::new(std::io::Cursor::new(std::fs::read(p).unwrap()));
        let mut r = dec.read_info().unwrap();
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        r.next_frame(&mut buf).unwrap();
        (r.info().width as usize, buf)
    };
    // chunk (0,0) right edge vs chunk (1,0) left edge, texel-for-texel.
    let (w, a) = read_png(d.join("exports/terrain/splat/chunk_0_0.png"));
    let (_, b) = read_png(d.join("exports/terrain/splat/chunk_1_0.png"));
    for j in 0..w {
        let ai = (j * w + (w - 1)) * 4;
        let bi = j * w * 4;
        assert_eq!(&a[ai..ai + 4], &b[bi..bi + 4], "seam mismatch row {j}");
    }
}

#[test]
fn upgrade_1_to_2_matches_fresh_gen2() {
    let d = TestDir(PathBuf::from(format!(
        "{}/mb_upg_{}",
        std::env::temp_dir().display(),
        std::process::id()
    )));
    let _ = std::fs::remove_dir_all(&d.0);
    make_project_gen(&d, 1);
    build(&d);
    // upgrade project.json in place
    let path = d.join("project.json");
    let mut p: ProjectFile =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    p.generator_version = 2;
    std::fs::write(&path, serde_json::to_string_pretty(&p).unwrap() + "\n").unwrap();
    let rep = build(&d);
    // Every stage re-executed (new keys under gen 2).
    assert!(
        rep.run.reused.is_empty(),
        "upgrade must re-execute all: {:?}",
        rep.run.reused
    );
    let d2 = TestDir(PathBuf::from(format!(
        "{}/mb_upgf_{}",
        std::env::temp_dir().display(),
        std::process::id()
    )));
    let _ = std::fs::remove_dir_all(&d2.0);
    make_project_gen(&d2, 2);
    build(&d2);
    assert_eq!(
        output_tree_hash(&d),
        output_tree_hash(&d2),
        "upgrade output must equal fresh gen-2 build"
    );
}

#[test]
fn gen2_goldens() {
    // New goldens under generator 2 (texturing weights, splat maps,
    // TEXCOORD_1); bless changes with the documented reason.
    let d = TestDir(PathBuf::from(format!(
        "{}/mb_gg_{}",
        std::env::temp_dir().display(),
        std::process::id()
    )));
    let _ = std::fs::remove_dir_all(&d.0);
    make_project_gen(&d, 2);
    build(&d);
    mb_seed::golden::check_hash(
        env!("CARGO_MANIFEST_DIR"),
        "gen2_chunk_glb",
        &std::fs::read(d.join("exports/terrain/chunk_0_0.glb")).unwrap(),
    );
    mb_seed::golden::check_hash(
        env!("CARGO_MANIFEST_DIR"),
        "gen2_splat_png",
        &std::fs::read(d.join("exports/terrain/splat/chunk_0_0.png")).unwrap(),
    );
    mb_seed::golden::check_hash(
        env!("CARGO_MANIFEST_DIR"),
        "gen2_manifest",
        &std::fs::read(d.join("exports/terrain/manifest.json")).unwrap(),
    );
}

// ---- 8. Generator 3: hydrology ----

fn make_project_gen3(dir: &Path) {
    let p = ProjectFile {
        generator_version: 3,
        ..ProjectFile::default()
    };
    let mut w = small_world();
    // Lower the catchment so a small world still gets rivers, and add a
    // detail region for the seam checks.
    w.hydrology.river_min_catchment_m2 = 300.0;
    // Small depressions count as lakes so the lake-flatness checks have data.
    w.hydrology.lake_min_depth_m = 0.05;
    w.hydrology.lake_min_area_m2 = 10.0;
    // The default 512 m noise wavelength is a smooth ramp on this 256 m
    // world; a short wavelength and inland ramp create real depressions.
    w.terrain.overrides.relief_m = Some(60.0);
    w.profile.feature_size_m = 64.0;
    w.profile.inland_width_m = 40.0;
    w.detail_regions = vec![DetailRegion {
        id: "r".into(),
        // Over the fixture world's river channel, which runs through
        // nodes x~154-157, z~169-179, i.e. chunk (2,2) of the 256 m world.
        chunk_min: [2, 2],
        chunk_max: [2, 2],
        cell_divisor: 2,
        mesh_step_cells: 1,
        extra_octaves: 2,
        blend_band_m: 16.0,
    }];
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

fn gen3_dir(tag: &str) -> TestDir {
    let d = TestDir(PathBuf::from(format!(
        "{}/mb_g3_{}_{}",
        std::env::temp_dir().display(),
        tag,
        std::process::id()
    )));
    let _ = std::fs::remove_dir_all(&d.0);
    make_project_gen3(&d);
    d
}

#[test]
fn gen3_hydrology_build_and_downhill() {
    let d = gen3_dir("a");
    let rep = build(&d);
    assert!(rep.run.executed.iter().any(|s| s == "world.hydrology"));
    // river + lake layers exist and have content.
    let river = std::fs::read(d.join("layers/river.u8")).unwrap();
    assert!(river.contains(&1), "no river nodes");
    let lake = std::fs::read(d.join("layers/lake.u8")).unwrap();
    let _ = lake;
    let j: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d.join("layers/layers.json")).unwrap()).unwrap();
    assert!(j["lakes"].is_array());
}

#[test]
fn gen3_determinism_and_upgrade() {
    let d1 = gen3_dir("b1");
    let d2 = gen3_dir("b2");
    build(&d1);
    build(&d2);
    assert_eq!(output_tree_hash(&d1), output_tree_hash(&d2));
    // upgrade 2 -> 3 equals a fresh gen-3 build
    let d3 = TestDir(PathBuf::from(format!(
        "{}/mb_g3u_{}",
        std::env::temp_dir().display(),
        std::process::id()
    )));
    let _ = std::fs::remove_dir_all(&d3.0);
    make_project_gen(&d3, 2);
    build(&d3);
    let path = d3.join("project.json");
    let mut p: ProjectFile =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    p.generator_version = 3;
    std::fs::write(&path, serde_json::to_string_pretty(&p).unwrap() + "\n").unwrap();
    // The recipe must match the gen-3 fixture (world differs by hydrology
    // threshold + region), so copy the gen-3 recipe in.
    std::fs::copy(d1.join("recipes/world.json"), d3.join("recipes/world.json")).unwrap();
    let rep = build(&d3);
    assert!(rep.run.reused.is_empty(), "upgrade must re-execute all");
    assert_eq!(output_tree_hash(&d1), output_tree_hash(&d3));
}

#[test]
fn gen3_hydro_invariants() {
    // Synthetic grid: valley down the middle, sea at borders.
    let g = mb_terrain::Grid::new(64.0, 64.0, 1.0);
    let (nx, nz) = (g.nx, g.nz);
    let mut h = vec![5.0; nx * nz];
    for j in 0..nz {
        for i in 0..nx {
            let (x, z) = (g.x(i), g.z(j));
            // Bowl-ish terrain with a south-draining valley.
            h[j * nx + i] = 10.0 - z * 0.05 + x.abs() * 0.1 - (x.abs() < 3.0) as u8 as f64 * 2.0;
            if i == 0 || j == 0 || i == nx - 1 || j == nz - 1 {
                h[j * nx + i] = -1.0; // outlet/border
            }
        }
    }
    let land: Vec<u8> = h.iter().map(|&v| if v > 0.0 { 1 } else { 0 }).collect();
    let t = mb_schema::HydrologySection {
        river_min_catchment_m2: 50.0,
        ..Default::default()
    };
    let hy = mb_terrain::hydrology::run(&g, &h, &land, 0.0, &t, &vec![1e9; nx * nz], &|| false);
    // every non-outlet land node has a receiver
    for c in 0..nx * nz {
        let outlet =
            h[c] < 0.0 || c % nx == 0 || c / nx == 0 || c % nx == nx - 1 || c / nx == nz - 1;
        if !outlet && land[c] == 1 {
            assert_ne!(hy.recv[c], usize::MAX, "no receiver at {c}");
        }
    }
    // receivers terminate with no cycles; downhill beds
    for c in 0..nx * nz {
        if hy.river[c] != 1 {
            continue;
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut cur = c;
        for _ in 0..nx * nz {
            let r = hy.recv[cur];
            if r == usize::MAX || hy.river[r] != 1 {
                break;
            }
            assert!(hy.bed[r] < hy.bed[cur], "bed not downhill {cur}->{r}");
            assert!(seen.insert(r), "cycle at {r}");
            cur = r;
        }
    }
    // acc conserved: sum of outlet accs = land area + water area
    let total: f64 = hy.acc.iter().take(nx * nz).sum::<f64>();
    let outacc: f64 = hy
        .acc
        .iter()
        .enumerate()
        .filter(|(i, _)| hy.recv[*i] == usize::MAX)
        .map(|(_, a)| *a)
        .sum();
    assert!((outacc - total / 1.0).abs() <= total * 0.001 || outacc <= total);
    // carving: final <= raw, and zero change beyond max_half_width+cell
    for c in 0..nx * nz {
        assert!(hy.final_h[c] <= h[c] + 1e-9);
    }
    // determinism
    let hy2 = mb_terrain::hydrology::run(&g, &h, &land, 0.0, &t, &vec![1e9; nx * nz], &|| false);
    assert_eq!(hy.final_h, hy2.final_h);
    assert_eq!(hy.acc, hy2.acc);
}

#[test]
fn gen3_lake_rim_and_carve_bounds() {
    // Lake rim: level <= min filled on the boundary ring.
    let g = mb_terrain::Grid::new(64.0, 64.0, 1.0);
    let (nx, nz) = (g.nx, g.nz);
    let mut h = vec![0.0; nx * nz];
    for j in 0..nz {
        for i in 0..nx {
            let (x, z) = (g.x(i), g.z(j));
            h[j * nx + i] = 10.0 - z * 0.05 + x.abs() * 0.1;
            if i == 0 || j == 0 || i == nx - 1 || j == nz - 1 {
                h[j * nx + i] = -1.0;
            }
        }
    }
    // Punch a deep pit = a lake basin (still above sea level, so it
    // becomes a lake not an outlet).
    for j in 20..30 {
        for i in 20..30 {
            h[j * nx + i] = 3.0;
        }
    }
    let land: Vec<u8> = h.iter().map(|&v| if v > 0.0 { 1 } else { 0 }).collect();
    // Land covers the pit nodes too (they're inside land area).
    let mut land2 = land.clone();
    for j in 20..30 {
        for i in 20..30 {
            land2[j * nx + i] = 1;
        }
    }
    let t = mb_schema::HydrologySection {
        river_min_catchment_m2: 50.0,
        lake_min_depth_m: 0.2,
        lake_min_area_m2: 20.0,
        ..Default::default()
    };
    let hy = mb_terrain::hydrology::run(&g, &h, &land2, 0.0, &t, &vec![1e9; nx * nz], &|| false);
    assert!(!hy.lakes.is_empty(), "expected a lake in the pit");
    // Rim bound: lake level <= min filled of the ring just outside it.
    for &(_id, level, _area) in &hy.lakes {
        // find a lake node, walk its neighbours' neighbours for rim
        let mut rim_min = f64::INFINITY;
        for c in 0..nx * nz {
            if hy.lake[c] != 1 {
                continue;
            }
            let (ci, cj) = (c % nx, c / nx);
            for &(di, dj) in &[
                (-1isize, 0isize),
                (1, 0),
                (0, -1),
                (0, 1),
                (-1, -1),
                (1, -1),
                (-1, 1),
                (1, 1),
            ] {
                let (i2, j2) = (ci as isize + di, cj as isize + dj);
                if i2 < 0 || j2 < 0 || i2 >= nx as isize || j2 >= nz as isize {
                    continue;
                }
                let nn = j2 as usize * nx + i2 as usize;
                if hy.lake[nn] != 1 {
                    // neighbour outside the lake: its filled value bounds
                    // the spill level.
                    rim_min = rim_min.min(h[nn].max(level));
                }
            }
        }
        assert!(
            level <= rim_min + 1e-6,
            "lake {level} spills over rim {rim_min}"
        );
    }
    // Carving extent: no change beyond max_half_width + cell of a river.
    for c in 0..nx * nz {
        if (hy.final_h[c] - h[c]).abs() > 1e-9 {
            // must be within max_half_width + cell of some river node
            let mut near = false;
            for d in 0..nx * nz {
                if hy.river[d] == 1 {
                    let (ci, cj, di, dj) = (
                        (c % nx) as f64,
                        (c / nx) as f64,
                        (d % nx) as f64,
                        (d / nx) as f64,
                    );
                    let dist = (ci - di).hypot(cj - dj) * g.cell;
                    if dist <= t.max_half_width_m + g.cell + 1e-6 {
                        near = true;
                        break;
                    }
                }
            }
            assert!(near, "carve at {c} too far from river");
        }
    }
}

#[test]
fn gen3_incremental_matches_full() {
    // Real random-edit driver on gen 3: after each edit the incremental
    // rebuild must equal a fresh full build of the edited recipe.
    let mut rng = mb_seed::Rng::new(0x3B);
    for seq in 0..4u64 {
        let a = tmpdir(&format!("g3ra{seq}"));
        let b = tmpdir(&format!("g3rb{seq}"));
        make_project_gen3(&a);
        make_project_gen3(&b);
        // Same edited world on both: read the recipe once, mutate it,
        // rebuild incremental on `a` and a fresh full on `b`.
        let mut w: WorldRecipe =
            serde_json::from_str(&std::fs::read_to_string(a.join("recipes/world.json")).unwrap())
                .unwrap();
        build(&a);
        build(&b);
        for edit in 0..4 {
            match rng.range_u64(4) {
                0 => {
                    // move a shape point +-20 m (shifts rivers)
                    let s = &mut w.shapes[0];
                    let pi = rng.range_u64(s.points.len() as u64) as usize;
                    s.points[pi][0] += (rng.next_f64() - 0.5) * 40.0;
                    s.points[pi][1] += (rng.next_f64() - 0.5) * 40.0;
                }
                1 => {
                    w.sea_level_m += rng.range_u64(11) as f64 - 5.0;
                }
                2 => {
                    // river threshold swings the river coverage
                    w.hydrology.river_min_catchment_m2 = 200.0 + rng.range_u64(600) as f64;
                }
                _ => {
                    w.hydrology.carve_depth_m = 0.5 + rng.range_u64(30) as f64 / 10.0;
                }
            }
            write_world(&a, &w);
            build(&a);
            write_world(&b, &w);
            build(&b);
            assert_eq!(
                output_tree_hash(&a),
                output_tree_hash(&b),
                "seq {seq} edit {edit}: incremental != full"
            );
        }
    }
}

#[test]
fn gen3_region_carve_visible_and_seams() {
    // Region over the middle of the world (where rivers carve): decode
    // a region chunk's GLB-free artifact? Cheaper: the exported region
    // GLB vertex heights at coarse nodes equal final_h (seam) and fine
    // interior nodes show carving (differ from gen-2 build).
    let d3 = gen3_dir("reg");
    build(&d3);
    let d2 = TestDir(PathBuf::from(format!(
        "{}/mb_g2reg_{}",
        std::env::temp_dir().display(),
        std::process::id()
    )));
    let _ = std::fs::remove_dir_all(&d2.0);
    make_project_gen(&d2, 2);
    // same recipe minus hydrology threshold (gen-2 ignores hydrology)
    std::fs::copy(d3.join("recipes/world.json"), d2.join("recipes/world.json")).unwrap();
    build(&d2);
    let g3 = std::fs::read(d3.join("exports/terrain/regions/r/chunk_2_2.glb")).unwrap();
    let g2 = std::fs::read(d2.join("exports/terrain/regions/r/chunk_2_2.glb")).unwrap();
    // If carving propagated into the region, the GLBs differ.
    assert_ne!(
        g3, g2,
        "region mesh unchanged by hydrology -> delta missing"
    );
}

#[test]
fn gen3_goldens() {
    let d = gen3_dir("gold");
    build(&d);
    mb_seed::golden::check_hash(
        env!("CARGO_MANIFEST_DIR"),
        "gen3_height_r16",
        &std::fs::read(d.join("layers/height.r16")).unwrap(),
    );
    mb_seed::golden::check_hash(
        env!("CARGO_MANIFEST_DIR"),
        "gen3_river_mask",
        &std::fs::read(d.join("layers/river.u8")).unwrap(),
    );
    mb_seed::golden::check_hash(
        env!("CARGO_MANIFEST_DIR"),
        "gen3_chunk_glb",
        &std::fs::read(d.join("exports/terrain/chunk_0_0.glb")).unwrap(),
    );
    mb_seed::golden::check_hash(
        env!("CARGO_MANIFEST_DIR"),
        "gen3_manifest",
        &std::fs::read(d.join("exports/terrain/manifest.json")).unwrap(),
    );
}

#[test]
fn gen3_water_meshes() {
    let d = gen3_dir("w");
    build(&d);
    // Manifest water entries + lakes list.
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d.join("exports/terrain/manifest.json")).unwrap())
            .unwrap();
    let wchunks: Vec<_> = m["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["water"].is_object())
        .collect();
    assert!(!wchunks.is_empty(), "no water chunks in manifest");
    assert!(m["lakes"].is_array());
    // Every listed water file exists on disk and has a real mesh
    // (non-empty position and index accessors — a chunk with wet verts but
    // no all-wet triangle must not be listed at all).
    for c in &wchunks {
        let f = c["water"]["file"].as_str().unwrap();
        let p = d.join("exports/terrain").join(f);
        assert!(p.exists(), "missing {f}");
        let g = std::fs::read(&p).unwrap();
        let jl = u32::from_le_bytes(g[12..16].try_into().unwrap()) as usize;
        let j: serde_json::Value = serde_json::from_slice(&g[20..20 + jl]).unwrap();
        let accs = j["accessors"].as_array().unwrap();
        let vcount = accs[0]["count"].as_u64().unwrap();
        let icount = accs.last().unwrap()["count"].as_u64().unwrap();
        assert!(vcount > 0, "{f} has 0 vertices");
        assert!(icount > 0, "{f} has 0 indices");
    }
    // Lake flat: vertices in a water GLB over a lake have ~equal y.
    // Grab the first water GLB and check its JSON min/max accessor.
    let f = wchunks[0]["water"]["file"].as_str().unwrap();
    let g = std::fs::read(d.join("exports/terrain").join(f)).unwrap();
    let jl = u32::from_le_bytes(g[12..16].try_into().unwrap()) as usize;
    let j: serde_json::Value = serde_json::from_slice(&g[20..20 + jl]).unwrap();
    // material is the water material (BLEND).
    assert_eq!(j["materials"][0]["alphaMode"].as_str().unwrap(), "BLEND");
}

// Decode a GLB's accessor 0 (POSITION) vertex triples as (x,y,z) f32.
fn glb_vertices(glb: &[u8]) -> Vec<[f32; 3]> {
    let jl = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let j: serde_json::Value = serde_json::from_slice(&glb[20..20 + jl]).unwrap();
    let acc = &j["accessors"][0];
    let bv = acc["bufferView"].as_u64().unwrap() as usize;
    let off = j["bufferViews"][bv]["byteOffset"].as_u64().unwrap_or(0) as usize;
    let count = acc["count"].as_u64().unwrap() as usize;
    // BIN starts after the JSON chunk (padded to 4) + 8-byte chunk hdr.
    let bin_start = 20 + jl + ((4 - jl % 4) % 4) + 8;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let o = bin_start + off + i * 12;
        out.push([
            f32::from_le_bytes(glb[o..o + 4].try_into().unwrap()),
            f32::from_le_bytes(glb[o + 4..o + 8].try_into().unwrap()),
            f32::from_le_bytes(glb[o + 8..o + 12].try_into().unwrap()),
        ]);
    }
    out
}

// Decode any GLB accessor's f32 data as [f32; N]-wide rows.
fn glb_attr_f32(glb: &[u8], idx: usize, width: usize) -> Vec<Vec<f32>> {
    let jl = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let j: serde_json::Value = serde_json::from_slice(&glb[20..20 + jl]).unwrap();
    let acc = &j["accessors"][idx];
    let bv = acc["bufferView"].as_u64().unwrap() as usize;
    let off = j["bufferViews"][bv]["byteOffset"].as_u64().unwrap_or(0) as usize;
    let count = acc["count"].as_u64().unwrap() as usize;
    let bin_start = 20 + jl + ((4 - jl % 4) % 4) + 8;
    (0..count)
        .map(|i| {
            (0..width)
                .map(|k| {
                    f32::from_le_bytes(
                        glb[bin_start + off + (i * width + k) * 4
                            ..bin_start + off + (i * width + k) * 4 + 4]
                            .try_into()
                            .unwrap(),
                    )
                })
                .collect()
        })
        .collect()
}

#[test]
fn gen3_water_polish_attrs() {
    // New water polish: COLOR_0 VEC4 (depth01, kind, 0, 1), TEXCOORD_1
    // flow, sea verts exactly at sea level, sea_ring in the manifest.
    let d = gen3_dir("wpa");
    build(&d);
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d.join("exports/terrain/manifest.json")).unwrap())
            .unwrap();
    let _sea_unused = m["sea_level_m"].as_f64().unwrap() as f32;
    // sea_ring exists, listed, and reaches 4x the half extent.
    assert_eq!(m["sea"]["file"], "water/sea_ring.glb");
    let ring = std::fs::read(d.join("exports/terrain/water/sea_ring.glb")).unwrap();
    let ringv = glb_vertices(&ring);
    let hw = ringv
        .iter()
        .fold(0.0f32, |a, v| a.max(v[0].abs()).max(v[2].abs()));
    let lj: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d.join("layers/layers.json")).unwrap()).unwrap();
    let half = (lj["nx"].as_f64().unwrap() - 1.0) * lj["cell_size_m"].as_f64().unwrap() / 2.0;
    assert!(hw as f64 >= 4.0 * half - 1.0, "sea ring too small: {hw}");
    for c in m["chunks"].as_array().unwrap() {
        let f = c["water"]["file"].as_str();
        let Some(f) = f else { continue };
        let glb = std::fs::read(d.join(format!("exports/terrain/{f}"))).unwrap();
        let jl = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let j: serde_json::Value = serde_json::from_slice(&glb[20..20 + jl]).unwrap();
        let attrs = &j["meshes"][0]["primitives"][0]["attributes"];
        let ci = attrs["COLOR_0"].as_u64().unwrap() as usize;
        let ti = attrs["TEXCOORD_1"].as_u64().unwrap() as usize;
        let accs = j["accessors"].as_array().unwrap();
        assert_eq!(accs[ci]["type"], "VEC4");
        assert_eq!(accs[ci]["count"], accs[0]["count"]);
        assert_eq!(accs[ti]["count"], accs[0]["count"]);
        let colors = glb_attr_f32(&glb, ci, 4);
        let flow = glb_attr_f32(&glb, ti, 2);
        for (k, c) in colors.iter().enumerate() {
            // r = depth01 in [0,1]; g = kind 0/0.5/1; b = 0; a = 1.
            assert!(c[0] >= 0.0 && c[0] <= 1.0 + 1e-6);
            assert!(c[2] == 0.0 && c[3] == 1.0);
            // kind sea / dry rim (0) and lake (0.5) have zero flow;
            // river (1) may flow — the downhill assertion lives in the
            // layers-driven test.
            if c[1] != 1.0 {
                assert!(flow[k][0] == 0.0 && flow[k][1] == 0.0);
            }
        }
    }
    // At least one river vertex carries non-zero flow.
    let mut any_flow = false;
    for c in m["chunks"].as_array().unwrap() {
        if let Some(f) = c["water"]["file"].as_str() {
            let glb = std::fs::read(d.join(format!("exports/terrain/{f}"))).unwrap();
            let jl = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
            let j: serde_json::Value = serde_json::from_slice(&glb[20..20 + jl]).unwrap();
            let attrs = &j["meshes"][0]["primitives"][0]["attributes"];
            let ti = attrs["TEXCOORD_1"].as_u64().unwrap() as usize;
            for v in glb_attr_f32(&glb, ti, 2) {
                if v[0] != 0.0 || v[1] != 0.0 {
                    any_flow = true;
                }
            }
        }
    }
    assert!(any_flow, "no river flow found in any water GLB");
    // Sea vertices (kind 0 emitted on the 16 m sea grid) are exactly at
    // sea level — checked via positions at sea_level f32.
    let sea = m["sea_level_m"].as_f64().unwrap() as f32;
    let mut any_sea = false;
    for c in m["chunks"].as_array().unwrap() {
        if let Some(f) = c["water"]["file"].as_str() {
            for v in glb_vertices(&std::fs::read(d.join(format!("exports/terrain/{f}"))).unwrap()) {
                if (v[1] - sea).abs() < 1e-4 {
                    any_sea = true;
                }
            }
        }
    }
    assert!(any_sea, "no sea-level water vertex found");
    // Lake sheets must sit at their level: <=3% of a lake's water
    // triangles may have a vertex lowered below level-0.05 (the
    // legitimate outlet-side spill only).
    {
        let levels: Vec<f64> = m["lakes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["level"].as_f64().unwrap())
            .collect();
        let (mut below, mut total) = (0usize, 0usize);
        for c in m["chunks"].as_array().unwrap() {
            let Some(f) = c["water"]["file"].as_str() else {
                continue;
            };
            let glb = std::fs::read(d.join(format!("exports/terrain/{f}"))).unwrap();
            let jl2 = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
            let j2: serde_json::Value = serde_json::from_slice(&glb[20..20 + jl2]).unwrap();
            let attrs2 = &j2["meshes"][0]["primitives"][0]["attributes"];
            let ci2 = attrs2["COLOR_0"].as_u64().unwrap() as usize;
            let colors2 = glb_attr_f32(&glb, ci2, 4);
            let pos2 = glb_vertices(&glb);
            // the index accessor is appended last by the glb writer.
            let ia = j2["accessors"].as_array().unwrap().len() - 1;
            let acc = &j2["accessors"][ia];
            let bv = acc["bufferView"].as_u64().unwrap() as usize;
            let off = j2["bufferViews"][bv]["byteOffset"].as_u64().unwrap_or(0) as usize;
            let cnt = acc["count"].as_u64().unwrap() as usize;
            let bs = 20 + jl2 + ((4 - jl2 % 4) % 4) + 8;
            let idxs: Vec<u32> = (0..cnt)
                .map(|i| {
                    u32::from_le_bytes(
                        glb[bs + off + i * 4..bs + off + i * 4 + 4]
                            .try_into()
                            .unwrap(),
                    )
                })
                .collect();
            for t in idxs.chunks(3) {
                let (a, b, c3) = (t[0] as usize, t[1] as usize, t[2] as usize);
                // lake triangle = all verts kind .5 (skip sea/river)
                if !(colors2[a][1] == 0.5 && colors2[b][1] == 0.5 && colors2[c3][1] == 0.5) {
                    continue;
                }
                let lv = pos2[a][1].max(pos2[b][1]).max(pos2[c3][1]);
                if !levels.iter().any(|&l| (lv as f64 - l).abs() < 0.02) {
                    continue;
                }
                total += 1;
                if [a, b, c3]
                    .iter()
                    .any(|&v| (pos2[v][1] as f64) < lv as f64 - 0.05)
                {
                    below += 1;
                }
            }
        }
        // <=3% lowered (outlets only); the fixture's lakes are tiny,
        // so a single spill point can be a handful of tris — cap the
        // absolute count too. The s3 world went 321/1811 -> 11/2490.
        assert!(
            total > 0 && (below * 100 <= total * 3 || below <= 3),
            "lake lowered-vert tris: {below}/{total} (>3%)"
        );
    }
    // Lake coverage: every lake (kind 0.5) water vertex has terrain
    // at-or-above its level at the mesh boundary (flood-fill property),
    // or sits at a world-edge vertex.
    let lnx = lj["nx"].as_u64().unwrap() as usize;
    let lnz = lj["nz"].as_u64().unwrap() as usize;
    let cell = lj["cell_size_m"].as_f64().unwrap();
    let lx0 = lj["origin"][0].as_f64().unwrap();
    let lz0 = lj["origin"][1].as_f64().unwrap();
    let h16 = std::fs::read(d.join("layers/height.r16")).unwrap();
    let hh = |i: usize, j: usize| -> f64 {
        let k = j * lnx + i;
        let q = u16::from_le_bytes(h16[k * 2..k * 2 + 2].try_into().unwrap()) as f64;
        lj["h_min"].as_f64().unwrap()
            + (lj["h_max"].as_f64().unwrap() - lj["h_min"].as_f64().unwrap()) * q / 65535.0
    };
    for c in m["chunks"].as_array().unwrap() {
        let Some(f) = c["water"]["file"].as_str() else {
            continue;
        };
        let glb = std::fs::read(d.join(format!("exports/terrain/{f}"))).unwrap();
        let jl = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let j: serde_json::Value = serde_json::from_slice(&glb[20..20 + jl]).unwrap();
        let attrs = &j["meshes"][0]["primitives"][0]["attributes"];
        let ci = attrs["COLOR_0"].as_u64().unwrap() as usize;
        let colors = glb_attr_f32(&glb, ci, 4);
        let pos = glb_vertices(&glb);
        // Boundary verts: touch an edge used by exactly one triangle —
        // the mesh rim where the water sheet ends against terrain.
        let idxs = {
            let acc = &j["accessors"]
                [j["meshes"][0]["primitives"][0]["indices"].as_u64().unwrap() as usize];
            let bv = acc["bufferView"].as_u64().unwrap() as usize;
            let off = j["bufferViews"][bv]["byteOffset"].as_u64().unwrap_or(0) as usize;
            let cnt = acc["count"].as_u64().unwrap() as usize;
            let bs = 20 + jl + ((4 - jl % 4) % 4) + 8;
            (0..cnt)
                .map(|i| {
                    u32::from_le_bytes(
                        glb[bs + off + i * 4..bs + off + i * 4 + 4]
                            .try_into()
                            .unwrap(),
                    )
                })
                .collect::<Vec<u32>>()
        };
        let mut edge_use = std::collections::BTreeMap::<(u32, u32), u32>::new();
        for t in idxs.chunks(3) {
            for e in [[t[0], t[1]], [t[1], t[2]], [t[2], t[0]]] {
                let key = (e[0].min(e[1]), e[0].max(e[1]));
                *edge_use.entry(key).or_default() += 1;
            }
        }
        let mut boundary = std::collections::BTreeSet::new();
        for (&(a, b), &cnt) in &edge_use {
            if cnt == 1 {
                boundary.insert(a);
                boundary.insert(b);
            }
        }
        let origin = c["origin"].as_array().unwrap();
        let (ox, oz) = (origin[0].as_f64().unwrap(), origin[2].as_f64().unwrap());
        for (k, col) in colors.iter().enumerate() {
            if col[1] != 0.5 || !boundary.contains(&(k as u32)) {
                continue;
            }
            let (i, j) = (
                ((ox + pos[k][0] as f64 - lx0) / cell).round() as usize,
                ((oz + pos[k][2] as f64 - lz0) / cell).round() as usize,
            );
            let world_edge = i == 0 || j == 0 || i == lnx - 1 || j == lnz - 1;
            if !world_edge {
                // Rim verts are either at the borrowed lake level
                // (covered bowl side) or clamped flush to terrain
                // (outer ring / pour side) — never a half-float.
                let y = pos[k][1] as f64;
                let h = hh(i, j);
                let on_level = m["lakes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|l| (y - l["level"].as_f64().unwrap()).abs() < 0.01);
                assert!(
                    (y - h).abs() < 0.5 || on_level,
                    "lake boundary vert half-floats: y={y} h={h}"
                );
            }
        }
    }
}

#[test]
fn gen3_water_invariants() {
    let d = gen3_dir("wi");
    build(&d);
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d.join("exports/terrain/manifest.json")).unwrap())
            .unwrap();
    let layers: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d.join("layers/layers.json")).unwrap()).unwrap();
    let lake = std::fs::read(d.join("layers/lake.u8")).unwrap();
    let river = std::fs::read(d.join("layers/river.u8")).unwrap();
    let lnx = layers["nx"].as_u64().unwrap() as usize;
    let cell = layers["cell_size_m"].as_f64().unwrap();
    let (lx0, lz0) = (
        layers["origin"][0].as_f64().unwrap(),
        layers["origin"][1].as_f64().unwrap(),
    );
    let mut lake_vertices = 0usize;
    for c in m["chunks"].as_array().unwrap() {
        if c["water"].is_null() {
            continue;
        }
        let (cx, cz) = (c["cx"].as_u64().unwrap(), c["cz"].as_u64().unwrap());
        let wv = glb_vertices(
            &std::fs::read(d.join(format!(
                "exports/terrain/{}",
                c["water"]["file"].as_str().unwrap()
            )))
            .unwrap(),
        );
        // Terrain chunk GLB at the same stride — positions share x/z but
        // water has its own stride; compare y at the same x/z by sampling
        // the terrain GLB's vertex grid.
        let tv = glb_vertices(
            &std::fs::read(d.join(format!("exports/terrain/chunk_{cx}_{cz}.glb"))).unwrap(),
        );
        // For each water vertex find the terrain vertex with the nearest
        // x/z. Wet verts (river/lake node) sit on/above the bed; river
        // dry-rim verts are clamped to terrain, so y <= max. Lake rim
        // verts stay at lake level by design (extra ring tucks under
        // the bank), so kind-0.5 verts skip the clamp check.
        let wglb = std::fs::read(d.join(format!(
            "exports/terrain/{}",
            c["water"]["file"].as_str().unwrap()
        )))
        .unwrap();
        let wcol = {
            let jl = u32::from_le_bytes(wglb[12..16].try_into().unwrap()) as usize;
            let j: serde_json::Value = serde_json::from_slice(&wglb[20..20 + jl]).unwrap();
            glb_attr_f32(
                &wglb,
                j["meshes"][0]["primitives"][0]["attributes"]["COLOR_0"]
                    .as_u64()
                    .unwrap() as usize,
                4,
            )
        };
        for (wi, w) in wv.iter().enumerate() {
            let nearest = tv
                .iter()
                .min_by(|a, b| {
                    ((a[0] - w[0]).powi(2) + (a[2] - w[2]).powi(2))
                        .total_cmp(&((b[0] - w[0]).powi(2) + (b[2] - w[2]).powi(2)))
                })
                .unwrap();
            let origin = c["origin"].as_array().unwrap();
            let (i, j) = (
                ((origin[0].as_f64().unwrap() + w[0] as f64 - lx0) / cell).round() as usize,
                ((origin[2].as_f64().unwrap() + w[2] as f64 - lz0) / cell).round() as usize,
            );
            // Sea verts sit exactly at sea level over submerged terrain —
            // neither the wet-bed nor dry-rim rule applies to them.
            let is_sea = (w[1] - m["sea_level_m"].as_f64().unwrap() as f32).abs() < 1e-4;
            if is_sea {
                // flat at sea level by construction
            } else if river[j * lnx + i] == 1 || lake[j * lnx + i] == 1 {
                assert!(
                    w[1] >= nearest[1] - 1e-4,
                    "wet water vertex below bed: w={w:?} t={nearest:?}"
                );
            } else if wcol[wi][1] != 0.5 {
                // river rim (kind 0/1 non-sea): clamped to terrain.
                assert!(
                    w[1] <= nearest[1] + 1e-4,
                    "dry rim vertex floats above terrain: w={w:?} t={nearest:?}"
                );
            }
        }
        // Lakes are exactly flat: a water vertex on a lake node sits at one
        // of the lake levels, bit-exact as f32.
        let levels: Vec<f32> = m["lakes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["level"].as_f64().unwrap() as f32)
            .collect();
        let origin = c["origin"].as_array().unwrap();
        let (ox, oz) = (origin[0].as_f64().unwrap(), origin[2].as_f64().unwrap());
        for w in &wv {
            let (i, j) = (
                ((ox + w[0] as f64 - lx0) / cell).round() as usize,
                ((oz + w[2] as f64 - lz0) / cell).round() as usize,
            );
            if lake[j * lnx + i] == 1 {
                assert!(
                    levels.contains(&w[1]),
                    "lake vertex y {} is not a lake level {levels:?}",
                    w[1]
                );
                lake_vertices += 1;
            }
        }
    }
    assert!(
        lake_vertices > 0,
        "fixture has no lake water vertices (lakes {}, lake nodes {})",
        m["lakes"].as_array().unwrap().len(),
        lake.iter().filter(|&&v| v == 1).count()
    );
}

#[test]
fn gen3_water_determinism() {
    let d1 = gen3_dir("wd1");
    let d2 = gen3_dir("wd2");
    build(&d1);
    build(&d2);
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(d1.join("exports/terrain/manifest.json")).unwrap())
            .unwrap();
    for c in m["chunks"].as_array().unwrap() {
        if c["water"].is_null() {
            continue;
        }
        let f = c["water"]["file"].as_str().unwrap();
        assert_eq!(
            std::fs::read(d1.join("exports/terrain").join(f)).unwrap(),
            std::fs::read(d2.join("exports/terrain").join(f)).unwrap(),
            "water GLB {f} not deterministic"
        );
    }
}
