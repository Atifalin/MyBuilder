//! Enforcement: every golden file's content must equal the `new` hash of
//! its LAST changelog row. Forgetting to bless-log a change fails CI.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_golden_matches_last_changelog_row() {
    let root = repo_root();
    let changelog = std::fs::read_to_string(root.join("GOLDEN_CHANGELOG.md")).unwrap();
    // golden name -> last `new` value
    let mut last: BTreeMap<String, String> = BTreeMap::new();
    for line in changelog.lines() {
        let line = line.trim();
        if !line.starts_with('|') || line.starts_with("|---") {
            continue;
        }
        // split on '|': index 0 is empty (leading pipe), then
        // date, generator, golden, old, new, reason.
        let cols: Vec<&str> = line.split('|').map(|c| c.trim()).collect();
        if cols.len() < 7 || cols[1].contains("date") {
            continue;
        }
        last.insert(cols[3].to_string(), cols[5].to_string());
    }
    let mut missing = Vec::new();
    let mut mismatched = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).unwrap() {
        let crate_dir = entry.unwrap().path();
        let gdir = crate_dir.join("tests/golden");
        if !gdir.is_dir() {
            continue;
        }
        let cname = crate_dir.file_name().unwrap().to_string_lossy().to_string();
        for g in std::fs::read_dir(&gdir).unwrap() {
            let g = g.unwrap().path();
            if g.extension().and_then(|e| e.to_str()) != Some("blake3") {
                continue;
            }
            let name = format!("{cname}/{}", g.file_stem().unwrap().to_string_lossy());
            let content = std::fs::read_to_string(&g).unwrap().trim().to_string();
            match last.get(&name) {
                None => missing.push(name),
                Some(v) if *v != content => {
                    mismatched.push(format!("{name}: file {content} != last row {v}"))
                }
                _ => {}
            }
        }
    }
    assert!(
        missing.is_empty(),
        "goldens with no changelog row: {missing:?}"
    );
    assert!(
        mismatched.is_empty(),
        "stale changelog rows: {mismatched:?}"
    );
}
