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
    let idx_off = bin.len();
    for &i in &mesh.indices {
        bin.extend_from_slice(&i.to_le_bytes());
    }

    let (mut pmin, mut pmax) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in &mesh.positions {
        for k in 0..3 {
            pmin[k] = pmin[k].min(p[k]);
            pmax[k] = pmax[k].max(p[k]);
        }
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
                "attributes": {
                    "POSITION": 0,
                    "NORMAL": 1,
                    "TEXCOORD_0": 2,
                    "COLOR_0": 3,
                },
                "indices": 4,
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
        "bufferViews": [
            { "buffer": 0, "byteOffset": pos_off, "byteLength": mesh.positions.len() * 12 },
            { "buffer": 0, "byteOffset": nrm_off, "byteLength": mesh.normals.len() * 12 },
            { "buffer": 0, "byteOffset": uv_off, "byteLength": mesh.uvs.len() * 8 },
            { "buffer": 0, "byteOffset": col_off, "byteLength": mesh.colors.len() * 12 },
            { "buffer": 0, "byteOffset": idx_off, "byteLength": mesh.indices.len() * 4 },
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": mesh.positions.len(),
              "type": "VEC3", "min": pmin, "max": pmax },
            { "bufferView": 1, "componentType": 5126, "count": mesh.normals.len(), "type": "VEC3" },
            { "bufferView": 2, "componentType": 5126, "count": mesh.uvs.len(), "type": "VEC2" },
            { "bufferView": 3, "componentType": 5126, "count": mesh.colors.len(), "type": "VEC3" },
            { "bufferView": 4, "componentType": 5125, "count": mesh.indices.len(), "type": "SCALAR" },
        ],
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
