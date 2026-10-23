//! Project and world recipe types: defaults, versioning, validation and
//! canonical hashing.
//!
//! A project folder looks like:
//! ```text
//! project.json        ProjectFile
//! recipes/world.json  WorldRecipe
//! layers/ overrides/ cache/ styles/ previews/ exports/
//! ```

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Highest `schema_version` this build understands.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Deterministic canonical hash: BLAKE3 of the compact serde_json bytes of
/// a typed value. Because we serialize typed structs (never maps), field
/// order is fixed by the struct definitions and identical across
/// platforms. Typed structs in this crate must therefore never contain
/// maps with unspecified iteration order.
pub fn canonical_hash<T: Serialize>(value: &T) -> [u8; 32] {
    let bytes = serde_json::to_vec(value).expect("schema types always serialize");
    mb_seed::hash_bytes(&bytes)
}

// ---------------------------------------------------------------------
// project.json
// ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    pub schema_version: u32,
    pub name: String,
    /// Project seed, serialized as a 16-char lowercase hex string.
    #[serde(with = "mb_seed::serde_hex")]
    pub seed: u64,
    pub units: Units,
    pub up_axis: UpAxis,
    pub handedness: Handedness,
    pub world_scale: WorldScale,
    pub recipes: RecipePaths,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    Meters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpAxis {
    Y,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Handedness {
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldScale {
    /// Only 1.0 is supported in schema v1.
    pub meters_per_unit: f64,
    /// Vertex spacing of terrain chunks in meters.
    pub cell_size_m: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipePaths {
    /// Path of the world recipe, relative to the project dir.
    pub world: String,
}

impl Default for ProjectFile {
    fn default() -> Self {
        ProjectFile {
            schema_version: CURRENT_SCHEMA_VERSION,
            name: "My World".to_string(),
            seed: 0,
            units: Units::Meters,
            up_axis: UpAxis::Y,
            handedness: Handedness::Right,
            world_scale: WorldScale::default(),
            recipes: RecipePaths::default(),
        }
    }
}

impl Default for WorldScale {
    fn default() -> Self {
        WorldScale {
            meters_per_unit: 1.0,
            cell_size_m: 1.0,
        }
    }
}

impl Default for RecipePaths {
    fn default() -> Self {
        RecipePaths {
            world: "recipes/world.json".to_string(),
        }
    }
}

// ---------------------------------------------------------------------
// recipes/world.json
// ---------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldRecipe {
    pub schema_version: u32,
    /// World size in meters. The origin is the world center: x ranges over
    /// [-width/2, width/2], z over [-depth/2, depth/2].
    pub extent: Extent,
    /// Terrain chunk side length in meters; must be a power of two.
    pub chunk_size_m: f64,
    pub sea_level_m: f64,
    pub mode: WorldMode,
    pub terrain: TerrainSection,
    #[serde(default)]
    pub profile: TerrainProfile,
    #[serde(default)]
    pub export: ExportSettings,
    pub shapes: Vec<Shape>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extent {
    pub width_m: f64,
    pub depth_m: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldMode {
    LandFirst,
    WaterFirst,
}

/// Coast-to-inland height profile parameters (all meters).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainProfile {
    /// How deep the seabed gets.
    pub shelf_depth_m: f64,
    /// Horizontal distance over which the seabed reaches full depth.
    pub shelf_width_m: f64,
    /// Height right at the coastline.
    pub coast_height_m: f64,
    /// Height reached far inland before noise is added.
    pub inland_height_m: f64,
    /// Horizontal distance over which base height rises to inland_height.
    pub inland_width_m: f64,
    /// Extra height added inside mountain shapes at full weight.
    pub mountain_height_m: f64,
    /// Distance inside a mountain shape over which its weight ramps 0 -> 1.
    pub mountain_falloff_m: f64,
    /// Base noise wavelength in meters; noise frequency = 1 / feature_size.
    pub feature_size_m: f64,
}

impl Default for TerrainProfile {
    fn default() -> Self {
        TerrainProfile {
            shelf_depth_m: 40.0,
            shelf_width_m: 300.0,
            coast_height_m: 2.0,
            inland_height_m: 30.0,
            inland_width_m: 400.0,
            mountain_height_m: 600.0,
            mountain_falloff_m: 250.0,
            feature_size_m: 512.0,
        }
    }
}

/// Export-time settings (affect mesh files, not layer data).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportSettings {
    /// Vertex stride of exported chunk meshes, in grid cells. Data stays
    /// at 1 m cells; meshes are lighter. Must be a power of two that
    /// divides chunk_size_m / cell_size_m.
    pub mesh_step_cells: u32,
}

impl Default for ExportSettings {
    fn default() -> Self {
        ExportSettings { mesh_step_cells: 4 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainSection {
    pub feel: TerrainFeel,
    #[serde(default)]
    pub overrides: TerrainOverrides,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerrainFeel {
    Bumpy,
    Hilly,
    Mountainous,
}

/// Optional per-field overrides applied on top of the feel preset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub octaves: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lacunarity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relief_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ridged_mix: Option<f64>,
}

/// Fully resolved terrain parameters (feel preset + overrides).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainParams {
    pub octaves: u32,
    pub gain: f64,
    pub lacunarity: f64,
    /// Total relief amplitude in meters.
    pub relief_m: f64,
    /// Blend factor between smooth fBm and ridged fBm, 0..=1.
    pub ridged_mix: f64,
}

impl TerrainFeel {
    /// Preset parameters per feel (algorithms doc 3.3).
    pub fn preset(&self) -> TerrainParams {
        match self {
            TerrainFeel::Bumpy => TerrainParams {
                octaves: 4,
                gain: 0.45,
                lacunarity: 2.0,
                relief_m: 20.0,
                ridged_mix: 0.0,
            },
            TerrainFeel::Hilly => TerrainParams {
                octaves: 5,
                gain: 0.50,
                lacunarity: 2.0,
                relief_m: 120.0,
                ridged_mix: 0.15,
            },
            TerrainFeel::Mountainous => TerrainParams {
                octaves: 7,
                gain: 0.55,
                lacunarity: 2.0,
                relief_m: 800.0,
                ridged_mix: 0.6,
            },
        }
    }

    /// Preset with the user's per-field overrides applied.
    pub fn resolve(&self, overrides: &TerrainOverrides) -> TerrainParams {
        let p = self.preset();
        TerrainParams {
            octaves: overrides.octaves.unwrap_or(p.octaves),
            gain: overrides.gain.unwrap_or(p.gain),
            lacunarity: overrides.lacunarity.unwrap_or(p.lacunarity),
            relief_m: overrides.relief_m.unwrap_or(p.relief_m),
            ridged_mix: overrides.ridged_mix.unwrap_or(p.ridged_mix),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Land,
    Water,
    Mountain,
    Forest,
}

/// A 2D map shape (coast, lake, mountain range, forest zone).
/// `points` are `[x, z]` pairs in world meters, origin at world center.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shape {
    pub id: String,
    pub kind: ShapeKind,
    pub points: Vec<[f64; 2]>,
}

impl Default for WorldRecipe {
    /// The default world carries one centered land blob so that
    /// "no-world mode" still has land to build on. The twelve points are
    /// written as exact literals (no sin/cos) for readability and to keep
    /// the default file stable forever.
    fn default() -> Self {
        WorldRecipe {
            schema_version: CURRENT_SCHEMA_VERSION,
            extent: Extent {
                width_m: 2048.0,
                depth_m: 2048.0,
            },
            chunk_size_m: 256.0,
            sea_level_m: 0.0,
            mode: WorldMode::LandFirst,
            terrain: TerrainSection {
                feel: TerrainFeel::Hilly,
                overrides: TerrainOverrides::default(),
            },
            profile: TerrainProfile::default(),
            export: ExportSettings::default(),
            shapes: vec![Shape {
                id: "coast-1".to_string(),
                kind: ShapeKind::Land,
                points: vec![
                    [700.0, 0.0],
                    [610.0, 350.0],
                    [350.0, 610.0],
                    [0.0, 700.0],
                    [-350.0, 610.0],
                    [-610.0, 350.0],
                    [-700.0, 0.0],
                    [-610.0, -350.0],
                    [-350.0, -610.0],
                    [0.0, -700.0],
                    [350.0, -610.0],
                    [610.0, -350.0],
                ],
            }],
        }
    }
}

// ---------------------------------------------------------------------
// Versioning and migration
// ---------------------------------------------------------------------

#[derive(Debug)]
pub enum SchemaError {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// `schema_version` missing or 0.
    MissingVersion {
        path: PathBuf,
    },
    /// File is newer than this build understands.
    UnsupportedVersion {
        found: u32,
        max: u32,
    },
    /// `create_project` refuses to overwrite.
    ProjectExists(PathBuf),
    /// A recipe path escapes the project directory.
    InvalidRecipePath(String),
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::Io(e) => write!(f, "io error: {e}"),
            SchemaError::Json(e) => write!(f, "json error: {e}"),
            SchemaError::MissingVersion { path } => write!(
                f,
                "{}: schema_version is missing or 0; a positive version is required",
                path.display()
            ),
            SchemaError::UnsupportedVersion { found, max } => write!(
                f,
                "schema_version {found} is newer than supported version {max}; \
                 upgrade My Builder to open this file"
            ),
            SchemaError::ProjectExists(p) => {
                write!(f, "refusing to overwrite existing {}", p.display())
            }
            SchemaError::InvalidRecipePath(p) => write!(
                f,
                "invalid recipe path {p:?}: recipe paths must be relative \
                 and stay inside the project directory"
            ),
        }
    }
}

impl std::error::Error for SchemaError {}

impl From<std::io::Error> for SchemaError {
    fn from(e: std::io::Error) -> Self {
        SchemaError::Io(e)
    }
}

impl From<serde_json::Error> for SchemaError {
    fn from(e: serde_json::Error) -> Self {
        SchemaError::Json(e)
    }
}

fn read_versioned(path: &Path) -> Result<serde_json::Value, SchemaError> {
    let text = std::fs::read_to_string(path)?;
    let mut value: serde_json::Value = serde_json::from_str(&text)?;
    let version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .filter(|v| *v > 0);
    let version = match version {
        Some(v) => v as u32,
        None => {
            return Err(SchemaError::MissingVersion {
                path: path.to_path_buf(),
            });
        }
    };
    if version > CURRENT_SCHEMA_VERSION {
        return Err(SchemaError::UnsupportedVersion {
            found: version,
            max: CURRENT_SCHEMA_VERSION,
        });
    }
    let value = migrate(std::mem::take(&mut value), version)?;
    Ok(value)
}

/// Read a JSON file through the version check + migration chain,
/// returning the migrated `serde_json::Value`.
pub fn load_json_file(path: &Path) -> Result<serde_json::Value, SchemaError> {
    read_versioned(path)
}

/// Migration chain from an older `schema_version` to the current one.
/// Currently the chain has no steps (v1 is the only version); future
/// steps slot in as `1 => value = migrate_1_to_2(value)?`.
pub fn migrate(value: serde_json::Value, from: u32) -> Result<serde_json::Value, SchemaError> {
    debug_assert!(from >= 1 && from <= CURRENT_SCHEMA_VERSION);
    Ok(value)
}

// ---------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        })
    }
}

/// A validation finding. Serialized so UIs and engines can mirror it.
/// (`code` is a borrowed constant, so this type is serialize-only.)
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Stable machine-readable code, e.g. "world.extent.non_positive".
    pub code: &'static str,
    /// JSON pointer into the offending file, e.g. "/shapes/2/points".
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

impl Diagnostic {
    fn error(code: &'static str, path: String, message: String) -> Self {
        Diagnostic {
            severity: Severity::Error,
            code,
            path,
            message,
            suggestion: None,
        }
    }

    fn warning(code: &'static str, path: String, message: String) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            code,
            path,
            message,
            suggestion: None,
        }
    }

    fn with_suggestion(mut self, suggestion: &str) -> Self {
        self.suggestion = Some(suggestion.to_string());
        self
    }
}

