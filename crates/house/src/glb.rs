//! Minimal deterministic GLB writer for house LODs: one mesh, one
//! primitive per material, POSITION/NORMAL/TEXCOORD_0/COLOR_0 + u32 idx.

use crate::{Lod, Mat, Prim};

pub const MAT_NAMES: [&str; 6] = ["plaster", "stone", "roof", "floor", "opening", "wood"];

pub fn mat_rgb(m: Mat, plaster_rgb: [f32; 3]) -> [f32; 3] {
    match m {
        Mat::Plaster => plaster_rgb,
        Mat::Stone => [0.62, 0.30, 0.22],
        Mat::Roof => [0.70, 0.62, 0.52],
        Mat::Floor => [0.55, 0.45, 0.38],
        Mat::Opening => [0.12, 0.10, 0.09],
        Mat::Wood => [0.36, 0.22, 0.12],
    }
}

fn f32s(v: &mut Vec<u8>, x: f32) {
    v.extend_from_slice(&x.to_le_bytes());
}

/// GLB for one LOD. Deterministic bytes (fixed JSON layout, 4-byte pad).
pub fn lod_glb(lod: &Lod, house_id: &str, lod_index: u8, plaster_rgb: [f32; 3]) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    let mut accessors = String::new();
    let mut views = String::new();
    let mut prims_json = String::new();
    let mut acc_i = 0usize;
    let mut view_i = 0usize;
    let mut prims_used: Vec<&Prim> = lod.prims.iter().filter(|p| !p.indices.is_empty()).collect();
    prims_used.sort_by_key(|p| p.mat as usize);
    for (pi, p) in prims_used.iter().enumerate() {
        if pi > 0 {
            prims_json.push(',');
        }
        let mut attrs = String::new();
        // POSITION with min/max
        let (mut mn, mut mx) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in &p.positions {
            for k in 0..3 {
                mn[k] = mn[k].min(v[k]);
                mx[k] = mx[k].max(v[k]);
            }
        }
        let off = bin.len();
        for v in &p.positions {
            for c in v {
                f32s(&mut bin, *c);
            }
        }
        views += &format!(
            "{} {{\"buffer\":0,\"byteOffset\":{off},\"byteLength\":{}}}",
            if view_i > 0 { "," } else { "" },
            p.positions.len() * 12
        );
        view_i += 1;
        attrs += &format!("\"POSITION\":{acc_i}");
        accessors += &format!(
            "{} {{\"bufferView\":{},\"componentType\":5126,\"count\":{},\"type\":\"VEC3\",\"min\":[{},{},{}],\"max\":[{},{},{}]}}",
            if acc_i > 0 { "," } else { "" },
            view_i - 1,
            p.positions.len(),
            mn[0],
            mn[1],
            mn[2],
            mx[0],
            mx[1],
            mx[2]
        );
        acc_i += 1;
        // NORMAL
        let off = bin.len();
        for v in &p.normals {
            for c in v {
                f32s(&mut bin, *c);
            }
        }
        views += &format!(
            ",{{\"buffer\":0,\"byteOffset\":{off},\"byteLength\":{}}}",
            p.normals.len() * 12
        );
        view_i += 1;
        attrs += ",\"NORMAL\":";
        attrs += &acc_i.to_string();
        accessors += &format!(
            ",{{\"bufferView\":{},\"componentType\":5126,\"count\":{},\"type\":\"VEC3\"}}",
            view_i - 1,
            p.normals.len()
        );
        acc_i += 1;
        // TEXCOORD_0
        let off = bin.len();
        for v in &p.uvs {
            for c in v {
                f32s(&mut bin, *c);
            }
        }
        views += &format!(
            ",{{\"buffer\":0,\"byteOffset\":{off},\"byteLength\":{}}}",
            p.uvs.len() * 8
        );
        view_i += 1;
        attrs += ",\"TEXCOORD_0\":";
        attrs += &acc_i.to_string();
        accessors += &format!(
            ",{{\"bufferView\":{},\"componentType\":5126,\"count\":{},\"type\":\"VEC2\"}}",
            view_i - 1,
            p.uvs.len()
        );
        acc_i += 1;
        // COLOR_0 = material colour (VEC3 float)
        let rgb = mat_rgb(p.mat, plaster_rgb);
        let off = bin.len();
        for _ in &p.positions {
            for c in rgb {
                f32s(&mut bin, c);
            }
        }
        views += &format!(
            ",{{\"buffer\":0,\"byteOffset\":{off},\"byteLength\":{}}}",
            p.positions.len() * 12
        );
        view_i += 1;
        attrs += ",\"COLOR_0\":";
        attrs += &acc_i.to_string();
        accessors += &format!(
            ",{{\"bufferView\":{},\"componentType\":5126,\"count\":{},\"type\":\"VEC3\"}}",
            view_i - 1,
            p.positions.len()
        );
        acc_i += 1;
        // indices
        let off = bin.len();
        for &i in &p.indices {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        views += &format!(
            ",{{\"buffer\":0,\"byteOffset\":{off},\"byteLength\":{}}}",
            p.indices.len() * 4
        );
        view_i += 1;
        accessors += &format!(
            ",{{\"bufferView\":{},\"componentType\":5125,\"count\":{},\"type\":\"SCALAR\"}}",
            view_i - 1,
            p.indices.len()
        );
        acc_i += 1;
        prims_json += &format!(
            "{{\"attributes\":{{{attrs}}},\"indices\":{},\"material\":{}}}",
            acc_i - 1,
            pi
        );
    }
    let mut mats = String::new();
    for (i, n) in MAT_NAMES.iter().enumerate() {
        if i > 0 {
            mats.push(',');
        }
        // colour is per-prim; emit the canonical colour for the mat name
        let rgb = mat_rgb(crate::MATS_ORDER[i], plaster_rgb);
        mats += &format!(
            "{{\"name\":\"{n}\",\"pbrMetallicRoughness\":{{\"baseColorFactor\":[{},{},1.0],\"metallicFactor\":0.0,\"roughnessFactor\":0.9}}}}",
            rgb[0], rgb[1]
        );
    }
    // materials must carry all three channels — rebuild mats string
    let mut mats2 = String::new();
    for (i, n) in MAT_NAMES.iter().enumerate() {
        if i > 0 {
            mats2.push(',');
        }
        let rgb = mat_rgb(crate::MATS_ORDER[i], plaster_rgb);
        mats2 += &format!(
            "{{\"name\":\"{n}\",\"pbrMetallicRoughness\":{{\"baseColorFactor\":[{},{},{},1.0],\"metallicFactor\":0.0,\"roughnessFactor\":0.9}}}}",
            rgb[0], rgb[1], rgb[2]
        );
    }
    let _ = mats;
    let json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"mb-house\"}},\"scene\":0,\"scenes\":[{{\"nodes\":[0]}}],\"nodes\":[{{\"mesh\":0,\"name\":\"{house_id}\",\"extras\":{{\"mb_house_id\":\"{house_id}\",\"lod\":{lod_index}}}}}],\"meshes\":[{{\"primitives\":[{prims_json}]}}],\"materials\":[{mats2}],\"accessors\":[{accessors}],\"bufferViews\":[{views}],\"buffers\":[{{\"byteLength\":{}}}]}}",
        bin.len()
    );
    let mut jbytes = json.into_bytes();
    while jbytes.len() % 4 != 0 {
        jbytes.push(b' ');
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + jbytes.len() + 8 + bin.len();
    let mut glb = Vec::with_capacity(total);
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total as u32).to_le_bytes());
    glb.extend_from_slice(&(jbytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&jbytes);
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin);
    glb
}
