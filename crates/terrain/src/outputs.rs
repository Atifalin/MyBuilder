//! Everything the export stage emits as files, plus the write policy.

use std::path::Path;

use mb_pipeline::Artifact;
use mb_schema::{ProjectFile, WorldRecipe};

use crate::glb::chunk_glb;
use crate::mesh::{chunk_mesh, normals};
use crate::{Grid, HeightField, Masks, quant_step, quantize};

/// An ordered list of project-relative output files.
/// Wire format: u32 path_len, path bytes, u64 data_len, data — repeated.
pub struct FileBundle {
    pub files: Vec<(String, Vec<u8>)>,
}

impl FileBundle {
    pub fn encode(&self) -> Artifact {
        let mut b = Vec::new();
        for (path, data) in &self.files {
            b.extend_from_slice(&(path.len() as u32).to_le_bytes());
            b.extend_from_slice(path.as_bytes());
            b.extend_from_slice(&(data.len() as u64).to_le_bytes());
            b.extend_from_slice(data);
        }
        Artifact::new(b)
    }

    pub fn decode(b: &[u8]) -> FileBundle {
        let mut files = Vec::new();
        let mut o = 0usize;
        while o < b.len() {
            let plen = u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
            o += 4;
            let path = String::from_utf8(b[o..o + plen].to_vec()).unwrap();
            o += plen;
            let dlen = u64::from_le_bytes(b[o..o + 8].try_into().unwrap()) as usize;
            o += 8;
            files.push((path, b[o..o + dlen].to_vec()));
            o += dlen;
        }
        FileBundle { files }
    }
}

/// Path of the manifest inside the bundle; written last and atomically.
pub const MANIFEST_PATH: &str = "exports/terrain/manifest.json";

/// Build every output file for a world build.
pub fn build_export_bundle(
    grid: &Grid,
    project: &ProjectFile,
    world: &WorldRecipe,
    hf: &HeightField,
    masks: &Masks,
    coast: &[f64],
) -> FileBundle {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let n = grid.len();

    // --- layers ---
    let mut r16 = Vec::with_capacity(n * 2);
    for &h in &hf.h {
        r16.extend_from_slice(&quantize(h, hf.h_min, hf.h_max).to_le_bytes());
    }
    files.push(("layers/height.r16".to_string(), r16));

    // Cropped W/cell x D/cell version for Terrain3D (drops last row/col).
    let mut r16t = Vec::with_capacity((grid.nx - 1) * (grid.nz - 1) * 2);
    for j in 0..grid.nz - 1 {
        for i in 0..grid.nx - 1 {
            let q = quantize(hf.h[j * grid.nx + i], hf.h_min, hf.h_max);
            r16t.extend_from_slice(&q.to_le_bytes());
        }
    }
    files.push(("layers/height_terrain3d.r16".to_string(), r16t));
    files.push(("layers/land.u8".to_string(), masks.land.clone()));
    files.push(("layers/mountain.u8".to_string(), masks.mountain.clone()));
    files.push(("layers/forest.u8".to_string(), masks.forest.clone()));

    let layers_json = serde_json::json!({
        "schema_version": 1,
        "nx": grid.nx,
        "nz": grid.nz,
        "cell_size_m": grid.cell,
        "origin": [grid.origin_x, grid.origin_z],
        "h_min": hf.h_min,
        "h_max": hf.h_max,
        "step": quant_step(hf.h_min, hf.h_max),
        "files": {
            "height": "height.r16",
            "height_terrain3d": "height_terrain3d.r16",
            "land": "land.u8",
            "mountain": "mountain.u8",
            "forest": "forest.u8",
        }
    });
    files.push(("layers/layers.json".to_string(), pretty(&layers_json)));

    // --- preview ---
    files.push((
        "previews/hillshade.png".to_string(),
        hillshade_png(grid, hf, world.sea_level_m),
    ));

    // --- chunks + manifest ---
    let normals_full = normals(grid, hf);
    let step = world.export.mesh_step_cells as usize;
    let cells_per_chunk = (world.chunk_size_m / grid.cell) as usize;
    let ncx = (grid.nx - 1) / cells_per_chunk;
    let ncz = (grid.nz - 1) / cells_per_chunk;
    let mut chunk_entries = Vec::new();
    let (mut total_v, mut total_t) = (0usize, 0usize);
    for cz in 0..ncz {
        for cx in 0..ncx {
            let name = format!("chunk_{cx}_{cz}");
            let id = mb_seed::EntityId::from_path(
                project.seed,
                &[("world", "main"), ("chunk", &format!("{cx}_{cz}"))],
            );
            let mesh = chunk_mesh(
                grid,
                hf,
                &normals_full,
                coast,
                world.sea_level_m,
                cells_per_chunk,
                step,
                cx,
                cz,
            );
            let glb = chunk_glb(&mesh, &name, &id.to_string(), cx, cz);
            chunk_entries.push(serde_json::json!({
                "id": id.to_string(),
                "cx": cx,
                "cz": cz,
                "file": format!("{name}.glb"),
                "origin": mesh.origin,
                "vertex_count": mesh.vertex_count,
                "triangle_count": mesh.triangle_count,
                "blake3": mb_seed::hex_bytes(&mb_seed::hash_bytes(&glb)),
            }));
            total_v += mesh.vertex_count;
            total_t += mesh.triangle_count;
            files.push((format!("exports/terrain/{name}.glb"), glb));
        }
    }

    let manifest = serde_json::json!({
        "schema_version": 1,
        "generator": format!("mybuilder {}", env!("CARGO_PKG_VERSION")),
        "seed": format!("{:016x}", project.seed),
        "extent": world.extent,
        "cell_size_m": grid.cell,
        "chunk_size_m": world.chunk_size_m,
        "mesh_step_cells": world.export.mesh_step_cells,
        "sea_level_m": world.sea_level_m,
        "h_min": hf.h_min,
        "h_max": hf.h_max,
        "layers": "layers/layers.json",
        "chunks": chunk_entries,
        "totals": {
            "chunks": ncx * ncz,
            "vertices": total_v,
            "triangles": total_t,
        }
    });
    files.push((MANIFEST_PATH.to_string(), pretty(&manifest)));

    FileBundle { files }
}