/// Proposed maximum world size in meters per axis. Proposed: the max
/// world size is still an open decision.
const MAX_EXTENT_M: f64 = 16384.0;

/// Sane ranges for terrain overrides; violations are warnings, not
/// errors, because out-of-range values still produce valid output.
fn terrain_override_diags(o: &TerrainOverrides, out: &mut Vec<Diagnostic>) {
    let check = |path: &str, value: f64, lo: f64, hi: f64, out: &mut Vec<Diagnostic>| {
        if !(lo..=hi).contains(&value) {
            out.push(
                Diagnostic::warning(
                    "world.terrain.override_out_of_range",
                    format!("/terrain/overrides/{path}"),
                    format!(
                        "terrain override {path} = {value} is outside the sane range {lo}..={hi}"
                    ),
                )
                .with_suggestion("clamp the value or remove the override"),
            );
        }
    };
    if let Some(v) = o.octaves {
        check("octaves", v as f64, 1.0, 10.0, out);
    }
    if let Some(v) = o.gain {
        check("gain", v, 0.0, 1.0, out);
    }
    if let Some(v) = o.lacunarity {
        check("lacunarity", v, 1.5, 3.0, out);
    }
    if let Some(v) = o.relief_m {
        check("relief_m", v, 0.0, 5000.0, out);
    }
    if let Some(v) = o.ridged_mix {
        check("ridged_mix", v, 0.0, 1.0, out);
    }
}

