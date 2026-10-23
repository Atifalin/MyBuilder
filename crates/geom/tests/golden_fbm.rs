use mb_geom::{FbmParams, Perlin, fbm, ridged_fbm};

const DIR: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn golden_fbm_grids() {
    // Sample a 257x257 grid at 4 m spacing starting at (-512, -512) in
    // world coordinates, quantize each sample to u16, hash the LE bytes.
    // Params mirror the mb-schema feel presets (octaves, gain,
    // lacunarity); they are duplicated here because mb-geom must not
    // depend on mb-schema.
    let perlin = Perlin::new(42);
    let feels = [
        (
            "bumpy",
            FbmParams {
                octaves: 4,
                gain: 0.45,
                lacunarity: 2.0,
                frequency: 0.01,
            },
        ),
        (
            "hilly",
            FbmParams {
                octaves: 5,
                gain: 0.50,
                lacunarity: 2.0,
                frequency: 0.01,
            },
        ),
        (
            "mountainous",
            FbmParams {
                octaves: 7,
                gain: 0.55,
                lacunarity: 2.0,
                frequency: 0.01,
            },
        ),
    ];
    for (name, params) in feels {
        let mut fbm_bytes = Vec::new();
        let mut ridged_bytes = Vec::new();
        for iz in 0..257 {
            for ix in 0..257 {
                let x = -512.0 + ix as f64 * 4.0;
                let z = -512.0 + iz as f64 * 4.0;
                let q = |v: f64| {
                    (((v.clamp(-1.0, 1.0) + 1.0) * 0.5 * 65535.0).round() as u16).to_le_bytes()
                };
                fbm_bytes.extend_from_slice(&q(fbm(&perlin, x, z, &params)));
                ridged_bytes.extend_from_slice(&q(ridged_fbm(&perlin, x, z, &params)));
            }
        }
        mb_seed::golden::check_hash(DIR, &format!("fbm_{name}"), &fbm_bytes);
        mb_seed::golden::check_hash(DIR, &format!("ridged_fbm_{name}"), &ridged_bytes);
    }
}
