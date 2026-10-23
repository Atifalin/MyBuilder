use mb_pipeline::{
    Artifact, Cache, CacheStore, CancelToken, Cancelled, DiskCache, Pipeline, PipelineError,
    RunCtx, SlicedStage, Stage,
};

const DIR: &str = env!("CARGO_MANIFEST_DIR");

/// A test stage whose output is `payload` (echo_params=false) or
/// `params ++ payload` (echo_params=true), followed by input bytes.
/// This lets a test change `params` with or without changing the output.
struct TestStage {
    id: &'static str,
    deps: Vec<String>,
    code_version: u32,
    params: Vec<u8>,
    payload: Vec<u8>,
    echo_params: bool,
}

impl TestStage {
    fn new(id: &'static str, deps: &[&str]) -> Self {
        TestStage {
            id,
            deps: deps.iter().map(|s| s.to_string()).collect(),
            code_version: 1,
            params: vec![],
            payload: id.as_bytes().to_vec(),
            echo_params: false,
        }
    }

    fn with_params(mut self, params: &[u8]) -> Self {
        self.params = params.to_vec();
        self
    }

    fn echo_params(mut self) -> Self {
        self.echo_params = true;
        self
    }
}

impl Stage for TestStage {
    fn id(&self) -> &str {
        self.id
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        self.params.clone()
    }
    fn run(&self, inputs: &[&Artifact], _seed: u64, _ctx: &RunCtx) -> Result<Artifact, Cancelled> {
        let mut bytes = Vec::new();
        if self.echo_params {
            bytes.extend_from_slice(&self.params);
        }
        bytes.extend_from_slice(&self.payload);
        for i in inputs {
            bytes.extend_from_slice(&i.bytes);
        }
        Ok(Artifact::new(bytes))
    }
}

fn diamond() -> Pipeline {
    let mut p = Pipeline::new(42, 1);
    p.add(Box::new(TestStage::new("a", &[]).with_params(b"pa")));
    p.add(Box::new(TestStage::new("b", &["a"]).with_params(b"pb")));
    p.add(Box::new(
        TestStage::new("c", &["a"]).with_params(b"pc").echo_params(),
    ));
    p.add(Box::new(TestStage::new("d", &["b", "c"])));
    p
}

#[test]
fn first_run_executes_all() {
    let mut cache = Cache::default();
    let r = diamond().run(&mut cache, &CancelToken::new()).unwrap();
    assert_eq!(r.executed, ["a", "b", "c", "d"]);
    assert!(r.reused.is_empty());
}

#[test]
fn second_run_reuses_all() {
    let mut cache = Cache::default();
    diamond().run(&mut cache, &CancelToken::new()).unwrap();
    let r = diamond().run(&mut cache, &CancelToken::new()).unwrap();
    assert!(r.executed.is_empty());
    assert_eq!(r.reused, ["a", "b", "c", "d"]);
}

#[test]
fn changed_params_rerun_only_dependents() {
    let mut cache = Cache::default();
    diamond().run(&mut cache, &CancelToken::new()).unwrap();
    // Same pipeline but c has new params (echo_params => output changes).
    let mut p2 = Pipeline::new(42, 1);
    p2.add(Box::new(TestStage::new("a", &[]).with_params(b"pa")));
    p2.add(Box::new(TestStage::new("b", &["a"]).with_params(b"pb")));
    p2.add(Box::new(
        TestStage::new("c", &["a"])
            .with_params(b"pc2")
            .echo_params(),
    ));
    p2.add(Box::new(TestStage::new("d", &["b", "c"])));
    let r = p2.run(&mut cache, &CancelToken::new()).unwrap();
    assert_eq!(r.executed, ["c", "d"]);
    assert_eq!(r.reused, ["a", "b"]);
}