/// True when a recipe path is empty, absolute, or contains a `..`
/// component — all of which would let a project file reference files
/// outside its own directory.
fn recipe_path_is_invalid(p: &str) -> bool {
    p.is_empty()
        || p.starts_with(['/', '\\'])
        || p.contains(':')
        || Path::new(p).is_absolute()
        || p.split(['/', '\\']).any(|seg| seg == "..")
}

/// Validate a project + world recipe pair. Returns diagnostics in a
/// stable order: file-level checks first, then per-shape checks.
pub fn validate_project(project: &ProjectFile, world: &WorldRecipe) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    if project.world_scale.meters_per_unit != 1.0 {
        out.push(
            Diagnostic::error(
                "project.world_scale.meters_per_unit.unsupported",
                "/world_scale/meters_per_unit".to_string(),
                format!(
                    "meters_per_unit {} is not supported; only 1.0 is valid in schema v1",
                    project.world_scale.meters_per_unit
                ),
            )
            .with_suggestion("set meters_per_unit to 1.0"),
        );
    }
    let cell_ok =
        project.world_scale.cell_size_m.is_finite() && project.world_scale.cell_size_m > 0.0;
    if !cell_ok {
        out.push(Diagnostic::error(
            "project.world_scale.cell_size.non_positive",
            "/world_scale/cell_size_m".to_string(),
            format!(
                "cell_size_m {} must be positive",
                project.world_scale.cell_size_m
            ),
        ));
    }
    if recipe_path_is_invalid(&project.recipes.world) {
        out.push(
            Diagnostic::error(
                "project.recipes.world.invalid_path",
                "/recipes/world".to_string(),
                format!(
                    "recipe path {:?} is empty, absolute, or contains a '..' component",
                    project.recipes.world
                ),
            )
            .with_suggestion("use a relative path inside the project, e.g. recipes/world.json"),
        );
    }
    if project.name.is_empty() {
        out.push(Diagnostic::warning(
            "project.name.empty",
            "/name".to_string(),
            "project name is empty".to_string(),
        ));
    }

    validate_world(world, project.world_scale.cell_size_m, &mut out);
    out
}

