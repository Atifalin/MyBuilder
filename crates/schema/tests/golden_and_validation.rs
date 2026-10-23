use mb_schema::*;

const DIR: &str = env!("CARGO_MANIFEST_DIR");

fn world() -> WorldRecipe {
    WorldRecipe::default()
}

fn project() -> ProjectFile {
    ProjectFile {
        seed: 1,
        ..ProjectFile::default()
    }
}

fn codes(diags: &[Diagnostic]) -> Vec<&'static str> {
    diags.iter().map(|d| d.code).collect()
}

#[test]
fn default_project_is_valid() {
    let diags = validate_project(&project(), &world());
    assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
}

#[test]
fn golden_canonical_hashes() {
    let p = project();
    let w = world();
    mb_seed::golden::check_hex(
        DIR,
        "canonical_project",
        &mb_seed::hex_bytes(&canonical_hash(&p)),
    );
    mb_seed::golden::check_hex(
        DIR,
        "canonical_world",
        &mb_seed::hex_bytes(&canonical_hash(&w)),
    );
}

// --- validation rules, one test per code ---

#[test]
fn err_extent_non_positive() {
    let mut w = world();
    w.extent.width_m = 0.0;
    assert!(codes(&validate_project(&project(), &w)).contains(&"world.extent.non_positive"));
}

#[test]
fn err_chunk_not_power_of_two() {
    let mut w = world();
    w.chunk_size_m = 300.0;
    assert!(codes(&validate_project(&project(), &w)).contains(&"world.chunk_size.invalid"));
}

#[test]
fn err_chunk_larger_than_extent() {
    let mut w = world();
    w.chunk_size_m = 4096.0;
    assert!(codes(&validate_project(&project(), &w)).contains(&"world.chunk_size.invalid"));
}

#[test]
fn err_extent_not_multiple_of_chunk() {
    let mut w = world();
    w.extent.width_m = 2048.0 + 128.0;
    assert!(
        codes(&validate_project(&project(), &w)).contains(&"world.extent.not_multiple_of_chunk")
    );
}

#[test]
fn err_cell_size_non_positive() {
    let mut p = project();
    p.world_scale.cell_size_m = 0.0;
    assert!(
        codes(&validate_project(&p, &world()))
            .contains(&"project.world_scale.cell_size.non_positive")
    );
}

#[test]
fn err_chunk_not_multiple_of_cell() {
    let mut p = project();
    p.world_scale.cell_size_m = 3.0; // 256 / 3 is not an integer
    assert!(
        codes(&validate_project(&p, &world())).contains(&"world.chunk_size.not_multiple_of_cell")
    );
}

#[test]
fn err_shape_too_few_points() {
    let mut w = world();
    w.shapes[0].points.truncate(2);
    assert!(codes(&validate_project(&project(), &w)).contains(&"world.shape.too_few_points"));
}

#[test]
fn err_shape_non_finite() {
    let mut w = world();
    w.shapes[0].points[0][0] = f64::NAN;
    assert!(codes(&validate_project(&project(), &w)).contains(&"world.shape.non_finite"));
}

#[test]
fn err_shape_duplicate_id() {
    let mut w = world();
    let dup = w.shapes[0].clone();
    w.shapes.push(dup);
    assert!(codes(&validate_project(&project(), &w)).contains(&"world.shape.duplicate_id"));
}

#[test]
fn err_shape_empty_id() {
    let mut w = world();
    w.shapes[0].id = String::new();
    assert!(codes(&validate_project(&project(), &w)).contains(&"world.shape.empty_id"));
}

#[test]
fn err_meters_per_unit() {
    let mut p = project();
    p.world_scale.meters_per_unit = 2.0;
    assert!(
        codes(&validate_project(&p, &world()))
            .contains(&"project.world_scale.meters_per_unit.unsupported")
    );
}

#[test]
fn warn_shape_outside_extent() {
    let mut w = world();
    w.shapes[0].points[0] = [5000.0, 0.0];
    assert!(codes(&validate_project(&project(), &w)).contains(&"world.shape.outside_extent"));
}

