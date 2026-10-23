//! `build_project`: the S10 rebuild engine shared by CLI, tests and the
//! future app. Assembles the stage DAG (global + layers + per-chunk
//! sliced + manifest), runs it against a persistent DiskCache, writes
//! outputs atomically, cleans stale files, and reports.

use std::path::{Path, PathBuf};

use mb_pipeline::{Artifact, CacheStore, DiskCache, Pipeline, RunReport};
use mb_schema::WorldRecipe;

use crate::outputs::{FileBundle, MANIFEST_PATH};
use crate::stages::*;

pub struct BuildOptions {
    /// `mybuilder build --clean --yes`: wipe cache + listed outputs first.
    pub clean: bool,
    /// Cache location override; None = OS cache dir per project.
    /// Tests must ALWAYS set this — never write to the real user cache.
    pub cache_dir: Option<PathBuf>,
}

pub struct WriteReport {
    pub written: usize,
    pub unchanged: usize,
    pub deleted: usize,
    /// Manifest/output paths that failed the confinement check.
    pub skipped_unsafe: usize,
    /// Paths actually written, relative to the project dir.
    pub written_paths: Vec<String>,
    pub deleted_paths: Vec<String>,
}

pub struct BuildReport {
    /// True when the run was cancelled (no outputs were written).
    pub cancelled: bool,
    /// Info-level diagnostics (e.g. legacy cache dir).
    pub diagnostics: Vec<mb_schema::Diagnostic>,
    pub run: RunReport,
    pub write: WriteReport,
    /// Grid for callers that need it (tests).
    pub grid: crate::Grid,
    /// Final output files (path, bytes) that the build produced.
    pub outputs: FileBundle,
}

#[derive(Debug)]
pub enum BuildError {
    /// `.mybuilder.lock` is held by another process.
    Locked,
    Schema(mb_schema::SchemaError),
    Validation(Vec<mb_schema::Diagnostic>),
    Pipeline(mb_pipeline::PipelineError),
    Cache(mb_pipeline::CacheError),
    Io(std::io::Error),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Schema(e) => write!(f, "{e}"),
            BuildError::Validation(ds) => {
                for d in ds {
                    writeln!(f, "{} {} {}: {}", d.severity, d.code, d.path, d.message)?;
                }
                Ok(())
            }
            BuildError::Pipeline(e) => write!(f, "{e}"),
            BuildError::Cache(e) => write!(f, "{e}"),
            BuildError::Io(e) => write!(f, "{e}"),
            BuildError::Locked => write!(f, "another build is running on this project"),
        }
    }
}
impl std::error::Error for BuildError {}
impl From<mb_schema::SchemaError> for BuildError {
    fn from(e: mb_schema::SchemaError) -> Self {
        BuildError::Schema(e)
    }
}
impl From<mb_pipeline::PipelineError> for BuildError {
    fn from(e: mb_pipeline::PipelineError) -> Self {
        BuildError::Pipeline(e)
    }
}
impl From<mb_pipeline::CacheError> for BuildError {
    fn from(e: mb_pipeline::CacheError) -> Self {
        BuildError::Cache(e)
    }
}
impl From<std::io::Error> for BuildError {
    fn from(e: std::io::Error) -> Self {
        BuildError::Io(e)
    }
}

