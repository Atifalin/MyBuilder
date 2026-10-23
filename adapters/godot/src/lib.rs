//! Godot GDExtension adapter. Thin: every call forwards to `mb-bridge`.

use godot::builtin::{GString, PackedStringArray};
use godot::classes::RefCounted;
use godot::prelude::*;

struct MyBuilderExtension;

#[gdextension]
unsafe impl ExtensionLibrary for MyBuilderExtension {}

/// Static facade exposing the core to GDScript as
/// `MyBuilderCore.probe()` etc.
#[derive(GodotClass)]
#[class(base=RefCounted)]
struct MyBuilderCore {
    base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for MyBuilderCore {
    fn init(base: Base<RefCounted>) -> Self {
        Self { base }
    }
}

#[godot_api]
impl MyBuilderCore {
    /// Crate version string.
    #[func]
    fn version() -> GString {
        GString::from(&mb_bridge::version().to_string())
    }

    /// Determinism probe hash. Must equal the CLI `mybuilder probe` value.
    #[func]
    fn probe() -> GString {
        GString::from(&mb_bridge::probe())
    }

    /// Normalized fBm sample at world `(x, z)`.
    #[func]
    fn sample_fbm(
        seed: i64,
        x: f64,
        z: f64,
        octaves: i64,
        gain: f64,
        lacunarity: f64,
        frequency: f64,
    ) -> f64 {
        mb_bridge::sample_fbm(
            seed as u64,
            x,
            z,
            octaves.max(0) as u32,
            gain,
            lacunarity,
            frequency,
        )
    }

    /// Derive a child seed. `path` entries are "kind:key" strings.
    #[func]
    fn derive_seed_hex(root_hex: GString, path: PackedStringArray) -> GString {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for s in path.to_vec().iter() {
            let s = s.to_string();
            match s.split_once(':') {
                Some((k, v)) => pairs.push((k.to_string(), v.to_string())),
                None => {
                    return GString::from(&format!("error: path entry {s:?} is not kind:key"));
                }
            }
        }
        let refs: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        match mb_bridge::derive_seed_hex(&root_hex.to_string(), &refs) {
            Ok(h) => GString::from(&h),
            Err(e) => GString::from(&format!("error: {e}")),
        }
    }
}
