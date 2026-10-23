//! clear_all: wipe a project's generated state under the project lock.
//! It does NOT build — the UI calls build(Full) afterwards. Note the
//! seed lives in project.json, not the recipe, so recipe undo history
//! is unaffected by a clear.

use std::path::{Path, PathBuf};

pub struct ClearRequest {
    /// New seed; None = freshly generated.
    pub new_seed: Option<u64>,
    /// Keep files inside overrides/ (always keeps .gitkeep).
    pub keep_overrides: bool,
    pub cache_dir: Option<PathBuf>,
}

pub struct ClearReport {
    pub seed: u64,
    pub deleted_files: usize,
}

fn gen_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let h = mb_seed::hash_bytes(&nanos.to_le_bytes());
    u64::from_le_bytes(h[..8].try_into().unwrap())
}

pub fn clear_all(
    dir: &Path,
    req: &ClearRequest,
) -> Result<ClearReport, mb_terrain::build::BuildError> {
    use mb_terrain::build::BuildError;
    // Whole operation under the project lock.
    let _lock = mb_terrain::build::ProjectLock::acquire(dir)?;
    let cache_dir = req
        .cache_dir
        .clone()
        .map(|c| if c.is_absolute() { c } else { dir.join(c) })
        .unwrap_or_else(|| {
            mb_terrain::build::project_cache_dir(dir).unwrap_or_else(|_| dir.join("cache"))
        });
    let mut deleted = 0usize;
    // Clear the resolved cache.
    {
        let mut cache = mb_pipeline::DiskCache::open(&cache_dir)?;
        cache.clear();
    }
    // Delete the outputs the previous manifest listed (confinement +
    // symlink checks happen inside write_outputs; here we mirror it by
    // only touching confined, non-symlinked listed files).
    for rel in mb_terrain::build::listed_outputs(dir) {
        if mb_terrain::build::may_delete(dir, &rel) && std::fs::remove_file(dir.join(&rel)).is_ok()
        {
            deleted += 1;
        }
    }
    // Overrides unless kept.
    if !req.keep_overrides {
        let odir = dir.join("overrides");
        if let Ok(rd) = std::fs::read_dir(&odir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.file_name().map(|n| n == ".gitkeep").unwrap_or(false) {
                    continue;
                }
                let rel = format!("overrides/{}", e.file_name().to_string_lossy());
                if mb_terrain::build::may_delete(dir, &rel) && std::fs::remove_file(&p).is_ok() {
                    deleted += 1;
                }
            }
        }
    }
    // New seed in project.json.
    let (mut project, _w, _d) = mb_schema::load_project(dir).map_err(BuildError::Schema)?;
    project.seed = req.new_seed.unwrap_or_else(gen_seed);
    let text = serde_json::to_string_pretty(&project).unwrap() + "\n";
    let path = dir.join(mb_schema::PROJECT_FILE);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &text)?;
    std::fs::rename(&tmp, &path)?;
    Ok(ClearReport {
        seed: project.seed,
        deleted_files: deleted,
    })
}
