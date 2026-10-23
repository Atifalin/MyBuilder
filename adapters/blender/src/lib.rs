//! Blender (PyO3) adapter, built as Python module `mybuilder_core`.
//! Thin: every call forwards to `mb-bridge`.

use pyo3::prelude::*;

/// Crate version string.
#[pyfunction]
fn version() -> &'static str {
    mb_bridge::version()
}

/// Determinism probe hash. Must equal the CLI `mybuilder probe` value.
#[pyfunction]
fn probe() -> String {
    mb_bridge::probe()
}

/// Normalized fBm sample at world `(x, z)`.
#[pyfunction]
#[allow(clippy::too_many_arguments)]
fn sample_fbm(
    seed: u64,
    x: f64,
    z: f64,
    octaves: u32,
    gain: f64,
    lacunarity: f64,
    frequency: f64,
) -> f64 {
    mb_bridge::sample_fbm(seed, x, z, octaves, gain, lacunarity, frequency)
}

/// Derive a child seed from a hex root and a list of (kind, key) tuples.
#[pyfunction]
fn derive_seed_hex(root_hex: &str, path: Vec<(String, String)>) -> PyResult<String> {
    let refs: Vec<(&str, &str)> = path.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    mb_bridge::derive_seed_hex(root_hex, &refs).map_err(pyo3::exceptions::PyValueError::new_err)
}

#[pymodule]
fn mybuilder_core(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(probe, m)?)?;
    m.add_function(wrap_pyfunction!(sample_fbm, m)?)?;
    m.add_function(wrap_pyfunction!(derive_seed_hex, m)?)?;
    Ok(())
}