fn pretty(v: &serde_json::Value) -> Vec<u8> {
    let mut s = serde_json::to_string_pretty(v).expect("json");
    s.push('\n');
    s.into_bytes()
}

/// Hillshade preview PNG, one pixel per 2 m, Lambert shading with a
/// literal normalized light direction (no trig). Water is blue-tinted by
/// depth; land is a hypsometric tint scaled by shade.
fn hillshade_png(grid: &Grid, hf: &HeightField, sea: f64) -> Vec<u8> {
    const L: [f64; 3] = [0.5, std::f64::consts::FRAC_1_SQRT_2, 0.5]; // normalized
    let pxw = ((grid.nx - 1) as f64 * grid.cell / 2.0) as usize;
    let pxd = ((grid.nz - 1) as f64 * grid.cell / 2.0) as usize;
    let norms = normals(grid, hf);
    let mut rgb = Vec::with_capacity(pxw * pxd * 3);
    let span = if hf.h_max > hf.h_min {
        hf.h_max - hf.h_min
    } else {
        1.0
    };
    for pj in 0..pxd {
        for pi in 0..pxw {
            // Pixel center world coords -> nearest grid node.
            let x = grid.origin_x + (pi as f64 + 0.5) * 2.0;
            let z = grid.origin_z + (pj as f64 + 0.5) * 2.0;
            let i = ((x - grid.origin_x) / grid.cell).round() as usize;
            let j = ((z - grid.origin_z) / grid.cell).round() as usize;
            let i = i.min(grid.nx - 1);
            let j = j.min(grid.nz - 1);
            let idx = j * grid.nx + i;
            let n = norms[idx];
            let h = hf.h[idx];
            let shade =
                (n[0] as f64 * L[0] + n[1] as f64 * L[1] + n[2] as f64 * L[2]).clamp(0.0, 1.0);
            let (r, g, b) = if h < sea {
                let depth = ((sea - h) / 40.0).clamp(0.0, 1.0);
                (
                    0.05 + 0.10 * (1.0 - depth),
                    0.20 + 0.25 * (1.0 - depth),
                    0.45 + 0.30 * (1.0 - depth),
                )
            } else {
                let t = ((h - hf.h_min) / span).clamp(0.0, 1.0);
                let (r, g, b) = (0.20 + 0.65 * t, 0.40 + 0.45 * t, 0.20 + 0.55 * t);
                (r * shade, g * shade, b * shade)
            };
            rgb.push((r.clamp(0.0, 1.0) * 255.0).round() as u8);
            rgb.push((g.clamp(0.0, 1.0) * 255.0).round() as u8);
            rgb.push((b.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, pxw as u32, pxd as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().expect("png header");
    w.write_image_data(&rgb).expect("png data");
    drop(w);
    out
}

pub struct WriteReport {
    pub written: Vec<String>,
    pub unchanged: Vec<String>,
}

/// Write a bundle into the project dir. A file is written only if its
/// bytes differ from what is already on disk (blake3 compare). The
/// manifest is written last, atomically via temp file + rename.
pub fn write_outputs(dir: &Path, bundle: &FileBundle) -> std::io::Result<WriteReport> {
    let mut report = WriteReport {
        written: Vec::new(),
        unchanged: Vec::new(),
    };
    // Everything except the manifest, in bundle order.
    for (rel, data) in &bundle.files {
        if rel == MANIFEST_PATH {
            continue;
        }
        write_one(dir, rel, data, &mut report, false)?;
    }
    // Manifest last, atomic.
    if let Some((rel, data)) = bundle.files.iter().find(|(r, _)| r == MANIFEST_PATH) {
        write_one(dir, rel, data, &mut report, true)?;
    }
    Ok(report)
}

fn write_one(
    dir: &Path,
    rel: &str,
    data: &[u8],
    report: &mut WriteReport,
    atomic: bool,
) -> std::io::Result<()> {
    let path = dir.join(rel);
    let unchanged = std::fs::read(&path)
        .map(|old| mb_seed::hash_bytes(&old) == mb_seed::hash_bytes(data))
        .unwrap_or(false);
    if unchanged {
        report.unchanged.push(rel.to_string());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if atomic {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, &path)?;
    } else {
        std::fs::write(&path, data)?;
    }
    report.written.push(rel.to_string());
    Ok(())
}