/// Validate just the world recipe (cell size comes from the project).
pub fn validate_world(world: &WorldRecipe, cell_size_m: f64, out: &mut Vec<Diagnostic>) {
    let Extent { width_m, depth_m } = world.extent;
    let finite_extent = width_m.is_finite() && depth_m.is_finite();

    let extent_positive = width_m > 0.0 && depth_m > 0.0;
    if !finite_extent || !extent_positive {
        out.push(Diagnostic::error(
            "world.extent.non_positive",
            "/extent".to_string(),
            format!("extent {width_m} x {depth_m} must be finite and positive"),
        ));
    } else {
        // Proposed: the max world size is still an open decision.
        if width_m > MAX_EXTENT_M || depth_m > MAX_EXTENT_M {
            out.push(
                Diagnostic::warning(
                    "world.extent.very_large",
                    "/extent".to_string(),
                    format!(
                        "extent {width_m} x {depth_m} exceeds the proposed {MAX_EXTENT_M} m maximum"
                    ),
                )
                .with_suggestion("consider a smaller world or check the open max-size decision"),
            );
        }
    }

    let chunk = world.chunk_size_m;
    let pow2 = chunk.is_finite()
        && chunk > 0.0
        && (chunk as u64).is_power_of_two()
        && chunk.fract() == 0.0;
    if !pow2 || (finite_extent && (chunk > width_m || chunk > depth_m)) {
        out.push(
            Diagnostic::error(
                "world.chunk_size.invalid",
                "/chunk_size_m".to_string(),
                format!("chunk_size_m {chunk} must be a power of two no larger than the extent"),
            )
            .with_suggestion("use e.g. 64, 128, 256 or 512"),
        );
    } else {
        if finite_extent && (width_m % chunk != 0.0 || depth_m % chunk != 0.0) {
            out.push(
                Diagnostic::error(
                    "world.extent.not_multiple_of_chunk",
                    "/extent".to_string(),
                    format!(
                        "extent {width_m} x {depth_m} is not a multiple of chunk_size_m {chunk}"
                    ),
                )
                .with_suggestion("choose an extent that is an integer number of chunks"),
            );
        }
        if cell_size_m > 0.0 && (chunk / cell_size_m).fract() != 0.0 {
            out.push(
                Diagnostic::error(
                    "world.chunk_size.not_multiple_of_cell",
                    "/chunk_size_m".to_string(),
                    format!("chunk_size_m {chunk} is not an integer multiple of cell_size_m {cell_size_m}"),
                )
                .with_suggestion("make chunk_size_m / cell_size_m an integer"),
            );
        }
    }

    terrain_override_diags(&world.terrain.overrides, out);

    let prof = &world.profile;
    for (field, v) in [
        ("shelf_width_m", prof.shelf_width_m),
        ("inland_width_m", prof.inland_width_m),
        ("mountain_falloff_m", prof.mountain_falloff_m),
        ("feature_size_m", prof.feature_size_m),
    ] {
        if !v.is_finite() || v <= 0.0 {
            out.push(Diagnostic::error(
                "world.profile.non_positive",
                format!("/profile/{field}"),
                format!("profile {field} = {v} must be finite and positive"),
            ));
        }
    }
    for (field, v) in [
        ("shelf_depth_m", prof.shelf_depth_m),
        ("coast_height_m", prof.coast_height_m),
        ("inland_height_m", prof.inland_height_m),
        ("mountain_height_m", prof.mountain_height_m),
    ] {
        if !v.is_finite() {
            out.push(Diagnostic::error(
                "world.profile.non_finite",
                format!("/profile/{field}"),
                format!("profile {field} = {v} must be finite"),
            ));
        }
    }

    let step = world.export.mesh_step_cells;
    if !step.is_power_of_two() {
        out.push(Diagnostic::error(
            "world.export.mesh_step.invalid",
            "/export/mesh_step_cells".to_string(),
            format!("mesh_step_cells {step} must be a power of two"),
        ));
    } else if cell_size_m > 0.0 && chunk.is_finite() && chunk > 0.0 {
        let cells_per_chunk = chunk / cell_size_m;
        if cells_per_chunk.fract() == 0.0 && cells_per_chunk % step as f64 != 0.0 {
            out.push(
                Diagnostic::error(
                    "world.export.mesh_step.invalid",
                    "/export/mesh_step_cells".to_string(),
                    format!("mesh_step_cells {step} does not divide chunk cells {cells_per_chunk}"),
                )
                .with_suggestion("use a power of two dividing chunk_size_m / cell_size_m"),
            );
        }
    }

    let mut seen_ids = BTreeSet::new();
    for (i, shape) in world.shapes.iter().enumerate() {
        let sp = format!("/shapes/{i}");
        if shape.id.is_empty() {
            out.push(Diagnostic::error(
                "world.shape.empty_id",
                format!("{sp}/id"),
                "shape id is empty".to_string(),
            ));
        } else if !seen_ids.insert(shape.id.clone()) {
            out.push(
                Diagnostic::error(
                    "world.shape.duplicate_id",
                    format!("{sp}/id"),
                    format!("duplicate shape id {:?}", shape.id),
                )
                .with_suggestion("shape ids must be unique within a world"),
            );
        }
        if shape.points.len() < 3 {
            out.push(Diagnostic::error(
                "world.shape.too_few_points",
                format!("{sp}/points"),
                format!(
                    "shape {:?} has {} points; at least 3 are required",
                    shape.id,
                    shape.points.len()
                ),
            ));
        }
        let half_w = width_m / 2.0;
        let half_d = depth_m / 2.0;
        let mut outside = false;
        for (j, [x, z]) in shape.points.iter().enumerate() {
            if !x.is_finite() || !z.is_finite() {
                out.push(Diagnostic::error(
                    "world.shape.non_finite",
                    format!("{sp}/points/{j}"),
                    format!("shape {:?} point {j} ({x}, {z}) is not finite", shape.id),
                ));
            } else if finite_extent && (x.abs() > half_w || z.abs() > half_d) && !outside {
                outside = true;
                out.push(Diagnostic::warning(
                    "world.shape.outside_extent",
                    format!("{sp}/points"),
                    format!("shape {:?} has points outside the world extent", shape.id),
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------
// Project folder I/O
// ---------------------------------------------------------------------

pub const PROJECT_FILE: &str = "project.json";
pub const SUBDIRS: [&str; 6] = [
    "layers",
    "overrides",
    "cache",
    "styles",
    "previews",
    "exports",
];

fn write_pretty(path: &Path, value: &impl Serialize) -> Result<(), SchemaError> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    std::fs::write(path, text)?;
    Ok(())
}

/// Create a project folder with project.json, the world recipe and the
/// standard subfolders (each with a .gitkeep). Refuses to overwrite an
/// existing project.json.
pub fn create_project(
    dir: &Path,
    project: &ProjectFile,
    world: &WorldRecipe,
) -> Result<(), SchemaError> {
    let project_path = dir.join(PROJECT_FILE);
    if project_path.exists() {
        return Err(SchemaError::ProjectExists(project_path));
    }
    std::fs::create_dir_all(dir.join("recipes"))?;
    for sub in SUBDIRS {
        let d = dir.join(sub);
        std::fs::create_dir_all(&d)?;
        std::fs::write(d.join(".gitkeep"), "")?;
    }
    write_pretty(&project_path, project)?;
    let world_path = dir.join(&project.recipes.world);
    if let Some(parent) = world_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_pretty(&world_path, world)?;
    Ok(())
}

/// Load project.json + world recipe and validate them.
pub fn load_project(
    dir: &Path,
) -> Result<(ProjectFile, WorldRecipe, Vec<Diagnostic>), SchemaError> {
    let project: ProjectFile = serde_json::from_value(read_versioned(&dir.join(PROJECT_FILE))?)?;
    // Check the recipe path BEFORE reading it: a project file must never
    // point outside its own directory.
    if recipe_path_is_invalid(&project.recipes.world) {
        return Err(SchemaError::InvalidRecipePath(project.recipes.world));
    }
    let world: WorldRecipe =
        serde_json::from_value(read_versioned(&dir.join(&project.recipes.world))?)?;
    let diags = validate_project(&project, &world);
    Ok((project, world, diags))
}
