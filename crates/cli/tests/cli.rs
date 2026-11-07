use std::path::PathBuf;
use std::process::Command;

fn mybuilder() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mybuilder"))
}

fn workspace_target() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/cli-it")
}

#[test]
fn new_validate_hash_is_stable() {
    let base = workspace_target();
    std::fs::create_dir_all(&base).unwrap();
    let run = |n: &str| -> String {
        let dir = base.join(n);
        let _ = std::fs::remove_dir_all(&dir);
        let out = mybuilder()
            .args(["new", dir.to_str().unwrap(), "--seed", "0000000000000001"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = mybuilder()
            .args(["validate", dir.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let out = mybuilder()
            .args(["hash", dir.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(out.status.success());
        String::from_utf8(out.stdout).unwrap()
    };
    let h1 = run("proj-a");
    let h2 = run("proj-b");
    assert_eq!(h1, h2, "identical seeds must give identical hashes");
    assert!(h1.contains("project "));
    assert!(h1.contains("world   "));
}

#[test]
fn house_material_set_and_clear() {
    let base = workspace_target();
    let dir = base.join("material");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&base).unwrap();
    let d = dir.to_str().unwrap();
    let ok = |args: &[&str]| {
        let out = mybuilder().args(args).output().unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    ok(&["new", d, "--seed", "0000000000000001"]);
    ok(&["house", "add", d, "--id", "a"]);
    let recipe = || std::fs::read_to_string(dir.join("recipes/houses.json")).unwrap();
    // A non-image is rejected and nothing is written.
    let bad = base.join("material_bad.png");
    std::fs::write(&bad, b"not a png").unwrap();
    let before = recipe();
    let out = mybuilder()
        .args([
            "house",
            "material",
            "set",
            d,
            "--slot",
            "plaster",
            "--texture",
        ])
        .arg(&bad)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(recipe(), before);
    assert!(!dir.join("assets/textures").exists());
    // A PNG is copied in and recorded.
    let good = base.join("material_good.png");
    std::fs::write(&good, b"\x89PNG\r\n\x1a\nrest").unwrap();
    let out = mybuilder()
        .args([
            "house",
            "material",
            "set",
            d,
            "--slot",
            "wood",
            "--tile",
            "0.8",
            "--texture",
        ])
        .arg(&good)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read(dir.join("assets/textures/wood.png")).unwrap(),
        std::fs::read(&good).unwrap()
    );
    let r: serde_json::Value = serde_json::from_str(&recipe()).unwrap();
    assert_eq!(r["materials"][0]["slot"], "wood");
    assert_eq!(r["materials"][0]["texture"], "assets/textures/wood.png");
    assert_eq!(r["materials"][0]["tile_m"], 0.8);
    // An out-of-range tile is rejected without touching the recipe.
    let before = recipe();
    let out = mybuilder()
        .args([
            "house",
            "material",
            "set",
            d,
            "--slot",
            "stone",
            "--tile",
            "50",
            "--texture",
        ])
        .arg(&good)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(recipe(), before);
    assert!(!dir.join("assets/textures/stone.png").exists());
    ok(&["house", "material", "clear", d, "--slot", "wood"]);
    let r: serde_json::Value = serde_json::from_str(&recipe()).unwrap();
    assert!(r.get("materials").is_none());
    ok(&["validate", d]);
}
