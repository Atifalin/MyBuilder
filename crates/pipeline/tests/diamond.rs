use mb_pipeline::{Artifact, Cache, Pipeline, PipelineError, Stage};

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
    fn run(&self, inputs: &[&Artifact], _seed: u64) -> Artifact {
        let mut bytes = Vec::new();
        if self.echo_params {
            bytes.extend_from_slice(&self.params);
        }
        bytes.extend_from_slice(&self.payload);
        for i in inputs {
            bytes.extend_from_slice(&i.bytes);
        }
        Artifact::new(bytes)
    }
}

fn diamond() -> Pipeline {
    let mut p = Pipeline::new(42);
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
    let r = diamond().run(&mut cache).unwrap();
    assert_eq!(r.executed, ["a", "b", "c", "d"]);
    assert!(r.reused.is_empty());
}

#[test]
fn second_run_reuses_all() {
    let mut cache = Cache::default();
    diamond().run(&mut cache).unwrap();
    let r = diamond().run(&mut cache).unwrap();
    assert!(r.executed.is_empty());
    assert_eq!(r.reused, ["a", "b", "c", "d"]);
}

#[test]
fn changed_params_rerun_only_dependents() {
    let mut cache = Cache::default();
    diamond().run(&mut cache).unwrap();
    // Same pipeline but c has new params (echo_params => output changes).
    let mut p2 = Pipeline::new(42);
    p2.add(Box::new(TestStage::new("a", &[]).with_params(b"pa")));
    p2.add(Box::new(TestStage::new("b", &["a"]).with_params(b"pb")));
    p2.add(Box::new(
        TestStage::new("c", &["a"])
            .with_params(b"pc2")
            .echo_params(),
    ));
    p2.add(Box::new(TestStage::new("d", &["b", "c"])));
    let r = p2.run(&mut cache).unwrap();
    assert_eq!(r.executed, ["c", "d"]);
    assert_eq!(r.reused, ["a", "b"]);
}

#[test]
fn early_cutoff_when_output_unchanged() {
    let mut cache = Cache::default();
    diamond().run(&mut cache).unwrap();
    let mut p2 = Pipeline::new(42);
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
    let r = p2.run(&mut cache).unwrap();
    assert_eq!(r.executed, ["b"]);
    assert_eq!(r.reused, ["a", "c", "d"]);
}

#[test]
fn incremental_matches_full_rebuild() {
    let mut cache = Cache::default();
    let r1 = diamond().run(&mut cache).unwrap();
    let mut p2 = Pipeline::new(42);
    p2.add(Box::new(TestStage::new("a", &[]).with_params(b"pa")));
    p2.add(Box::new(TestStage::new("b", &["a"]).with_params(b"pb")));
    p2.add(Box::new(
        TestStage::new("c", &["a"])
            .with_params(b"pc2")
            .echo_params(),
    ));
    p2.add(Box::new(TestStage::new("d", &["b", "c"])));
    let inc = p2.run(&mut cache).unwrap();
    let mut fresh = Cache::default();
    let mut full = Pipeline::new(42);
    full.add(Box::new(TestStage::new("a", &[]).with_params(b"pa")));
    full.add(Box::new(TestStage::new("b", &["a"]).with_params(b"pb")));
    full.add(Box::new(
        TestStage::new("c", &["a"])
            .with_params(b"pc2")
            .echo_params(),
    ));
    full.add(Box::new(TestStage::new("d", &["b", "c"])));
    let full = full.run(&mut fresh).unwrap();
    assert_eq!(inc.outputs, full.outputs);
    assert_ne!(r1.outputs["d"], inc.outputs["d"]);
}

#[test]
fn golden_diamond_outputs() {
    let mut cache = Cache::default();
    let r = diamond().run(&mut cache).unwrap();
    let mut bytes = Vec::new();
    for h in r.outputs.values() {
        bytes.extend_from_slice(h);
    }
    mb_seed::golden::check_hash(DIR, "diamond_outputs", &bytes);
}

#[test]
fn cycle_is_rejected() {
    let mut p = Pipeline::new(1);
    p.add(Box::new(TestStage::new("x", &["y"])));
    p.add(Box::new(TestStage::new("y", &["x"])));
    let mut cache = Cache::default();
    assert!(matches!(p.run(&mut cache), Err(PipelineError::Cycle)));
}

#[test]
fn duplicate_id_is_rejected() {
    let mut p = Pipeline::new(1);
    p.add(Box::new(TestStage::new("x", &[])));
    p.add(Box::new(TestStage::new("x", &[])));
    let mut cache = Cache::default();
    assert!(matches!(
        p.run(&mut cache),
        Err(PipelineError::DuplicateId(_))
    ));
}

#[test]
fn unknown_dep_is_rejected() {
    let mut p = Pipeline::new(1);
    p.add(Box::new(TestStage::new("x", &["missing"])));
    let mut cache = Cache::default();
    assert!(matches!(
        p.run(&mut cache),
        Err(PipelineError::UnknownDep { .. })
    ));
}
