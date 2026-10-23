//! Stage DAG runner with memo caching.
//!
//! Each stage is a pure function of (params, upstream, seed). Running
//! computes an input hash per stage —
//! `blake3(len·id, id, code_version, generator_version, len·params,
//! params, INPUT, stage_seed)` — and reuses the cached artifact when it
//! matches. INPUT is the dep output hashes for a normal [`Stage`], or
//! `blake3(slice)` for a [`SlicedStage`]: a sliced stage may only read
//! its slice, so the key provably covers everything `run` sees.
//!
//! Artifacts may be cached in memory ([`Cache`]) or on disk
//! ([`DiskCache`], content-addressed blobs + index, lock file, atomic
//! writes, hash-verified loads) through the [`CacheStore`] trait.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// A stage output. `hash` is always `blake3(bytes)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artifact {
    pub bytes: Vec<u8>,
    pub hash: [u8; 32],
}

impl Artifact {
    pub fn new(bytes: Vec<u8>) -> Self {
        let hash = mb_seed::hash_bytes(&bytes);
        Artifact { bytes, hash }
    }
}

/// A deterministic pipeline stage: run(inputs, seed) -> artifact.
pub trait Stage {
    /// Unique stage id, e.g. "terrain.heightfield".
    fn id(&self) -> &str;
    /// Upstream stage ids, in input order.
    fn deps(&self) -> &[String];
    /// Bump when the implementation changes in a way that affects
    /// output; participates in the input hash.
    fn code_version(&self) -> u32;
    /// Canonical parameter bytes (e.g. compact JSON of a typed struct).
    fn params(&self) -> Vec<u8>;
    /// Produce the artifact from upstream artifacts (deps() order).
    fn run(&self, inputs: &[&Artifact], seed: u64) -> Artifact;
}

/// A stage whose output depends only on a self-describing SLICE of its
/// upstream artifacts. `run` receives only the slice, so it is
/// structurally impossible to read outside the cached key's coverage.
pub trait SlicedStage {
    /// Unique stage id.
    fn id(&self) -> &str;
    /// Upstream stage ids, in input order.
    fn deps(&self) -> &[String];
    /// See [`Stage::code_version`].
    fn code_version(&self) -> u32;
    /// See [`Stage::params`].
    fn params(&self) -> Vec<u8>;
    /// Extract the minimal input slice from upstream artifacts. Always
    /// computed (it is how reuse is decided); `run` is not.
    fn slice(&self, inputs: &[&Artifact]) -> Vec<u8>;
    /// Produce the artifact from the slice only.
    fn run(&self, slice: &[u8], seed: u64) -> Artifact;
}

#[derive(Debug)]
pub enum PipelineError {
    DuplicateId(String),
    UnknownDep { stage: String, dep: String },
    Cycle,
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineError::DuplicateId(id) => write!(f, "duplicate stage id {id:?}"),
            PipelineError::UnknownDep { stage, dep } => {
                write!(f, "stage {stage:?} depends on unknown stage {dep:?}")
            }
            PipelineError::Cycle => write!(f, "stage graph contains a cycle"),
        }
    }
}

impl std::error::Error for PipelineError {}

/// Cache statistics accumulated during a run.
#[derive(Clone, Copy, Debug, Default)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    /// Blobs whose blake3 did not match the indexed artifact hash.
    pub corrupt: usize,
    /// Indexed blobs absent on disk.
    pub missing: usize,
    pub blobs_written: usize,
    pub blobs_deleted: usize,
}

/// Artifact storage shared by in-memory and disk caches. The pipeline
/// calls `get` with the computed input hash (key); a hit must return the
/// artifact only when the stored key equals it.
pub trait CacheStore {
    fn get(&mut self, id: &str, key: &[u8; 32]) -> Option<Artifact>;
    fn put(&mut self, id: &str, key: [u8; 32], artifact: &Artifact);
    fn stats(&self) -> CacheStats {
        CacheStats::default()
    }
    /// Persist metadata and drop entries for stages no longer present.
    /// In-memory caches ignore this.
    fn finish(&mut self, _live_ids: &[String]) {}
}

/// In-memory cache: stage id -> (input hash, artifact).
#[derive(Default)]
pub struct Cache {
    map: BTreeMap<String, ([u8; 32], Artifact)>,
    stats: CacheStats,
}

impl Cache {
    /// The cached artifact for a stage, if present.
    pub fn get(&self, id: &str) -> Option<&Artifact> {
        self.map.get(id).map(|(_, a)| a)
    }
}