/// Files referenced by the previous manifest + layers.json on disk
/// (paths relative to the project dir, as stored).
fn previous_outputs(dir: &Path) -> Vec<(String, bool)> {
    let mut out = vec![
        ("exports/terrain/manifest.json".to_string(), true),
        ("layers/layers.json".to_string(), true),
    ];
    let mut push = |full: String, raw: &str| {
        out.push((full, mb_schema::confined_rel_path_is_safe(raw)));
    };
    if let Ok(t) = std::fs::read(dir.join(MANIFEST_PATH))
        && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&t)
    {
        if let Some(ch) = v["chunks"].as_array() {
            for c in ch {
                if let Some(f) = c["file"].as_str() {
                    push(format!("exports/terrain/{f}"), f);
                }
            }
        }
        if let Some(regs) = v["regions"].as_array() {
            for r in regs {
                if let Some(ch) = r["chunks"].as_array() {
                    for c in ch {
                        if let Some(f) = c["file"].as_str() {
                            push(format!("exports/terrain/{f}"), f);
                        }
                    }
                }
            }
        }
    }
    if let Ok(t) = std::fs::read(dir.join("layers/layers.json"))
        && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&t)
        && let Some(files) = v["files"].as_object()
    {
        for (_, f) in files {
            if let Some(f) = f.as_str() {
                push(format!("layers/{f}"), f);
            }
        }
    }
    out
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Compare-and-write all files; manifest goes LAST. Files listed in the
/// previous manifest/layers.json but not in the new set are deleted
/// (only files we listed — never anything else under exports/).
/// Filesystem barrier before deleting `dir/rel`: no path component
/// from the project root down (final file included) may be a symlink,
/// and the canonicalized parent must stay inside the canonicalized
/// project dir. NOTE: this applies to DELETES only — writes still
/// follow symlinks on purpose (users may symlink exports/ into a Godot
/// project).
fn safe_to_delete(dir: &Path, rel: &str) -> bool {
    let canon_root = match dir.canonicalize() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mut cur = dir.to_path_buf();
    for comp in std::path::Path::new(rel).components() {
        cur.push(comp.as_os_str());
        if let Ok(md) = std::fs::symlink_metadata(&cur)
            && md.file_type().is_symlink()
        {
            return false;
        }
    }
    match cur.parent().and_then(|p| p.canonicalize().ok()) {
        Some(p) => p.starts_with(&canon_root),
        None => false,
    }
}

fn write_outputs(dir: &Path, outputs: &FileBundle) -> std::io::Result<WriteReport> {
    let prev = previous_outputs(dir);
    let mut rep = WriteReport {
        written: 0,
        unchanged: 0,
        deleted: 0,
        skipped_unsafe: 0,
        written_paths: Vec::new(),
        deleted_paths: Vec::new(),
    };
    let mut ordered: Vec<&(String, Vec<u8>)> = outputs.files.iter().collect();
    ordered.sort_by_key(|(p, _)| (*p != MANIFEST_PATH) as u8); // manifest last
    /// A path is confined if it is a safe relative path AND lives under
    /// one of our output roots (by path components).
    fn confined(rel: &str) -> bool {
        mb_schema::confined_rel_path_is_safe(rel)
            && (rel.starts_with("exports/terrain/")
                || rel.starts_with("layers/")
                || rel.starts_with("previews/"))
    }

    let mut new_set = std::collections::BTreeSet::new();
    for (rel, bytes) in ordered {
        if !confined(rel) {
            rep.skipped_unsafe += 1;
            continue;
        }
        new_set.insert(rel.clone());
        let path = dir.join(rel);
        let same = std::fs::read(&path)
            .map(|d| mb_seed::hash_bytes(&d) == mb_seed::hash_bytes(bytes))
            .unwrap_or(false);
        if same {
            rep.unchanged += 1;
        } else {
            write_atomic(&path, bytes)?;
            rep.written += 1;
            rep.written_paths.push(rel.clone());
        }
    }
    for (rel, entry_safe) in prev {
        if !new_set.contains(&rel) {
            if !entry_safe || !confined(&rel) || !safe_to_delete(dir, &rel) {
                rep.skipped_unsafe += 1;
                continue;
            }
            let p = dir.join(&rel);
            if p.exists() && std::fs::remove_file(&p).is_ok() {
                rep.deleted += 1;
                rep.deleted_paths.push(rel);
            }
        }
    }
    Ok(rep)
}