#[test]
fn warn_extent_very_large() {
    let mut w = world();
    w.extent.width_m = 65536.0;
    w.chunk_size_m = 1024.0;
    assert!(codes(&validate_project(&project(), &w)).contains(&"world.extent.very_large"));
}

#[test]
fn warn_terrain_override_out_of_range() {
    let mut w = world();
    w.terrain.overrides.octaves = Some(50);
    assert!(
        codes(&validate_project(&project(), &w)).contains(&"world.terrain.override_out_of_range")
    );
}

// --- round trip, versioning, project folder ---

#[test]
fn project_round_trip() {
    let p = project();
    let json = serde_json::to_string_pretty(&p).unwrap();
    let back: ProjectFile = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
    assert!(json.contains("\"seed\": \"0000000000000001\""));
    let w = world();
    let wjson = serde_json::to_string_pretty(&w).unwrap();
    let wback: WorldRecipe = serde_json::from_str(&wjson).unwrap();
    assert_eq!(w, wback);
}

#[test]
fn future_version_is_rejected() {
    let dir = std::env::temp_dir().join(format!("mb-schema-future-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("project.json");
    std::fs::write(
        &path,
        serde_json::json!({"schema_version": CURRENT_SCHEMA_VERSION + 1}).to_string(),
    )
    .unwrap();
    let err = read_versioned_pub(&path).unwrap_err();
    assert!(
        matches!(err, SchemaError::UnsupportedVersion { .. }),
        "{err:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn version_zero_and_missing_are_rejected() {
    let dir = std::env::temp_dir().join(format!("mb-schema-v0-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for body in [
        serde_json::json!({"schema_version": 0}).to_string(),
        serde_json::json!({"name": "x"}).to_string(),
    ] {
        let path = dir.join("project.json");
        std::fs::write(&path, body).unwrap();
        let err = read_versioned_pub(&path).unwrap_err();
        assert!(matches!(err, SchemaError::MissingVersion { .. }), "{err:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// read_versioned is private; exercise it through load_project's first step
// by calling the public wrapper below.
fn read_versioned_pub(path: &std::path::Path) -> Result<serde_json::Value, SchemaError> {
    mb_schema::load_json_file(path)
}

#[test]
fn create_and_load_project() {
    let dir = std::env::temp_dir().join(format!("mb-schema-proj-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, &project(), &world()).unwrap();
    // Refuses to overwrite.
    assert!(matches!(
        create_project(&dir, &project(), &world()),
        Err(SchemaError::ProjectExists(_))
    ));
    for sub in SUBDIRS {
        assert!(dir.join(sub).join(".gitkeep").exists());
    }
    let (p, w, diags) = load_project(&dir).unwrap();
    assert_eq!(p, project());
    assert_eq!(w, world());
    assert!(diags.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

// --- recipe path confinement ---

fn project_with_recipe_path(path: &str) -> ProjectFile {
    let mut p = project();
    p.recipes.world = path.to_string();
    p
}

fn assert_invalid_recipe_path(path: &str, tag: &str) {
    let p = project_with_recipe_path(path);
    assert!(
        codes(&validate_project(&p, &world())).contains(&"project.recipes.world.invalid_path"),
        "expected invalid_path diagnostic for {path:?}"
    );
    let dir = std::env::temp_dir().join(format!("mb-schema-rp-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    create_project(&dir, &project(), &world()).unwrap();
    // Point the on-disk project at the bad path, then reload.
    let mut proj = p.clone();
    proj.recipes.world = path.to_string();
    std::fs::write(
        dir.join("project.json"),
        serde_json::to_string_pretty(&proj).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        load_project(&dir),
        Err(SchemaError::InvalidRecipePath(_))
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn err_recipe_path_absolute() {
    assert_invalid_recipe_path("/etc/passwd", "abs");
    assert_invalid_recipe_path("\\Windows\\win.ini", "abs_backslash");
    assert_invalid_recipe_path("C:\\outside.json", "abs_drive");
    assert_invalid_recipe_path("C:outside.json", "drive_relative");
    assert_invalid_recipe_path("recipes\\..\\..\\outside.json", "parent_backslash");
}

#[test]
fn err_recipe_path_parent_component() {
    assert_invalid_recipe_path("../outside.json", "parent1");
    assert_invalid_recipe_path("recipes/../../outside.json", "parent2");
}

#[test]
fn err_recipe_path_empty() {
    assert_invalid_recipe_path("", "empty");
}

// --- detail region rules ---

fn region(id: &str) -> DetailRegion {
    DetailRegion {
        id: id.into(),
        chunk_min: [1, 1],
        chunk_max: [2, 2],
        cell_divisor: 2,
        mesh_step_cells: 1,
        extra_octaves: 2,
        blend_band_m: 16.0,
    }
}

fn w_region(r: DetailRegion) -> WorldRecipe {
    let mut w = world();
    w.detail_regions = vec![r];
    w
}

#[test]
fn err_region_empty_id() {
    let mut r = region("x");
    r.id = String::new();
    assert!(codes(&validate_project(&project(), &w_region(r))).contains(&"world.region.empty_id"));
}
#[test]
fn err_region_duplicate_id() {
    let mut w = world();
    w.detail_regions = vec![region("a"), region("a")];
    assert!(codes(&validate_project(&project(), &w)).contains(&"world.region.duplicate_id"));
}
#[test]
fn err_region_chunk_range() {
    let mut r = region("x");
    r.chunk_max = [9, 9]; // grid is 8x8
    assert!(
        codes(&validate_project(&project(), &w_region(r))).contains(&"world.region.chunk_range")
    );
    let mut r2 = region("y");
    r2.chunk_min = [2, 2];
    r2.chunk_max = [1, 1];
    assert!(
        codes(&validate_project(&project(), &w_region(r2))).contains(&"world.region.chunk_range")
    );
}
#[test]
fn err_region_overlap() {
    let mut w = world();
    let mut r2 = region("b");
    r2.chunk_min = [2, 2];
    r2.chunk_max = [3, 3];
    w.detail_regions = vec![region("a"), r2];
    assert!(codes(&validate_project(&project(), &w)).contains(&"world.region.overlap"));
}
#[test]
fn err_region_cell_divisor() {
    let mut r = region("x");
    r.cell_divisor = 3;
    assert!(
        codes(&validate_project(&project(), &w_region(r))).contains(&"world.region.cell_divisor")
    );
}
#[test]
fn err_region_mesh_step() {
    // coarse spacing 4 m; fine = 1/2 * 3 = 1.5 -> 4/1.5 not integer
    let mut r = region("x");
    r.mesh_step_cells = 3;
    assert!(codes(&validate_project(&project(), &w_region(r))).contains(&"world.region.mesh_step"));
}
#[test]
fn err_region_extra_octaves() {
    let mut r = region("x");
    r.extra_octaves = 7;
    assert!(
        codes(&validate_project(&project(), &w_region(r))).contains(&"world.region.extra_octaves")
    );
}
#[test]
fn err_region_blend_band() {
    let mut r = region("x");
    r.blend_band_m = 0.0;
    assert!(
        codes(&validate_project(&project(), &w_region(r))).contains(&"world.region.blend_band")
    );
    let mut r2 = region("y");
    r2.blend_band_m = 300.0; // side 512, half=256
    assert!(
        codes(&validate_project(&project(), &w_region(r2))).contains(&"world.region.blend_band")
    );
}

#[test]
fn shape_self_intersection_and_size_limits() {
    let mut w = WorldRecipe {
        shapes: vec![Shape {
            id: "bowtie".into(),
            kind: ShapeKind::Land,
            points: vec![[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]],
        }],
        ..WorldRecipe::default()
    };
    let mut d = Vec::new();
    validate_world(&w, 1.0, &mut d);
    assert!(d.iter().any(|x| x.code == "world.shape.self_intersecting"));
    w.shapes[0].points = (0..4100)
        .map(|i| [i as f64 % 100.0, i as f64 / 100.0])
        .collect();
    let mut d2 = Vec::new();
    validate_world(&w, 1.0, &mut d2);
    assert!(
        d2.iter()
            .any(|x| x.code == "world.shape.too_many_points" && x.severity == Severity::Error)
    );
}
