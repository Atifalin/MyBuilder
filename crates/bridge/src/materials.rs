//! User texture overrides for house material slots (Phase 3 step 4a).
//! The texture is copied into `<project>/assets/textures/<slot>.<ext>` so
//! the project stays self-contained; the houses recipe records the
//! project-relative path and real-world tile size.

use std::path::Path;

use mb_schema::{HouseRecipe, MaterialOverride, MaterialSlot, Severity};

use crate::session::BridgeError;

fn invalid(msg: String) -> BridgeError {
    BridgeError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, msg))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), BridgeError> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension("mbtmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn load(dir: &Path) -> Result<(std::path::PathBuf, HouseRecipe), BridgeError> {
    let (project, _, _) = mb_schema::load_project(dir).map_err(BridgeError::Schema)?;
    let rel = project
        .recipes
        .houses
        .clone()
        .ok_or_else(|| invalid("project has no houses recipe; add a house first".into()))?;
    let recipe = mb_schema::load_houses(dir, &project)
        .map_err(BridgeError::Schema)?
        .ok_or_else(|| invalid("project has no houses recipe; add a house first".into()))?;
    Ok((dir.join(rel), recipe))
}

fn check(recipe: &HouseRecipe) -> Result<(), BridgeError> {
    let errors: Vec<String> = mb_schema::validate_houses(recipe)
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| format!("{} {}: {}", d.code, d.path, d.message))
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(invalid(errors.join("; ")))
    }
}

fn save(path: &Path, recipe: &HouseRecipe) -> Result<(), BridgeError> {
    check(recipe)?;
    atomic_write(
        path,
        (serde_json::to_string_pretty(recipe).unwrap() + "\n").as_bytes(),
    )
}

/// Copy `src` (PNG or JPEG, by magic bytes) into the project and point
/// `slot` at it. Validates everything first: on error nothing is written.
pub fn set_material_override(
    dir: &Path,
    slot: MaterialSlot,
    src: &Path,
    tile_m: f64,
) -> Result<(), BridgeError> {
    let bytes = std::fs::read(src)?;
    let kind = mb_schema::texture_kind(&bytes)
        .ok_or_else(|| invalid(format!("{} is not a PNG or JPEG image", src.display())))?;
    let (path, mut recipe) = load(dir)?;
    let rel = format!("assets/textures/{}.{kind}", slot.name());
    recipe.materials.retain(|o| o.slot != slot);
    recipe.materials.push(MaterialOverride {
        slot,
        texture: rel.clone(),
        tile_m,
    });
    recipe.materials.sort_by_key(|o| o.slot);
    check(&recipe)?;
    atomic_write(&dir.join(&rel), &bytes)?;
    save(&path, &recipe)
}

/// Drop the override for `slot` (the copied file is left in place).
/// Returns false when there was none.
pub fn clear_material_override(dir: &Path, slot: MaterialSlot) -> Result<bool, BridgeError> {
    let (path, mut recipe) = load(dir)?;
    let before = recipe.materials.len();
    recipe.materials.retain(|o| o.slot != slot);
    if recipe.materials.len() == before {
        return Ok(false);
    }
    save(&path, &recipe)?;
    Ok(true)
}
