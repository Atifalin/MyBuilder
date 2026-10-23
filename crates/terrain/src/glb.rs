//! Minimal deterministic GLB 2.0 writer.
//!
//! Layout: 12-byte header (`glTF`, version 2, total length), a JSON
//! chunk padded to 4 bytes with spaces (0x20), and one BIN chunk padded
//! to 4 bytes with zeros. JSON is built with serde_json::Value, which
//! orders keys deterministically (BTreeMap-backed), so output bytes are
//! stable.

use crate::mesh::ChunkMesh;

fn pad4(v: &mut Vec<u8>, byte: u8) {
    while !v.len().is_multiple_of(4) {
        v.push(byte);
    }
}

fn push_f32(buf: &mut Vec<u8>, v: f32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn bviews(list: &[(usize, usize)], drop_empty: bool) -> serde_json::Value {
    let mut out = Vec::new();
    for (i, &(off, len)) in list.iter().enumerate() {
        if drop_empty && i == 4 {
            continue; // uv2: omit entirely on gen 1
        }
        out.push(serde_json::json!({
            "buffer": 0, "byteOffset": off, "byteLength": len,
        }));
    }
    serde_json::Value::Array(out)
}

fn accessors(mesh: &ChunkMesh) -> serde_json::Value {
    let (mut pmin, mut pmax) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &mesh.positions {
        for k in 0..3 {
            pmin[k] = pmin[k].min(p[k]);
            pmax[k] = pmax[k].max(p[k]);
        }
    }
    let mut out = vec![
        serde_json::json!({ "bufferView": 0, "componentType": 5126, "count": mesh.positions.len(),
          "type": "VEC3", "min": pmin, "max": pmax }),
        serde_json::json!({ "bufferView": 1, "componentType": 5126, "count": mesh.normals.len(), "type": "VEC3" }),
        serde_json::json!({ "bufferView": 2, "componentType": 5126, "count": mesh.uvs.len(), "type": "VEC2" }),
        serde_json::json!({ "bufferView": 3, "componentType": 5126, "count": mesh.colors.len(), "type": "VEC3" }),
    ];
    if !mesh.uv2.is_empty() {
        out.push(serde_json::json!({ "bufferView": 4, "componentType": 5126, "count": mesh.uv2.len(), "type": "VEC2" }));
    }
    out.push(serde_json::json!({
        "bufferView": if mesh.uv2.is_empty() { 4 } else { 5 },
        "componentType": 5125,
        "count": mesh.indices.len(),
        "type": "SCALAR",
    }));
    serde_json::Value::Array(out)
}

/// Serialize a chunk mesh to GLB bytes. `name` is the node name and
/// `extras` the per-chunk metadata (id, cx, cz).
pub fn chunk_glb(mesh: &ChunkMesh, name: &str, id_hex: &str, cx: usize, cz: usize) -> Vec<u8> {
    // --- BIN payload: positions | normals | uvs | colors | indices ---
    let mut bin = Vec::new();
    let pos_off = bin.len();
    for p in &mesh.positions {
        push_f32(&mut bin, p[0]);
        push_f32(&mut bin, p[1]);
        push_f32(&mut bin, p[2]);
    }
    let nrm_off = bin.len();
    for n in &mesh.normals {
        push_f32(&mut bin, n[0]);
        push_f32(&mut bin, n[1]);
        push_f32(&mut bin, n[2]);
    }
    let uv_off = bin.len();
    for t in &mesh.uvs {
        push_f32(&mut bin, t[0]);
        push_f32(&mut bin, t[1]);
    }
    let col_off = bin.len();
    for c in &mesh.colors {
        push_f32(&mut bin, c[0]);
        push_f32(&mut bin, c[1]);
        push_f32(&mut bin, c[2]);
    }
    let uv2_off = bin.len();
    for t in &mesh.uv2 {
        push_f32(&mut bin, t[0]);
        push_f32(&mut bin, t[1]);
    }
    let idx_off = bin.len();
    for &i in &mesh.indices {
        bin.extend_from_slice(&i.to_le_bytes());
    }

    let v = serde_json::json!({
        "asset": { "version": "2.0", "generator": format!("mybuilder {}", mb_bridge_version()) },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{
            "name": name,
            "mesh": 0,
            "translation": mesh.origin,
            "extras": { "id": id_hex, "cx": cx, "cz": cz },
        }],
        "meshes": [{
            "primitives": [{
                "attributes": if mesh.uv2.is_empty() {
                    serde_json::json!({
                        "POSITION": 0,
                        "NORMAL": 1,
                        "TEXCOORD_0": 2,
                        "COLOR_0": 3,
                    })
                } else {
                    serde_json::json!({
                        "POSITION": 0,
                        "NORMAL": 1,
                        "TEXCOORD_0": 2,
                        "COLOR_0": 3,
                        "TEXCOORD_1": 4,
                    })
                },
                "indices": if mesh.uv2.is_empty() { 4 } else { 5 },
                "material": 0,
                "mode": 4,
            }]
        }],
        "materials": [{
            "name": "terrain",
            "pbrMetallicRoughness": {
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "metallicFactor": 0.0,
                "roughnessFactor": 1.0,
            }
        }],
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": bviews(&[
            (pos_off, mesh.positions.len() * 12),
            (nrm_off, mesh.normals.len() * 12),
            (uv_off, mesh.uvs.len() * 8),
            (col_off, mesh.colors.len() * 12),
            (uv2_off, mesh.uv2.len() * 8),
            (idx_off, mesh.indices.len() * 4),
        ], mesh.uv2.is_empty()),
        "accessors": accessors(mesh),
    });
    let mut json = serde_json::to_vec(&v).expect("glb json");
    pad4(&mut json, b' ');
    pad4(&mut bin, 0);

    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    debug_assert_eq!(out.len(), total);
    out
}

fn mb_bridge_version() -> &'static str {
    // Avoid a dependency on mb-bridge just for the generator string.
    env!("CARGO_PKG_VERSION")
}
