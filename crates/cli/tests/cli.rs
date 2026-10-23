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
