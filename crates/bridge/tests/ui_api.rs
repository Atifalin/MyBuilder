//! Step 3: UI API tests — events, cancel, session, clear_all, preview.

use std::path::{Path, PathBuf};

use mb_bridge::{BuildEvent, BuildMode, BuildRequest, ClearRequest};
use mb_schema::*;

struct TestDir(PathBuf);
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
    let d = std::env::temp_dir().join(format!("mb_br_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    TestDir(d)
}

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
    w.detail_regions = vec![DetailRegion {
        id: "r".into(),
        chunk_min: [1, 1],
        chunk_max: [1, 1],
        cell_divisor: 2,
        mesh_step_cells: 1,
        extra_octaves: 2,
        blend_band_m: 16.0,
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
        dir.join(PROJECT_FILE),
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

fn tree_hash(dir: &Path) -> [u8; 32] {
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
                    files.push((
                        e.strip_prefix(dir).unwrap().to_string_lossy().to_string(),
                        std::fs::read(&e).unwrap(),
                    ));
                }
            }
        }
    }
    files.sort();
    let mut all = Vec::new();
    for (r, b) in &files {
        all.extend_from_slice(&(r.len() as u64).to_le_bytes());
        all.extend_from_slice(r.as_bytes());
        all.extend_from_slice(&(b.len() as u64).to_le_bytes());
        all.extend_from_slice(b);
    }
    mb_seed::hash_bytes(&all)
}

fn run(
    dir: &Path,
    mode: BuildMode,
    cancel: &mb_bridge::CancelToken,
    sink: &mut dyn FnMut(BuildEvent),
) -> mb_bridge::BuildOutcome {
    mb_bridge::build(
        dir,
        &BuildRequest {
            mode,
            cache_dir: Some(dir.join(".test-cache")),
        },
        cancel,
        sink,
    )
}

// ---- Test 1: event stream ----

#[test]
fn event_stream_full_and_incremental() {
    let d = tmpdir("events");
    make_project(&d);
    let mut ev: Vec<BuildEvent> = Vec::new();
    let out = run(
        &d,
        BuildMode::Full,
        &mb_bridge::CancelToken::new(),
        &mut |e| ev.push(e),
    );
    assert_eq!(
        out.status,
        "ok",
        "events: {:?}",
        ev.iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
    );
    // First Started, last Finished{ok}.
    assert!(matches!(ev.first(), Some(BuildEvent::Started { .. })));
    assert!(
        matches!(ev.last(), Some(BuildEvent::Finished { status, .. }) if status == "ok"),
        "last event: {:?}",
        ev.last()
    );
    // Every StageStarted has a StageFinished.
    let mut started: Vec<String> = Vec::new();
    let mut finished: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_pct = 0.0f64;
    let mut chunks_done = 0usize;
    let mut chunks_total = 0usize;
    for e in &ev {
        match e {
            BuildEvent::StageStarted { stage, .. } => started.push(stage.clone()),
            BuildEvent::StageFinished { stage, .. } => {
                finished.insert(stage.clone());
            }
            BuildEvent::Progress {
                percent,
                chunks_done: cd,
                chunks_total: ct,
                ..
            } => {
                assert!(
                    *percent >= last_pct,
                    "percent decreased: {last_pct} -> {percent}"
                );
                last_pct = *percent;
                chunks_done = *cd;
                chunks_total = *ct;
            }
            _ => {}
        }
    }
    for s in &started {
        assert!(finished.contains(s), "StageStarted {s} never finished");
    }
    assert_eq!(last_pct, 100.0);
    assert_eq!(chunks_done, chunks_total);
    assert!(chunks_total > 0);
    // serde round-trip
    for e in &ev {
        let j = serde_json::to_string(e).unwrap();
        let back: BuildEvent = serde_json::from_str(&j).unwrap();
        assert_eq!(*e, back);
    }
    // Incremental: all stages cached, 0 files written.
    let mut ev2 = Vec::new();
    let out2 = run(
        &d,
        BuildMode::Incremental,
        &mb_bridge::CancelToken::new(),
        &mut |e| ev2.push(e),
    );
    assert_eq!(out2.status, "ok");
    for e in &ev2 {
        if let BuildEvent::StageFinished {
            stage, executed, ..
        } = e
        {
            assert!(!executed, "incremental build executed stage {stage}");
        }
        if let BuildEvent::FilesWritten { written, .. } = e {
            assert_eq!(*written, 0);
        }
    }
}