impl CacheStore for Cache {
    fn get(&mut self, id: &str, key: &[u8; 32]) -> Option<Artifact> {
        match self.map.get(id) {
            Some((k, a)) if k == key => {
                self.stats.hits += 1;
                Some(a.clone())
            }
            _ => {
                self.stats.misses += 1;
                None
            }
        }
    }
    fn put(&mut self, id: &str, key: [u8; 32], artifact: &Artifact) {
        self.map.insert(id.to_string(), (key, artifact.clone()));
    }
    fn stats(&self) -> CacheStats {
        self.stats
    }
}

#[derive(Debug)]
pub enum CacheError {
    /// The project is already being built by another process.
    Locked,
    Io(std::io::Error),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheError::Locked => write!(f, "another build is running on this project"),
            CacheError::Io(e) => write!(f, "cache error: {e}"),
        }
    }
}

impl std::error::Error for CacheError {}
impl From<std::io::Error> for CacheError {
    fn from(e: std::io::Error) -> Self {
        CacheError::Io(e)
    }
}

/// Persistent, content-addressed cache in `<dir>`:
///   `<dir>/index.json` — sorted {stage id: {key, artifact}} hex map
///   `<dir>/blobs/<artifact hex>.bin` — artifact bytes
///   `<dir>/.lock` — exclusive advisory lock held while open; the OS
/// releases it when the process dies, so there are no stale locks.
/// Writes are atomic (temp file + rename); reads verify blake3.
pub struct DiskCache {
    dir: PathBuf,
    /// Held for the lock's lifetime; dropped (unlocked) with the cache.
    _lock: std::fs::File,
    /// stage id -> (input hash, artifact hash)
    index: BTreeMap<String, ([u8; 32], [u8; 32])>,
    stats: CacheStats,
}

fn hex32(h: &[u8; 32]) -> String {
    mb_seed::hex_bytes(h)
}

fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    if s.len() != 64 {
        return None;
    }
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

impl DiskCache {
    /// Open (creating if needed) the cache in `dir`, taking the exclusive
    /// lock. Returns [`CacheError::Locked`] if another process holds it.
    pub fn open(dir: &Path) -> Result<DiskCache, CacheError> {
        std::fs::create_dir_all(dir.join("blobs"))?;
        let lock_path = dir.join(".lock");
        let lock = std::fs::File::create(&lock_path)?;
        // Stable on our toolchain; WouldBlock means another build holds it.
        match lock.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Err(CacheError::Locked),
            Err(std::fs::TryLockError::Error(e)) => return Err(CacheError::Io(e)),
        }
        use std::io::Write;
        let mut f = &lock;
        let _ = writeln!(f, "{}", std::process::id());
        let index_path = dir.join("index.json");
        let mut index = BTreeMap::new();
        if let Ok(text) = std::fs::read_to_string(&index_path)
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(obj) = v.as_object()
        {
            for (id, e) in obj {
                let k = e["key"].as_str().and_then(parse_hex32);
                let a = e["artifact"].as_str().and_then(parse_hex32);
                if let (Some(k), Some(a)) = (k, a) {
                    index.insert(id.clone(), (k, a));
                }
            }
        }
        Ok(DiskCache {
            dir: dir.to_path_buf(),
            _lock: lock,
            index,
            stats: CacheStats::default(),
        })
    }

    fn blob_path(&self, hash: &[u8; 32]) -> PathBuf {
        self.dir.join("blobs").join(format!("{}.bin", hex32(hash)))
    }

    /// Write `bytes` to `path` atomically: temp file in the same
    /// directory, then rename.
    fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    }
}

impl DiskCache {
    /// Empty the cache in place while holding the lock: clears the index
    /// and deletes blobs/. The `.lock` file itself is never touched.
    pub fn clear(&mut self) {
        self.index.clear();
        if let Ok(rd) = std::fs::read_dir(self.dir.join("blobs")) {
            for e in rd.flatten() {
                if std::fs::remove_file(e.path()).is_ok() {
                    self.stats.blobs_deleted += 1;
                }
            }
        }
        let _ = Self::write_atomic(&self.dir.join("index.json"), b"{}");
    }

    /// Fetch the stored artifact for a stage id regardless of key — used
    /// right after a run, when every entry is live.
    pub fn get_any(&mut self, id: &str) -> Option<Artifact> {
        let (_, ahash) = self.index.get(id).copied()?;
        let bytes = std::fs::read(self.blob_path(&ahash)).ok()?;
        if mb_seed::hash_bytes(&bytes) != ahash {
            return None;
        }
        Some(Artifact { bytes, hash: ahash })
    }
}