#[test]
fn early_cutoff_when_output_unchanged() {
    let mut cache = Cache::default();
    diamond().run(&mut cache, &CancelToken::new()).unwrap();
    let mut p2 = Pipeline::new(42, 1);
    p2.add(Box::new(TestStage::new("a", &[]).with_params(b"pa")));
    // b's params changed, but b does not echo params so its output is
    // identical: d's input hash is unchanged and d is reused.
    p2.add(Box::new(
        TestStage::new("b", &["a"]).with_params(b"pb-changed"),
    ));
    p2.add(Box::new(
        TestStage::new("c", &["a"]).with_params(b"pc").echo_params(),
    ));
    p2.add(Box::new(TestStage::new("d", &["b", "c"])));
    let r = p2.run(&mut cache, &CancelToken::new()).unwrap();
    assert_eq!(r.executed, ["b"]);
    assert_eq!(r.reused, ["a", "c", "d"]);
}

#[test]
fn incremental_matches_full_rebuild() {
    let mut cache = Cache::default();
    let r1 = diamond().run(&mut cache, &CancelToken::new()).unwrap();
    let mut p2 = Pipeline::new(42, 1);
    p2.add(Box::new(TestStage::new("a", &[]).with_params(b"pa")));
    p2.add(Box::new(TestStage::new("b", &["a"]).with_params(b"pb")));
    p2.add(Box::new(
        TestStage::new("c", &["a"])
            .with_params(b"pc2")
            .echo_params(),
    ));
    p2.add(Box::new(TestStage::new("d", &["b", "c"])));
    let inc = p2.run(&mut cache, &CancelToken::new()).unwrap();
    let mut fresh = Cache::default();
    let mut full = Pipeline::new(42, 1);
    full.add(Box::new(TestStage::new("a", &[]).with_params(b"pa")));
    full.add(Box::new(TestStage::new("b", &["a"]).with_params(b"pb")));
    full.add(Box::new(
        TestStage::new("c", &["a"])
            .with_params(b"pc2")
            .echo_params(),
    ));
    full.add(Box::new(TestStage::new("d", &["b", "c"])));
    let full = full.run(&mut fresh, &CancelToken::new()).unwrap();
    assert_eq!(inc.outputs, full.outputs);
    assert_ne!(r1.outputs["d"], inc.outputs["d"]);
}

#[test]
fn golden_diamond_outputs() {
    let mut cache = Cache::default();
    let r = diamond().run(&mut cache, &CancelToken::new()).unwrap();
    let mut bytes = Vec::new();
    for h in r.outputs.values() {
        bytes.extend_from_slice(h);
    }
    mb_seed::golden::check_hash(DIR, "diamond_outputs", &bytes);
}

#[test]
fn cycle_is_rejected() {
    let mut p = Pipeline::new(1, 1);
    p.add(Box::new(TestStage::new("x", &["y"])));
    p.add(Box::new(TestStage::new("y", &["x"])));
    let mut cache = Cache::default();
    assert!(matches!(
        p.run(&mut cache, &CancelToken::new()),
        Err(PipelineError::Cycle)
    ));
}

#[test]
fn duplicate_id_is_rejected() {
    let mut p = Pipeline::new(1, 1);
    p.add(Box::new(TestStage::new("x", &[])));
    p.add(Box::new(TestStage::new("x", &[])));
    let mut cache = Cache::default();
    assert!(matches!(
        p.run(&mut cache, &CancelToken::new()),
        Err(PipelineError::DuplicateId(_))
    ));
}

#[test]
fn unknown_dep_is_rejected() {
    let mut p = Pipeline::new(1, 1);
    p.add(Box::new(TestStage::new("x", &["missing"])));
    let mut cache = Cache::default();
    assert!(matches!(
        p.run(&mut cache, &CancelToken::new()),
        Err(PipelineError::UnknownDep { .. })
    ));
}

// --- S10: sliced stages + cache stores ---

fn deps_holder() -> Vec<String> {
    vec!["src".to_string()]
}

struct Source(u32);
impl Stage for Source {
    fn id(&self) -> &str {
        "src"
    }
    fn deps(&self) -> &[String] {
        &[]
    }
    fn code_version(&self) -> u32 {
        1
    }
    fn params(&self) -> Vec<u8> {
        self.0.to_le_bytes().to_vec()
    }
    fn run(&self, _i: &[&Artifact], _s: u64, _ctx: &RunCtx) -> Result<Artifact, Cancelled> {
        Ok(Artifact::new(vec![self.0 as u8, 42]))
    }
}