/// Collect every output file from the artifacts: layer bundle files,
/// each chunk/region-chunk artifact's GLB (header stripped), and the
/// manifest bytes.
fn collect_outputs(
    run_artifacts: &dyn Fn(&str) -> Option<Artifact>,
    grid: &crate::Grid,
    world: &WorldRecipe,
) -> FileBundle {
    let mut files = Vec::new();
    let layers = FileBundle::decode(&run_artifacts(LayersStage::ID).unwrap().bytes);
    files.extend(layers.files);
    let cells_per_chunk = (world.chunk_size_m / grid.cell) as usize;
    let ncx = (grid.nx - 1) / cells_per_chunk;
    let ncz = (grid.nz - 1) / cells_per_chunk;
    for cz in 0..ncz {
        for cx in 0..ncx {
            let a = run_artifacts(&chunk_stage_id(cx, cz)).unwrap();
            files.push((
                format!("exports/terrain/chunk_{cx}_{cz}.glb"),
                a.bytes[CHUNK_HEADER..].to_vec(),
            ));
        }
    }
    for r in &world.detail_regions {
        for cz in r.chunk_min[1]..=r.chunk_max[1] {
            for cx in r.chunk_min[0]..=r.chunk_max[0] {
                let a =
                    run_artifacts(&region_chunk_stage_id(&r.id, cx as usize, cz as usize)).unwrap();
                files.push((
                    format!("exports/terrain/regions/{}/chunk_{cx}_{cz}.glb", r.id),
                    a.bytes[CHUNK_HEADER..].to_vec(),
                ));
            }
        }
    }
    let manifest = run_artifacts(ManifestStage::ID).unwrap();
    files.push((MANIFEST_PATH.to_string(), manifest.bytes.clone()));
    FileBundle { files }
}

/// Validate, (optionally clean), run the pipeline against the project's
/// persistent cache at `<dir>/cache`, write outputs, return the report.
/// Exclusive project-level build lock on `<project>/.mybuilder.lock`
/// (OS advisory: flock/LockFileEx). Held for the whole build, so it
/// covers outputs AND the cache. The pid is written for diagnostics
/// only; the OS releases the lock on process death.
pub struct ProjectLock(#[allow(dead_code)] std::fs::File);

impl ProjectLock {
    pub fn acquire(dir: &Path) -> Result<ProjectLock, BuildError> {
        let path = dir.join(".mybuilder.lock");
        let mut f = std::fs::File::create(&path)?;
        use std::io::Write as _;
        if f.try_lock().is_err() {
            return Err(BuildError::Locked);
        }
        let _ = write!(f, "{}", std::process::id());
        Ok(ProjectLock(f))
    }
}

/// Resolve the OS cache dir for a project:
/// `<os cache>/MyBuilder/projects/<sanitized name>-<16 hex of
/// blake3(canonical abs project path)>`. Env vars only, no dirs crate.
pub fn cache_root() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").expect("HOME not set");
        PathBuf::from(home).join("Library/Caches")
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").expect("HOME not set")).join(".cache")
            })
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .expect("LOCALAPPDATA not set")
    }
}