impl CacheStore for DiskCache {
    fn get(&mut self, id: &str, key: &[u8; 32]) -> Option<Artifact> {
        let Some((k, ahash)) = self.index.get(id).copied() else {
            self.stats.misses += 1;
            return None;
        };
        if &k != key {
            self.stats.misses += 1;
            return None;
        }
        let path = self.blob_path(&ahash);
        match std::fs::read(&path) {
            Ok(bytes) => {
                if mb_seed::hash_bytes(&bytes) == ahash {
                    self.stats.hits += 1;
                    Some(Artifact { bytes, hash: ahash })
                } else {
                    // Corrupt: delete and miss so the stage re-runs.
                    let _ = std::fs::remove_file(&path);
                    self.stats.corrupt += 1;
                    self.stats.blobs_deleted += 1;
                    self.stats.misses += 1;
                    None
                }
            }
            Err(_) => {
                self.stats.missing += 1;
                self.stats.misses += 1;
                None
            }
        }
    }

    fn put(&mut self, id: &str, key: [u8; 32], artifact: &Artifact) {
        let path = self.blob_path(&artifact.hash);
        if !path.exists() && Self::write_atomic(&path, &artifact.bytes).is_ok() {
            self.stats.blobs_written += 1;
        }
        self.index.insert(id.to_string(), (key, artifact.hash));
    }

    fn stats(&self) -> CacheStats {
        self.stats
    }

    /// Write index.json atomically, keeping only `live_ids`, then
    /// garbage-collect blobs no longer referenced.
    fn finish(&mut self, live_ids: &[String]) {
        self.index.retain(|id, _| live_ids.contains(id));
        let mut map = serde_json::Map::new();
        for (id, (k, a)) in &self.index {
            map.insert(
                id.clone(),
                serde_json::json!({"key": hex32(k), "artifact": hex32(a)}),
            );
        }
        let text = serde_json::to_string_pretty(&serde_json::Value::Object(map))
            .unwrap_or_else(|_| "{}".into());
        let _ = Self::write_atomic(&self.dir.join("index.json"), text.as_bytes());
        let mut keep = std::collections::BTreeSet::new();
        for (_, a) in self.index.values() {
            keep.insert(hex32(a));
        }
        if let Ok(rd) = std::fs::read_dir(self.dir.join("blobs")) {
            for e in rd.flatten() {
                let p = e.path();
                let name = p.file_stem().unwrap_or_default().to_string_lossy();
                if !keep.contains(name.as_ref())
                    && p.extension().is_some_and(|x| x == "bin")
                    && std::fs::remove_file(&p).is_ok()
                {
                    self.stats.blobs_deleted += 1;
                }
            }
        }
    }
}

enum StageKind {
    Normal(Box<dyn Stage>),
    Sliced(Box<dyn SlicedStage>),
}

impl StageKind {
    fn id(&self) -> &str {
        match self {
            StageKind::Normal(s) => s.id(),
            StageKind::Sliced(s) => s.id(),
        }
    }
    fn deps(&self) -> &[String] {
        match self {
            StageKind::Normal(s) => s.deps(),
            StageKind::Sliced(s) => s.deps(),
        }
    }
    fn code_version(&self) -> u32 {
        match self {
            StageKind::Normal(s) => s.code_version(),
            StageKind::Sliced(s) => s.code_version(),
        }
    }
    fn params(&self) -> Vec<u8> {
        match self {
            StageKind::Normal(s) => s.params(),
            StageKind::Sliced(s) => s.params(),
        }
    }
}

#[derive(Debug)]
pub struct RunReport {
    /// Stages executed this run, in execution order.
    pub executed: Vec<String>,
    /// Stages served from cache.
    pub reused: Vec<String>,
    /// Final output hash per stage.
    pub outputs: BTreeMap<String, [u8; 32]>,
    /// Wall time of each executed `run()` call, microseconds.
    pub durations_us: BTreeMap<String, u64>,
    /// Cache statistics for the run.
    pub cache: CacheStats,
}

pub struct Pipeline {
    project_seed: u64,
    generator_version: u32,
    stages: Vec<StageKind>,
}

impl Pipeline {
    /// `generator_version` participates in every input hash, so a bump
    /// re-runs everything.
    pub fn new(project_seed: u64, generator_version: u32) -> Self {
        Pipeline {
            project_seed,
            generator_version,
            stages: Vec::new(),
        }
    }

    /// Register a normal stage.
    pub fn add(&mut self, stage: Box<dyn Stage>) {
        self.stages.push(StageKind::Normal(stage));
    }

    /// Register a sliced stage (see [`SlicedStage`]).
    pub fn add_sliced(&mut self, stage: Box<dyn SlicedStage>) {
        self.stages.push(StageKind::Sliced(stage));
    }