#[test]
fn sliced_ignores_upstream_changes_outside_slice() {
    // src emits [params_byte, 42]; slice takes byte 0 only, so a params
    // change re-executes src AND changes byte0 -> slice changes too.
    // To isolate, make a source whose params change byte 1 only.
    // byte0 is always 9; byte1 varies with params so a params change
    // alters the artifact hash but never the slice.
    struct Src2(u8);
    impl Stage for Src2 {
        fn id(&self) -> &str {
            "src"
        }
        fn deps(&self) -> &[String] {
            &[]
        }
        fn code_version(&self) -> u32 {
            1
        }
        fn params(&self) -> Vec<u8> {
            vec![self.0]
        }
        fn run(&self, _i: &[&Artifact], _s: u64, _ctx: &RunCtx) -> Result<Artifact, Cancelled> {
            Ok(Artifact::new(vec![9, self.0]))
        }
    }
    let d = deps_holder();
    struct Sl2 {
        d: Vec<String>,
    }
    impl SlicedStage for Sl2 {
        fn id(&self) -> &str {
            "sl"
        }
        fn deps(&self) -> &[String] {
            &self.d
        }
        fn code_version(&self) -> u32 {
            1
        }
        fn params(&self) -> Vec<u8> {
            vec![]
        }
        fn slice(&self, inputs: &[&Artifact]) -> Vec<u8> {
            vec![inputs[0].bytes[0]]
        }
        fn run(&self, slice: &[u8], _s: u64, _ctx: &RunCtx) -> Result<Artifact, Cancelled> {
            Ok(Artifact::new(slice.to_vec()))
        }
    }
    // First run: src executes, sl executes.
    let mut cache = Cache::default();
    let mut p = Pipeline::new(7, 1);
    p.add(Box::new(Src2(1)));
    p.add_sliced(Box::new(Sl2 { d: d.clone() }));
    let r1 = p.run(&mut cache, &CancelToken::new()).unwrap();
    assert!(r1.executed.contains(&"src".to_string()));
    assert!(r1.executed.contains(&"sl".to_string()));
    // Same project_seed, but src's params change -> src's OUTPUT hash
    // changes (byte1) while the SLICE (byte0=9) stays identical, so sl
    // is reused even though its dep re-executed.
    let mut p2 = Pipeline::new(7, 1);
    p2.add(Box::new(Src2(2)));
    p2.add_sliced(Box::new(Sl2 { d }));
    let r2 = p2.run(&mut cache, &CancelToken::new()).unwrap();
    assert!(r2.executed.contains(&"src".to_string()));
    assert!(r2.reused.contains(&"sl".to_string()), "report: {r2:?}");
}