/// The resolved cache dir for a project (default OS location).
/// Also writes the standard CACHEDIR.TAG marker once.
pub fn project_cache_dir(dir: &Path) -> std::io::Result<PathBuf> {
    let canon = dir.canonicalize()?;
    let name = canon
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".to_string());
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let tag = mb_seed::hash_bytes(canon.to_string_lossy().as_bytes());
    let id = format!(
        "{}-{}",
        safe,
        tag.iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let root = cache_root().join("MyBuilder");
    std::fs::create_dir_all(&root)?;
    let tagf = root.join("CACHEDIR.TAG");
    if !tagf.exists() {
        write_atomic(
            &tagf,
            b"Signature: 8a477f597d28d172789f06886806bc55
# For information about cache directory tags see https://bford.info/cachedir/
",
        )?;
    }
    Ok(root.join("projects").join(id))
}

pub fn build_project(
    dir: &Path,
    opts: &BuildOptions,
    cancel: &mb_pipeline::CancelToken,
) -> Result<BuildReport, BuildError> {
    let (project, world, diags) = mb_schema::load_project(dir)?;
    if diags
        .iter()
        .any(|d| d.severity == mb_schema::Severity::Error)
    {
        return Err(BuildError::Validation(diags));
    }
    // Project-level lock covers outputs AND the cache for the whole
    // build — take it BEFORE any deletion.
    let _lock = ProjectLock::acquire(dir)?;
    let cache_dir = opts
        .cache_dir
        .clone()
        .map(|c| if c.is_absolute() { c } else { dir.join(c) })
        .unwrap_or_else(|| project_cache_dir(dir).unwrap_or_else(|_| dir.join("cache")));
    let mut info: Vec<mb_schema::Diagnostic> = Vec::new();
    if dir.join("cache/index.json").exists() {
        info.push(mb_schema::Diagnostic {
            severity: mb_schema::Severity::Info,
            code: "project.cache.legacy_dir",
            path: "cache/".to_string(),
            message: "project-local cache/ is no longer used; safe to delete".to_string(),
            suggestion: None,
        });
    }
    let mut cache = DiskCache::open(&cache_dir)?;
    if opts.clean {
        cache.clear();
        for (rel, entry_safe) in previous_outputs(dir) {
            if entry_safe && mb_schema::confined_rel_path_is_safe(&rel) && safe_to_delete(dir, &rel)
            {
                let _ = std::fs::remove_file(dir.join(rel));
            }
        }
    }

    let cell = project.world_scale.cell_size_m;
    let grid = crate::Grid::new(world.extent.width_m, world.extent.depth_m, cell);
    let mut pipe = Pipeline::new(project.seed, project.generator_version);
    pipe.add(Box::new(MasksStage {
        code_version: 1,
        grid,
        mode: world.mode,
        shapes: world.shapes.clone(),
        cell_size_m: cell,
        extent: world.extent,
        generator_version: project.generator_version,
    }));
    pipe.add(Box::new(DistanceStage::new(grid, cell, 1)));
    pipe.add(Box::new(HeightStage::new(
        grid,
        project.clone(),
        world.clone(),
        1,
    )));
    pipe.add(Box::new(LayersStage::new(
        grid,
        project.clone(),
        world.clone(),
        1,
    )));
    let cells_per_chunk = (world.chunk_size_m / cell) as usize;
    let ncx = (grid.nx - 1) / cells_per_chunk;
    let ncz = (grid.nz - 1) / cells_per_chunk;
    for cz in 0..ncz {
        for cx in 0..ncx {
            pipe.add_sliced(Box::new(ChunkStage::new(
                grid,
                project.clone(),
                world.clone(),
                cx,
                cz,
                1,
            )));
        }
    }
    for r in &world.detail_regions {
        for cz in r.chunk_min[1]..=r.chunk_max[1] {
            for cx in r.chunk_min[0]..=r.chunk_max[0] {
                pipe.add_sliced(Box::new(RegionChunkStage::new(
                    grid,
                    project.clone(),
                    world.clone(),
                    r.clone(),
                    cx as usize,
                    cz as usize,
                    1,
                )));
            }
        }
    }
    pipe.add(Box::new(ManifestStage::new(
        grid,
        project.clone(),
        world.clone(),
        1,
    )));
    let report = pipe.run(&mut cache, cancel)?;
    if report.cancelled {
        // No writes on cancel; the cache was already finished inside run.
        return Ok(BuildReport {
            diagnostics: info,
            run: report,
            write: WriteReport {
                written: 0,
                unchanged: 0,
                deleted: 0,
                skipped_unsafe: 0,
                written_paths: Vec::new(),
                deleted_paths: Vec::new(),
            },
            outputs: FileBundle { files: Vec::new() },
            grid,
            cancelled: true,
        });
    }
    let ids = pipe.stage_ids();
    cache.finish(&ids);
    let mut owned: Vec<(String, Artifact)> = Vec::new();
    for id in &ids {
        if let Some(a) = cache.get_any(id) {
            owned.push((id.clone(), a));
        }
    }
    let lookup = |id: &str| owned.iter().find(|(k, _)| k == id).map(|(_, a)| a.clone());
    let outputs = collect_outputs(&lookup, &grid, &world);
    let write = write_outputs(dir, &outputs)?;
    Ok(BuildReport {
        diagnostics: info,
        run: report,
        cancelled: false,
        write,
        grid,
        outputs,
    })
}