    /// Stage ids in registration order (used by DiskCache::finish).
    pub fn stage_ids(&self) -> Vec<String> {
        self.stages.iter().map(|s| s.id().to_string()).collect()
    }

    /// Deterministic topological order: Kahn's algorithm with ties broken
    /// by insertion (registration) order.
    fn topo_order(&self) -> Result<Vec<usize>, PipelineError> {
        let n = self.stages.len();
        let index_of: BTreeMap<&str, usize> = self
            .stages
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id(), i))
            .collect();
        if index_of.len() != n {
            for (i, s) in self.stages.iter().enumerate() {
                if self.stages[..i].iter().any(|o| o.id() == s.id()) {
                    return Err(PipelineError::DuplicateId(s.id().to_string()));
                }
            }
        }
        let mut indegree = vec![0usize; n];
        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, s) in self.stages.iter().enumerate() {
            for dep in s.deps() {
                match index_of.get(dep.as_str()) {
                    Some(&d) => {
                        indegree[i] += 1;
                        dependents[d].push(i);
                    }
                    None => {
                        return Err(PipelineError::UnknownDep {
                            stage: s.id().to_string(),
                            dep: dep.clone(),
                        });
                    }
                }
            }
        }
        let mut done = vec![false; n];
        let mut order = Vec::with_capacity(n);
        for _ in 0..n {
            match (0..n).find(|&i| !done[i] && indegree[i] == 0) {
                Some(i) => {
                    done[i] = true;
                    order.push(i);
                    for &d in &dependents[i] {
                        indegree[d] -= 1;
                    }
                }
                None => return Err(PipelineError::Cycle),
            }
        }
        Ok(order)
    }

    /// Input hash: blake3 over len·id, id, code_version LE,
    /// generator_version LE, len·params, params, INPUT, stage seed —
    /// where INPUT is dep output hashes (normal) or blake3(slice).
    fn input_hash(&self, stage: &StageKind, input: &[u8], stage_seed: u64) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        let id = stage.id();
        h.update(&(id.len() as u64).to_le_bytes());
        h.update(id.as_bytes());
        h.update(&stage.code_version().to_le_bytes());
        h.update(&self.generator_version.to_le_bytes());
        let params = stage.params();
        h.update(&(params.len() as u64).to_le_bytes());
        h.update(&params);
        h.update(input);
        h.update(&stage_seed.to_le_bytes());
        *h.finalize().as_bytes()
    }

    /// Run all stages, reusing cached artifacts whose inputs are
    /// unchanged.
    pub fn run(&self, cache: &mut dyn CacheStore) -> Result<RunReport, PipelineError> {
        let order = self.topo_order()?;
        let mut artifacts: BTreeMap<String, Artifact> = BTreeMap::new();
        let mut report = RunReport {
            executed: Vec::new(),
            reused: Vec::new(),
            outputs: BTreeMap::new(),
            durations_us: BTreeMap::new(),
            cache: CacheStats::default(),
        };
        for &i in &order {
            let stage = &self.stages[i];
            let id = stage.id().to_string();
            let stage_seed = mb_seed::derive(self.project_seed, "stage", &id);
            let inputs: Vec<&Artifact> = stage
                .deps()
                .iter()
                .map(|d| artifacts.get(d).expect("topo order guarantees deps ran"))
                .collect();
            // INPUT for the key; `slice` kept for a possible run.
            let (input_bytes, slice) = match stage {
                StageKind::Normal(_) => {
                    let mut v = Vec::new();
                    for a in &inputs {
                        v.extend_from_slice(&a.hash);
                    }
                    (v, None)
                }
                StageKind::Sliced(s) => {
                    let sl = s.slice(&inputs);
                    let h = mb_seed::hash_bytes(&sl);
                    (h.to_vec(), Some(sl))
                }
            };
            let ih = self.input_hash(stage, &input_bytes, stage_seed);
            let artifact = match cache.get(&id, &ih) {
                Some(cached) => {
                    report.reused.push(id.clone());
                    cached
                }
                None => {
                    let t0 = std::time::Instant::now();
                    let a = match stage {
                        StageKind::Normal(s) => s.run(&inputs, stage_seed),
                        StageKind::Sliced(s) => s.run(&slice.unwrap(), stage_seed),
                    };
                    report
                        .durations_us
                        .insert(id.clone(), t0.elapsed().as_micros() as u64);
                    cache.put(&id, ih, &a);
                    report.executed.push(id.clone());
                    a
                }
            };
            report.outputs.insert(id.clone(), artifact.hash);
            artifacts.insert(id, artifact);
        }
        report.cache = cache.stats();
        Ok(report)
    }
}
