//! PyO3 extension modules resolve Python symbols at load time, not link
//! time. On macOS the linker errors on undefined symbols unless told
//! otherwise, so emit the per-cdylib `-undefined dynamic_lookup` args
//! (scoped to this crate's cdylib target only — unlike global rustflags,
//! it cannot mask real link errors in other artifacts).
fn main() {
    pyo3_build_config::add_extension_module_link_args();
}
