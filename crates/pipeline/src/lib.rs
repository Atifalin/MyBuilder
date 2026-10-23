//! Minimal in-memory stage DAG runner with memo caching.
//!
//! Each stage is a pure function of (params, upstream outputs, seed).
//! Running the pipeline computes an input hash per stage —
//! `blake3(id, code_version, params, dep output hashes, stage seed)` —
//! and reuses the cached artifact when it matches, so a change to one
//! stage re-runs only that stage and its transitive dependents (and even
//! those stay cached when re-run output is identical: natural early
//! cutoff).

use std::collections::BTreeMap;
use std::fmt;

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

/// A deterministic pipeline stage.
pub trait Stage {
    /// Unique stage id, e.g. "terrain.heightfield".
    fn id(&self) -> &str;
    /// Upstream stage ids, in input order.
    fn deps(&self) -> &[String];
    /// Bump when the stage's implementation changes in a way that affects
    /// output; it participates in the input hash.
    fn code_version(&self) -> u32;
    /// Canonical parameter bytes (e.g. compact JSON of a typed struct).
    fn params(&self) -> Vec<u8>;
    /// Produce the artifact from upstream artifacts (in `deps()` order)
    /// and the stage seed.
    fn run(&self, inputs: &[&Artifact], seed: u64) -> Artifact;
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

/// Memo cache: stage id -> (input hash, artifact).
#[derive(Default)]
pub struct Cache {
    map: BTreeMap<String, ([u8; 32], Artifact)>,
}

impl Cache {
    /// The cached artifact for a stage, if present.
    pub fn get(&self, id: &str) -> Option<&Artifact> {
        self.map.get(id).map(|(_, a)| a)
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
    /// Wall time of each executed `run()` call, microseconds. Reused
    /// stages have no entry.
    pub durations_us: BTreeMap<String, u64>,
}

pub struct Pipeline {
    project_seed: u64,
    stages: Vec<Box<dyn Stage>>,
}

impl Pipeline {
    pub fn new(project_seed: u64) -> Self {
        Pipeline {
            project_seed,
            stages: Vec::new(),
        }
    }

    /// Register a stage. Stages may be added in any order; execution order
    /// comes from the dependency graph.
    pub fn add(&mut self, stage: Box<dyn Stage>) {
        self.stages.push(stage);
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
            // Pick the lowest insertion index whose deps are all done.
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

    /// Input hash for a stage: blake3 over len-prefixed id, code_version
    /// (LE), len-prefixed params, dep output hashes in deps() order,
    /// stage seed.
    fn input_hash(stage: &dyn Stage, dep_hashes: &[[u8; 32]], stage_seed: u64) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        let id = stage.id();
        h.update(&(id.len() as u64).to_le_bytes());
        h.update(id.as_bytes());
        h.update(&stage.code_version().to_le_bytes());
        let params = stage.params();
        h.update(&(params.len() as u64).to_le_bytes());
        h.update(&params);
        for d in dep_hashes {
            h.update(d);
        }
        h.update(&stage_seed.to_le_bytes());
        *h.finalize().as_bytes()
    }

    /// Run all stages, reusing cached artifacts whose inputs are unchanged.
    pub fn run(&self, cache: &mut Cache) -> Result<RunReport, PipelineError> {
        let order = self.topo_order()?;
        let mut artifacts: BTreeMap<String, Artifact> = BTreeMap::new();
        let mut report = RunReport {
            executed: Vec::new(),
            reused: Vec::new(),
            outputs: BTreeMap::new(),
            durations_us: BTreeMap::new(),
        };
        for &i in &order {
            let stage = &*self.stages[i];
            let id = stage.id().to_string();
            let stage_seed = mb_seed::derive(self.project_seed, "stage", &id);
            let inputs: Vec<&Artifact> = stage
                .deps()
                .iter()
                .map(|d| artifacts.get(d).expect("topo order guarantees deps ran"))
                .collect();
            let dep_hashes: Vec<[u8; 32]> = inputs.iter().map(|a| a.hash).collect();
            let ih = Self::input_hash(stage, &dep_hashes, stage_seed);
            let artifact = match cache.map.get(&id) {
                Some((cached_ih, cached)) if *cached_ih == ih => {
                    report.reused.push(id.clone());
                    cached.clone()
                }
                _ => {
                    let t0 = std::time::Instant::now();
                    let a = stage.run(&inputs, stage_seed);
                    report
                        .durations_us
                        .insert(id.clone(), t0.elapsed().as_micros() as u64);
                    cache.map.insert(id.clone(), (ih, a.clone()));
                    report.executed.push(id.clone());
                    a
                }
            };
            report.outputs.insert(id.clone(), artifact.hash);
            artifacts.insert(id, artifact);
        }
        Ok(report)
    }
}
