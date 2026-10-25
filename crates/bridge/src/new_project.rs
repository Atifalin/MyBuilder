//! `new_project`: the wizard path for creating a project — a feel preset,
//! a start mode, and a world size, producing the same project files as
//! `mybuilder new` plus the water-first island blob.

use std::path::Path;

use mb_schema::{Extent, ProjectFile, TerrainSection, WorldMode, WorldRecipe};

use crate::session::BridgeError;

/// How the world starts out: a sea with an island to reshape, or all land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorldStart {
    WaterFirst,
    LandFirst,
}

pub struct NewProjectOptions {
    pub name: String,
    /// None -> fresh seed (same clock-based source as clear_all).
    pub seed_hex: Option<String>,
    /// World side in meters: one of 1024, 2048, 4096.
    pub size_m: f64,
    pub feel: mb_schema::TerrainFeel,
    pub start: WorldStart,
}

fn gen_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let h = mb_seed::hash_bytes(&nanos.to_le_bytes());
    u64::from_le_bytes(h[..8].try_into().unwrap())
}

/// Create a project directory; refuses an existing project or an invalid
/// option. Validation errors -> `Rejected` with nothing on disk.
pub fn new_project(dir: &Path, opts: &NewProjectOptions) -> Result<ProjectFile, BridgeError> {
    let seed = match &opts.seed_hex {
        Some(s) => u64::from_str_radix(s.trim(), 16).map_err(|_| BridgeError::Rejected)?,
        None => gen_seed(),
    };
    if ![1024.0, 2048.0, 4096.0].contains(&opts.size_m) {
        return Err(BridgeError::Rejected);
    }
    let mut world = WorldRecipe {
        extent: Extent {
            width_m: opts.size_m,
            depth_m: opts.size_m,
        },
        terrain: TerrainSection {
            feel: opts.feel,
            overrides: mb_schema::TerrainOverrides::default(),
        },
        ..WorldRecipe::default()
    };
    match opts.start {
        WorldStart::WaterFirst => {
            world.mode = WorldMode::WaterFirst;
            // The default recipe's island blob, scaled by size_m/2048.
            let s = opts.size_m / 2048.0;
            for shape in &mut world.shapes {
                for p in &mut shape.points {
                    *p = [p[0] * s, p[1] * s];
                }
            }
        }
        WorldStart::LandFirst => {
            world.mode = WorldMode::LandFirst;
            world.shapes = Vec::new();
        }
    }
    let project = ProjectFile {
        seed,
        name: opts.name.clone(),
        generator_version: mb_schema::CURRENT_GENERATOR_VERSION,
        ..ProjectFile::default()
    };
    // Validation errors mean no files are written.
    let diags = mb_schema::validate_project(&project, &world);
    if diags
        .iter()
        .any(|d| d.severity == mb_schema::Severity::Error)
    {
        return Err(BridgeError::Rejected);
    }
    mb_schema::create_project(dir, &project, &world).map_err(BridgeError::Schema)?;
    Ok(project)
}
