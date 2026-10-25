//! new_project tests: feels x starts build, land fraction, feel ordering,
//! determinism, rejection.

use std::path::{Path, PathBuf};

use mb_bridge::{NewProjectOptions, WorldStart, new_project};
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
    let d = std::env::temp_dir().join(format!("mb_np_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    TestDir(d)
}

fn opts(feel: TerrainFeel, start: WorldStart) -> NewProjectOptions {
    NewProjectOptions {
        name: "T".into(),
        seed_hex: Some("0000000000000001".into()),
        size_m: 1024.0,
        feel,
        start,
    }
}

fn build(dir: &Path) -> serde_json::Value {
    let cancel = mb_bridge::CancelToken::new();
    let out = mb_bridge::build(
        dir,
        &mb_bridge::BuildRequest {
            mode: mb_bridge::BuildMode::Incremental,
            cache_dir: None,
        },
        &cancel,
        &mut |_| {},
    );
    assert_eq!(out.status, "ok", "build failed: {}", out.status);
    serde_json::from_slice(&std::fs::read(dir.join("exports/terrain/manifest.json")).unwrap())
        .unwrap()
}

fn land_fraction(dir: &Path) -> f64 {
    let land = std::fs::read(dir.join("layers/land.u8")).unwrap();
    land.iter().filter(|&&v| v == 1).count() as f64 / land.len() as f64
}

#[test]
fn np_feels_starts_build() {
    for feel in [
        TerrainFeel::Bumpy,
        TerrainFeel::Hilly,
        TerrainFeel::Mountainous,
    ] {
        for start in [WorldStart::WaterFirst, WorldStart::LandFirst] {
            let d = tmpdir("fs");
            new_project(&d.join("p"), &opts(feel, start)).unwrap();
            let m = build(&d.join("p"));
            assert!(m["chunks"].as_array().unwrap().len() == 16); // 1024/256 -> 4x4
        }
    }
}

#[test]
fn np_land_first_more_land() {
    let dw = tmpdir("lw");
    new_project(
        &dw.join("p"),
        &opts(TerrainFeel::Hilly, WorldStart::WaterFirst),
    )
    .unwrap();
    build(&dw.join("p"));
    let dl = tmpdir("ll");
    new_project(
        &dl.join("p"),
        &opts(TerrainFeel::Hilly, WorldStart::LandFirst),
    )
    .unwrap();
    build(&dl.join("p"));
    let fw = land_fraction(&dw.join("p"));
    let fl = land_fraction(&dl.join("p"));
    assert!(fl > fw, "land fraction land-first {fl} <= water-first {fw}");
}

#[test]
fn np_feel_relief_ordered() {
    let mut hmax = Vec::new();
    for feel in [
        TerrainFeel::Bumpy,
        TerrainFeel::Hilly,
        TerrainFeel::Mountainous,
    ] {
        let d = tmpdir("fo");
        new_project(&d.join("p"), &opts(feel, WorldStart::WaterFirst)).unwrap();
        let m = build(&d.join("p"));
        hmax.push(m["h_max"].as_f64().unwrap());
    }
    assert!(
        hmax[0] < hmax[1] && hmax[1] < hmax[2],
        "relief not ordered bumpy<hilly<mountainous: {hmax:?}"
    );
}

#[test]
fn np_deterministic() {
    let d1 = tmpdir("d1");
    let d2 = tmpdir("d2");
    new_project(
        &d1.join("p"),
        &opts(TerrainFeel::Mountainous, WorldStart::WaterFirst),
    )
    .unwrap();
    new_project(
        &d2.join("p"),
        &opts(TerrainFeel::Mountainous, WorldStart::WaterFirst),
    )
    .unwrap();
    for f in ["project.json", "recipes/world.json"] {
        assert_eq!(
            std::fs::read(d1.join("p").join(f)).unwrap(),
            std::fs::read(d2.join("p").join(f)).unwrap(),
            "{f} differs"
        );
    }
}

#[test]
fn np_rejects_bad_size_and_existing() {
    let d = tmpdir("rj");
    let mut o = opts(TerrainFeel::Hilly, WorldStart::WaterFirst);
    o.size_m = 1500.0;
    assert!(new_project(&d.join("p"), &o).is_err());
    assert!(!d.join("p").exists(), "rejected create left files");
    o.size_m = 1024.0;
    new_project(&d.join("p"), &o).unwrap();
    assert!(
        new_project(&d.join("p"), &o).is_err(),
        "existing project accepted"
    );
}