#[test]
fn disk_and_memory_cache_agree() {
    let dir = std::env::temp_dir().join(format!("mbpipe_disk_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    struct Guard(std::path::PathBuf);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _guard = Guard(dir.clone());
    let d = deps_holder();
    struct Sl3 {
        d: Vec<String>,
    }
    impl SlicedStage for Sl3 {
        fn id(&self) -> &str {
            "sl"
        }
        fn deps(&self) -> &[String] {
            &self.d
        }
        fn code_version(&self) -> u32 {
            1
        }
        fn params(&self) -> Vec<u8> {
            vec![]
        }
        fn slice(&self, inputs: &[&Artifact]) -> Vec<u8> {
            inputs[0].bytes.clone()
        }
        fn run(&self, slice: &[u8], _s: u64, _ctx: &RunCtx) -> Result<Artifact, Cancelled> {
            Ok(Artifact::new(slice.to_vec()))
        }
    }
    let mk = |d: Vec<String>| {
        let mut p = Pipeline::new(1, 1);
        p.add(Box::new(Source(5)));
        p.add_sliced(Box::new(Sl3 { d }));
        p
    };
    let mut mem = Cache::default();
    let rm1 = mk(d.clone()).run(&mut mem, &CancelToken::new()).unwrap();
    let rm2 = mk(d.clone()).run(&mut mem, &CancelToken::new()).unwrap();
    let mut disk = DiskCache::open(&dir).unwrap();
    let rd1 = mk(d.clone()).run(&mut disk, &CancelToken::new()).unwrap();
    disk.finish(&mk(d.clone()).stage_ids());
    let rd2 = mk(d).run(&mut disk, &CancelToken::new()).unwrap();
    assert_eq!(rm1.outputs, rd1.outputs);
    assert_eq!(rm2.outputs, rd2.outputs);
    assert_eq!(rd1.executed, rm1.executed);
    assert_eq!(rd2.reused, rm2.reused);
}

// --- Step 1: cooperative cancellation ---

/// Stage "b" cancels the token inside run -> Err(Cancelled).
#[test]
fn cancel_inside_stage() {
    let tok = CancelToken::new();
    struct Cancelling(CancelToken, Vec<String>);
    impl Stage for Cancelling {
        fn id(&self) -> &str {
            "b"
        }
        fn deps(&self) -> &[String] {
            self.1.as_slice()
        }
        fn code_version(&self) -> u32 {
            1
        }
        fn params(&self) -> Vec<u8> {
            vec![]
        }
        fn run(&self, _i: &[&Artifact], _s: u64, ctx: &RunCtx) -> Result<Artifact, Cancelled> {
            self.0.cancel();
            let _ = ctx;
            Err(Cancelled)
        }
    }
    let mut cache = Cache::default();
    let mut p = Pipeline::new(7, 1);
    p.add(Box::new(TestStage::new("a", &[])));
    p.add(Box::new(Cancelling(tok.clone(), vec!["a".to_string()])));
    p.add(Box::new(TestStage::new("d", &["b"])));
    let rep = p.run(&mut cache, &tok).unwrap();
    assert!(rep.cancelled);
    assert!(!rep.executed.contains(&"d".to_string()));
    // a ran and was cached; b has no entry.
    assert!(cache.get("a").is_some());
    assert!(cache.get("b").is_none());
    assert!(cache.get("d").is_none());
    // Rerun the same graph (normal b) on the SAME cache: a is reused
    // from its pre-cancel entry, b and d run, and the outputs equal a
    // never-cancelled control run on a fresh cache.
    let mut q = Pipeline::new(7, 1);
    q.add(Box::new(TestStage::new("a", &[])));
    q.add(Box::new(TestStage::new("b", &["a"])));
    q.add(Box::new(TestStage::new("d", &["b"])));
    let rep2 = q.run(&mut cache, &CancelToken::new()).unwrap();
    assert!(!rep2.cancelled);
    assert!(rep2.reused.contains(&"a".to_string()));
    let mut cache2 = Cache::default();
    let refrep = q.run(&mut cache2, &CancelToken::new()).unwrap();
    assert_eq!(rep2.outputs, refrep.outputs);
}

#[test]
fn cancelled_run_leaves_disk_cache_valid() {
    let dir = std::env::temp_dir().join(format!("mbpipe_cancel_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    struct G(std::path::PathBuf);
    impl Drop for G {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _g = G(dir.clone());
    // First complete run populates a.
    {
        let mut disk = DiskCache::open(&dir).unwrap();
        let mut p = Pipeline::new(1, 1);
        p.add(Box::new(TestStage::new("a", &[])));
        p.add(Box::new(TestStage::new("b", &["a"])));
        let rep = p.run(&mut disk, &CancelToken::new()).unwrap();
        assert!(!rep.cancelled);
        disk.finish(&p.stage_ids());
    }
    // Cancelled run: a is reused, b never runs.
    {
        let mut disk = DiskCache::open(&dir).unwrap();
        let tok = CancelToken::new();
        tok.cancel();
        let mut p = Pipeline::new(1, 1);
        p.add(Box::new(TestStage::new("a", &[])));
        p.add(Box::new(TestStage::new("b", &["a"])));
        let rep = p.run(&mut disk, &tok).unwrap();
        assert!(rep.cancelled);
        // Re-open: every index entry's blob exists and verifies.
        drop(disk);
        let mut disk2 = DiskCache::open(&dir).unwrap();
        let a = disk2.get_any("a").expect("a still cached after cancel");
        assert_eq!(a.bytes, b"a");
        let b = disk2.get_any("b").expect("b still cached after cancel");
        assert_eq!(b.bytes, b"ba");
    }
}
