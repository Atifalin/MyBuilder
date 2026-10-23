//! Engine-agnostic facade over the My Builder core crates.
//!
//! Adapters (Godot, Blender, CLI, app, MCP) must call this crate only —
//! never the core crates directly — so the surface they bind stays small
//! and stable.

/// Crate version, e.g. "0.1.0".
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Determinism probe: the hex BLAKE3 of a fixed fBm grid plus a fixed
/// derived seed. Identical output in the CLI, Godot and Blender proves
/// the whole chain (seed derivation, Perlin/fBm, hashing) is portable.
///
/// The grid is 129x129 at 4 m spacing starting at world (-256, -256),
/// seed 42, Hilly preset parameters at frequency 0.01, quantized to u16
/// exactly like the fbm goldens; then the 8 LE bytes of
/// `derive_path(1, [("region","r0"), ("settlement","s0")])`.
pub fn probe() -> String {
    let params = mb_schema::TerrainFeel::Hilly.preset();
    let fbm_params = mb_geom::FbmParams {
        octaves: params.octaves,
        lacunarity: params.lacunarity,
        gain: params.gain,
        frequency: 0.01,
    };
    let perlin = mb_geom::Perlin::new(42);
    let mut bytes = Vec::with_capacity(129 * 129 * 2 + 8);
    for iz in 0..129 {
        for ix in 0..129 {
            let x = -256.0 + ix as f64 * 4.0;
            let z = -256.0 + iz as f64 * 4.0;
            let v = mb_geom::fbm(&perlin, x, z, &fbm_params);
            let q = ((v.clamp(-1.0, 1.0) + 1.0) * 0.5 * 65535.0).round() as u16;
            bytes.extend_from_slice(&q.to_le_bytes());
        }
    }
    bytes.extend_from_slice(
        &mb_seed::derive_path(1, &[("region", "r0"), ("settlement", "s0")]).to_le_bytes(),
    );
    mb_seed::hex_bytes(&mb_seed::hash_bytes(&bytes))
}

/// Sample normalized fBm noise at world coordinates `(x, z)` (meters).
/// Output is approximately in [-1, 1].
pub fn sample_fbm(
    seed: u64,
    x: f64,
    z: f64,
    octaves: u32,
    gain: f64,
    lacunarity: f64,
    frequency: f64,
) -> f64 {
    let perlin = mb_geom::Perlin::new(seed);
    let params = mb_geom::FbmParams {
        octaves,
        lacunarity,
        gain,
        frequency,
    };
    mb_geom::fbm(&perlin, x, z, &params)
}

/// Derive a child seed from a hex-encoded root and a (kind, key) path,
/// returning it as a 16-char hex string.
pub fn derive_seed_hex(root_hex: &str, path: &[(&str, &str)]) -> Result<String, String> {
    let root = u64::from_str_radix(root_hex.trim(), 16)
        .map_err(|e| format!("invalid hex seed {root_hex:?}: {e}"))?;
    Ok(format!("{:016x}", mb_seed::derive_path(root, path)))
}