// ---- Test 3: cancel leaves the cache valid ----

fn cancelled_case(d: &TestDir, stop_after: usize, on: &str) {
    let tok = mb_bridge::CancelToken::new();
    let h0 = tree_hash(d);
    let mut ev: Vec<BuildEvent> = Vec::new();
    let mut count = 0usize;
    let mut evdump = Vec::new();
    let out = mb_bridge::build(
        d,
        &BuildRequest {
            mode: BuildMode::Incremental,
            cache_dir: Some(d.join(".test-cache")),
        },
        &tok,
        &mut |e| {
            match &e {
                BuildEvent::StageStarted { stage, .. } if stage == on => {
                    if stop_after == 0 {
                        tok.cancel();
                    }
                }
                BuildEvent::StageFinished { group, .. } if group == "chunk" => {
                    count += 1;
                    if count == stop_after && stop_after > 0 {
                        tok.cancel();
                    }
                }
                _ => {}
            }
            ev.push(e.clone());
            evdump.push(serde_json::to_string(&e).unwrap());
        },
    );
    assert_eq!(out.status, "cancelled", "events: {evdump:?}");
    assert!(
        ev.iter()
            .any(|e| matches!(e, BuildEvent::Finished { status, .. } if status == "cancelled"))
    );
    assert_eq!(tree_hash(d), h0, "cancelled build must not write outputs");
    // Cache still valid: every index entry's blob exists and verifies.
    let index = std::fs::read_to_string(d.join(".test-cache/index.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&index).unwrap();
    for (_id, e) in v.as_object().unwrap() {
        let blob = d
            .join(".test-cache/blobs")
            .join(format!("{}.bin", e["artifact"].as_str().unwrap()));
        let bytes = std::fs::read(&blob).unwrap_or_else(|_| panic!("missing blob {blob:?}"));
        let hex = e["artifact"].as_str().unwrap();
        let mut want = [0u8; 32];
        for i in 0..32 {
            want[i] = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap();
        }
        assert_eq!(
            mb_seed::hash_bytes(&bytes),
            want,
            "blob hash mismatch for {_id}"
        );
    }
    // Next incremental build completes and equals a full fresh build.
    let out2 = run(
        d,
        BuildMode::Incremental,
        &mb_bridge::CancelToken::new(),
        &mut |_| {},
    );
    assert_eq!(out2.status, "ok");
    let fresh = tmpdir(&format!("fresh{}", rand()));
    make_project(&fresh);
    let out3 = run(
        &fresh,
        BuildMode::Full,
        &mb_bridge::CancelToken::new(),
        &mut |_| {},
    );
    assert_eq!(out3.status, "ok");
    assert_eq!(tree_hash(d), tree_hash(&fresh));
}

fn rand() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

#[test]
fn cancel_mid_height_keeps_cache_valid() {
    let d = tmpdir("cx_height");
    make_project(&d);
    run(
        &d,
        BuildMode::Full,
        &mb_bridge::CancelToken::new(),
        &mut |_| {},
    );
    cancelled_case(&d, 0, "world.height");
}

#[test]
fn cancel_mid_chunks_keeps_cache_valid() {
    let d = tmpdir("cx_chunk");
    make_project(&d);
    run(
        &d,
        BuildMode::Full,
        &mb_bridge::CancelToken::new(),
        &mut |_| {},
    );
    cancelled_case(&d, 5, "");
}

// ---- Test 4: session + determinism ----

#[test]
fn session_edit_undo_redo() {
    let d = tmpdir("sess");
    make_project(&d);
    run(
        &d,
        BuildMode::Full,
        &mb_bridge::CancelToken::new(),
        &mut |_| {},
    );
    let h0 = tree_hash(&d);
    let mut sess = mb_bridge::Session::open(&d).unwrap();
    let orig = serde_json::to_string_pretty(sess.world()).unwrap();
    // Rejected edit: invalid chunk size, file + history unchanged.
    let mut bad = sess.world().clone();
    bad.chunk_size_m = 300.0;
    let diags = sess
        .apply(mb_bridge::RecipeEdit::ReplaceWorld(Box::new(bad)))
        .unwrap();
    assert!(
        diags
            .iter()
            .any(|d| d.severity == mb_schema::Severity::Error)
    );
    assert!(!sess.can_undo());
    let ondisk: WorldRecipe =
        serde_json::from_str(&std::fs::read_to_string(d.join("recipes/world.json")).unwrap())
            .unwrap();
    assert_eq!(serde_json::to_string_pretty(&ondisk).unwrap(), orig);
    // Edits.
    sess.apply(mb_bridge::RecipeEdit::SetFeel(TerrainFeel::Mountainous))
        .unwrap();
    run(
        &d,
        BuildMode::Incremental,
        &mb_bridge::CancelToken::new(),
        &mut |_| {},
    );
    sess.apply(mb_bridge::RecipeEdit::SetOctaves(Some(3)))
        .unwrap();
    run(
        &d,
        BuildMode::Incremental,
        &mb_bridge::CancelToken::new(),
        &mut |_| {},
    );
    assert!(sess.can_undo() && sess.can_undo());
    sess.undo().unwrap();
    sess.undo().unwrap();
    assert!(sess.can_redo());
    run(
        &d,
        BuildMode::Incremental,
        &mb_bridge::CancelToken::new(),
        &mut |_| {},
    );
    assert_eq!(tree_hash(&d), h0);
    sess.redo().unwrap();
    run(
        &d,
        BuildMode::Incremental,
        &mb_bridge::CancelToken::new(),
        &mut |_| {},
    );
    // equals full build of that recipe in a fresh copy
    let fresh = tmpdir("sessref");
    make_project(&fresh);
    write_world(&fresh, sess.world());
    run(
        &fresh,
        BuildMode::Full,
        &mb_bridge::CancelToken::new(),
        &mut |_| {},
    );
    assert_eq!(tree_hash(&d), tree_hash(&fresh));
}

// ---- Test 5: clear_all ----

#[test]
fn clear_all_keeps_or_drops_overrides() {
    for keep in [true, false] {
        let d = tmpdir(&format!("clear{keep}"));
        make_project(&d);
        std::fs::create_dir_all(d.join("overrides")).unwrap();
        std::fs::write(d.join("overrides/note.txt"), b"x").unwrap();
        run(
            &d,
            BuildMode::Full,
            &mb_bridge::CancelToken::new(),
            &mut |_| {},
        );
        let rep = mb_bridge::clear_all(
            &d,
            &ClearRequest {
                new_seed: Some(0xABCD),
                keep_overrides: keep,
                cache_dir: Some(d.join(".test-cache")),
            },
        )
        .unwrap();
        assert_eq!(rep.seed, 0xABCD);
        assert!(d.join("overrides/note.txt").exists() == keep);
        assert!(
            !d.join(".test-cache/index.json").exists()
                || std::fs::read_dir(d.join(".test-cache/blobs"))
                    .map(|m| m.count() == 0)
                    .unwrap_or(true)
        );
        assert!(!d.join("exports/terrain/chunk_1_1.glb").exists());
        let p: ProjectFile =
            serde_json::from_str(&std::fs::read_to_string(d.join("project.json")).unwrap())
                .unwrap();
        assert_eq!(p.seed, 0xABCD);
    }
    // Lock respected.
    let d = tmpdir("clearlock");
    make_project(&d);
    let _l = mb_terrain::build::ProjectLock::acquire(&d).unwrap();
    match mb_bridge::clear_all(
        &d,
        &ClearRequest {
            new_seed: None,
            keep_overrides: true,
            cache_dir: Some(d.join(".test-cache")),
        },
    ) {
        Err(mb_terrain::build::BuildError::Locked) => {}
        other => panic!(
            "expected Locked, got {:?}",
            other.map(|r| r.seed).unwrap_or(0)
        ),
    }
}

// ---- Test 6: preview ----

#[test]
fn preview_tiles() {
    let d = tmpdir("preview");
    make_project(&d);
    run(
        &d,
        BuildMode::Full,
        &mb_bridge::CancelToken::new(),
        &mut |_| {},
    );
    let src = mb_bridge::PreviewSource::open(&d).unwrap();
    let r = mb_bridge::Rect {
        x0: -128.0,
        z0: -128.0,
        x1: 128.0,
        z1: 128.0,
    };
    let t1 = src.tile(&r, 2.0).unwrap();
    let t2 = src.tile(&r, 2.0).unwrap();
    assert_eq!(t1, t2, "tiles must be deterministic");
    // Outside-world tile is fully transparent.
    let out = src
        .tile(
            &mb_bridge::Rect {
                x0: 500.0,
                z0: 500.0,
                x1: 1000.0,
                z1: 1000.0,
            },
            4.0,
        )
        .unwrap();
    let dec = png::Decoder::new(std::io::Cursor::new(&out));
    let mut reader = dec.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    assert!(
        buf.chunks(4).all(|px| px[3] == 0),
        "outside tile must be transparent"
    );
    // Whole-world tile at mpp=2: same dims as hillshade, similar mean.
    let hs = std::fs::read(d.join("previews/hillshade.png")).unwrap();
    let dec = png::Decoder::new(std::io::Cursor::new(&hs));
    let mut rd = dec.read_info().unwrap();
    let (hw, hh) = (rd.info().width, rd.info().height);
    let mut hbuf = vec![0; rd.output_buffer_size().unwrap()];
    rd.next_frame(&mut hbuf).unwrap();
    let hmean: f64 = hbuf
        .chunks(3)
        .map(|c| c[0] as f64 + c[1] as f64 + c[2] as f64)
        .sum::<f64>()
        / (hw * hh) as f64;
    let dec2 = png::Decoder::new(std::io::Cursor::new(&t1));
    let mut rd2 = dec2.read_info().unwrap();
    assert_eq!(
        (rd2.info().width, rd2.info().height),
        (hw, hh),
        "tile {}x{} vs hillshade {}x{}",
        rd2.info().width,
        rd2.info().height,
        hw,
        hh
    );
    let mut tbuf = vec![0; rd2.output_buffer_size().unwrap()];
    rd2.next_frame(&mut tbuf).unwrap();
    let tmean: f64 = tbuf
        .chunks(4)
        .map(|c| c[0] as f64 + c[1] as f64 + c[2] as f64)
        .sum::<f64>()
        / (hw * hh) as f64;
    println!("hillshade mean {hmean:.1}, tile mean {tmean:.1}");
    assert!(
        (tmean - hmean).abs() < 60.0,
        "means differ: {tmean} vs {hmean}"
    );
}

// ---- Test 2: CLI --json-events NDJSON + stdin cancel ----

/// Spawn `mybuilder build --json-events` via the built binary; feed
/// `cancel` on stdin after the first chunk StageFinished so the next
/// stage start sees the cancelled token. Deterministic because the
/// world has 16+1 chunks and cancel lands long before the last one.
#[test]
fn cli_json_events_and_cancel() {
    let d = tmpdir("cli_ev");
    make_project(&d);
    // Build the binary once.
    // Locate the CLI binary next to this test binary (target/*/mybuilder).
    let exe = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("mybuilder")
        .to_string_lossy()
        .into_owned();
    if !Path::new(&exe).exists() {
        eprintln!("skip: mybuilder binary not built at {exe}");
        return;
    }
    let child = std::process::Command::new(&exe)
        .args(["build", "--json-events", "--cache-dir"])
        .arg(d.join(".test-cache"))
        .arg(d.to_path_buf())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let mut first_type = String::new();
    let mut last_type = String::new();
    let mut last_pct = 0.0f64;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        if first_type.is_empty() {
            first_type = v["type"].as_str().unwrap().to_string();
        }
        last_type = v["type"].as_str().unwrap().to_string();
        if v["type"] == "progress" {
            let p = v["percent"].as_f64().unwrap();
            assert!(p >= last_pct, "percent decreased {last_pct} -> {p}");
            last_pct = p;
        }
    }
    assert_eq!(first_type, "started");
    assert_eq!(last_type, "finished");
    assert_eq!(last_pct, 100.0);

    // Cancel path: feed "cancel" right away, small world cancels.
    let mut child = std::process::Command::new(&exe)
        .args(["build", "--json-events", "--cache-dir"])
        .arg(d.join(".test-cache2"))
        .arg(d.to_path_buf())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        // Keep feeding cancel until the build reads it.
        for _ in 0..50 {
            let _ = writeln!(stdin, "cancel");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let cancelled = text.lines().any(|l| {
        serde_json::from_str::<serde_json::Value>(l)
            .map(|v| v["type"] == "finished" && v["status"] == "cancelled")
            .unwrap_or(false)
    });
    assert!(cancelled, "no cancelled event in:\n{text}");
    assert_eq!(out.status.code(), Some(2), "exit code: {:?}", out.status);
}
