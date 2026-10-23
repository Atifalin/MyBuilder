//! Pipeline stages for world terrain.
//!
//! Artifact binary layouts (all little-endian, row-major z):
//! - `world.masks`: u64 nx, u64 nz, then nx*nz u8 planes land, mountain,
//!   forest in that order.
//! - `world.distance`: u64 nx, u64 nz, then two nx*nz f64 planes: signed
//!   coast distance and inside-mountain distance (meters).
//! - `world.height`: u64 nx, u64 nz, f64 h_min, f64 h_max, then nx*nz
//!   f64 heights, then nx*nz f64 coast distances (the export stage needs
//!   d for vertex colors but does not depend on world.distance per the
//!   stage DAG).
//! - `world.export`: a [`outputs::FileBundle`] with every output file.
//!
//! Stage params are canonical JSON of the recipe slice the stage reads
//! plus the cell size, so a recipe edit invalidates exactly what depends
//! on it.

use mb_pipeline::{Artifact, Stage};
use mb_schema::{ProjectFile, WorldRecipe};
use serde::Serialize;

use crate::outputs::{self, FileBundle};
use crate::{Grid, HeightField, HeightInputs, Masks, build_distances, build_height, build_masks};

fn put_u64(b: &mut Vec<u8>, v: u64) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn put_f64(b: &mut Vec<u8>, v: f64) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn get_u64(b: &[u8], o: &mut usize) -> u64 {
    let v = u64::from_le_bytes(b[*o..*o + 8].try_into().unwrap());
    *o += 8;
    v
}
fn get_f64s(b: &[u8], o: &mut usize, n: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(f64::from_le_bytes(b[*o..*o + 8].try_into().unwrap()));
        *o += 8;
    }
    out
}

fn encode_masks(grid: &Grid, m: &Masks) -> Artifact {
    let mut b = Vec::with_capacity(16 + 3 * grid.len());
    put_u64(&mut b, grid.nx as u64);
    put_u64(&mut b, grid.nz as u64);
    b.extend_from_slice(&m.land);
    b.extend_from_slice(&m.mountain);
    b.extend_from_slice(&m.forest);
    Artifact::new(b)
}

fn decode_masks(b: &[u8]) -> (Grid, Masks) {
    let mut o = 0;
    let nx = get_u64(b, &mut o) as usize;
    let nz = get_u64(b, &mut o) as usize;
    let n = nx * nz;
    let land = b[o..o + n].to_vec();
    let mountain = b[o + n..o + 2 * n].to_vec();
    let forest = b[o + 2 * n..o + 3 * n].to_vec();
    // Grid dims decode without world-space origin; callers that need
    // coordinates pass their own Grid. Only masks rasterization needs
    // geometry upstream.
    (
        Grid {
            nx,
            nz,
            cell: 1.0,
            origin_x: 0.0,
            origin_z: 0.0,
        },
        Masks {
            land,
            mountain,
            forest,
        },
    )
}

fn encode_distance(grid: &Grid, d: &crate::Distances) -> Artifact {
    let mut b = Vec::with_capacity(16 + 16 * grid.len());
    put_u64(&mut b, grid.nx as u64);
    put_u64(&mut b, grid.nz as u64);
    for &v in &d.coast {
        put_f64(&mut b, v);
    }
    for &v in &d.mountain {
        put_f64(&mut b, v);
    }
    Artifact::new(b)
}

fn decode_distance(b: &[u8]) -> (usize, usize, Vec<f64>, Vec<f64>) {
    let mut o = 0;
    let nx = get_u64(b, &mut o) as usize;
    let nz = get_u64(b, &mut o) as usize;
    let coast = get_f64s(b, &mut o, nx * nz);
    let mountain = get_f64s(b, &mut o, nx * nz);
    (nx, nz, coast, mountain)
}

fn encode_height(grid: &Grid, hf: &HeightField, coast: &[f64]) -> Artifact {
    let mut b = Vec::with_capacity(32 + 16 * grid.len());
    put_u64(&mut b, grid.nx as u64);
    put_u64(&mut b, grid.nz as u64);
    put_f64(&mut b, hf.h_min);
    put_f64(&mut b, hf.h_max);
    for &v in &hf.h {
        put_f64(&mut b, v);
    }
    for &v in coast {
        put_f64(&mut b, v);
    }
    Artifact::new(b)
}

/// Returns (nx, nz, h_min, h_max, heights, coast).
fn decode_height(b: &[u8]) -> (usize, usize, f64, f64, Vec<f64>, Vec<f64>) {
    let mut o = 0;
    let nx = get_u64(b, &mut o) as usize;
    let nz = get_u64(b, &mut o) as usize;
    let h_min = f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    o += 8;
    let h_max = f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    o += 8;
    let h = get_f64s(b, &mut o, nx * nz);
    let coast = get_f64s(b, &mut o, nx * nz);
    (nx, nz, h_min, h_max, h, coast)
}

/// Shared per-stage parameter payload: canonical JSON bytes.
fn params<T: Serialize>(v: &T) -> Vec<u8> {
    serde_json::to_vec(v).expect("stage params serialize")
}

// ---------------------------------------------------------------------

pub struct MasksStage {
    pub code_version: u32,
    pub grid: Grid,
    pub mode: mb_schema::WorldMode,
    pub shapes: Vec<mb_schema::Shape>,
    pub cell_size_m: f64,
    pub extent: mb_schema::Extent,
    pub generator_version: u32,
}

impl MasksStage {
    pub const ID: &'static str = "world.masks";
}

impl Stage for MasksStage {
    fn id(&self) -> &str {
        Self::ID
    }
    fn deps(&self) -> &[String] {
        &[]
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({
            "extent": self.extent,
            "cell_size_m": self.cell_size_m,
            "mode": self.mode,
            "shapes": self.shapes,
            "generator_version": self.generator_version,
        }))
    }
    fn run(&self, _inputs: &[&Artifact], _seed: u64) -> Artifact {
        encode_masks(
            &self.grid,
            &build_masks(&self.grid, &self.shapes, self.mode),
        )
    }
}

pub struct DistanceStage {
    pub code_version: u32,
    pub grid: Grid,
    pub cell_size_m: f64,
    deps: Vec<String>,
}

impl DistanceStage {
    pub const ID: &'static str = "world.distance";
    pub fn new(grid: Grid, cell_size_m: f64, code_version: u32) -> Self {
        DistanceStage {
            code_version,
            grid,
            cell_size_m,
            deps: vec![MasksStage::ID.to_string()],
        }
    }
}

impl Stage for DistanceStage {
    fn id(&self) -> &str {
        Self::ID
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({ "cell_size_m": self.cell_size_m }))
    }
    fn run(&self, inputs: &[&Artifact], _seed: u64) -> Artifact {
        let (_g, masks) = decode_masks(&inputs[0].bytes);
        encode_distance(&self.grid, &build_distances(&self.grid, &masks))
    }
}

pub struct HeightStage {
    pub code_version: u32,
    pub grid: Grid,
    pub project: ProjectFile,
    pub world: WorldRecipe,
    deps: Vec<String>,
}

impl HeightStage {
    pub const ID: &'static str = "world.height";
    pub fn new(grid: Grid, project: ProjectFile, world: WorldRecipe, code_version: u32) -> Self {
        HeightStage {
            code_version,
            grid,
            project,
            world,
            deps: vec![DistanceStage::ID.to_string(), MasksStage::ID.to_string()],
        }
    }
}

impl Stage for HeightStage {
    fn id(&self) -> &str {
        Self::ID
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({
            "cell_size_m": self.project.world_scale.cell_size_m,
            "sea_level_m": self.world.sea_level_m,
            "terrain": self.world.terrain,
            "profile": self.world.profile,
            "seed": format!("{:016x}", self.project.seed),
            "generator_version": self.project.generator_version,
        }))
    }
    fn run(&self, inputs: &[&Artifact], _seed: u64) -> Artifact {
        let (_nx, _nz, coast, mountain) = decode_distance(&inputs[0].bytes);
        let (_g, masks) = decode_masks(&inputs[1].bytes);
        let dist = crate::Distances { coast, mountain };
        let terrain = self
            .world
            .terrain
            .feel
            .resolve(&self.world.terrain.overrides);
        let coast_for_artifact = dist.coast.clone();
        let hf = build_height(&HeightInputs {
            grid: &self.grid,
            masks: &masks,
            dist: &dist,
            sea_level_m: self.world.sea_level_m,
            project_seed: self.project.seed,
            terrain,
            profile: &self.world.profile,
        });
        encode_height(&self.grid, &hf, &coast_for_artifact)
    }
}

pub struct ExportStage {
    pub code_version: u32,
    pub grid: Grid,
    pub project: ProjectFile,
    pub world: WorldRecipe,
    deps: Vec<String>,
}

impl ExportStage {
    pub const ID: &'static str = "world.export";
    pub fn new(grid: Grid, project: ProjectFile, world: WorldRecipe, code_version: u32) -> Self {
        let deps = vec![HeightStage::ID.to_string(), MasksStage::ID.to_string()]
            .into_iter()
            .chain(world.detail_regions.iter().map(|r| region_stage_id(&r.id)))
            .collect();
        ExportStage {
            code_version,
            grid,
            project,
            world,
            deps,
        }
    }
}

/// Stage id of the per-region stage.
pub fn region_stage_id(id: &str) -> String {
    format!("world.region.{id}")
}

/// One stage per detail region: emits the region's fine chunk GLBs as a
/// FileBundle artifact. Depends on height (for h and coast) and distance
/// (mountain distances feed height_at via the coarse planes — but the
/// height artifact already carries coast; mountain is needed too).
pub struct RegionStage {
    pub code_version: u32,
    pub grid: Grid,
    pub project: ProjectFile,
    pub world: WorldRecipe,
    pub region: mb_schema::DetailRegion,
    stage_id: String,
    deps: Vec<String>,
}

impl RegionStage {
    pub fn new(
        grid: Grid,
        project: ProjectFile,
        world: WorldRecipe,
        region: mb_schema::DetailRegion,
        code_version: u32,
    ) -> Self {
        let stage_id = region_stage_id(&region.id);
        RegionStage {
            code_version,
            grid,
            project,
            world,
            region,
            stage_id,
            deps: vec![
                HeightStage::ID.to_string(),
                DistanceStage::ID.to_string(),
                MasksStage::ID.to_string(),
            ],
        }
    }
}

impl Stage for RegionStage {
    fn id(&self) -> &str {
        // Boxed id storage: keep a leaked string? No — deps use a stored
        // String, but id() needs &str tied to self. Store the id string.
        // We store it in `region.id`-derived owned field added below.
        &self.stage_id
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({
            "region": self.region,
            "cell_size_m": self.project.world_scale.cell_size_m,
            "chunk_size_m": self.world.chunk_size_m,
            "mesh_step_cells": self.world.export.mesh_step_cells,
            "generator_version": self.project.generator_version,
            "seed": format!("{:016x}", self.project.seed),
        }))
    }
    fn run(&self, inputs: &[&Artifact], _seed: u64) -> Artifact {
        let (_nx, _nz, h_min, h_max, heights, coast) = decode_height(&inputs[0].bytes);
        let (_dnx, _dnz, dcoast, mountain) = decode_distance(&inputs[1].bytes);
        let (_g, masks) = decode_masks(&inputs[2].bytes);
        let hf = HeightField {
            h: heights,
            h_min,
            h_max,
        };
        let dist = crate::Distances {
            coast: dcoast,
            mountain,
        };
        let terrain = self
            .world
            .terrain
            .feel
            .resolve(&self.world.terrain.overrides);
        let ctx = crate::HeightCtx::new(
            self.grid,
            &dist,
            self.world.sea_level_m,
            self.project.seed,
            terrain,
            self.world.profile,
        );
        let _ = (masks, coast);
        let cells_per_chunk = (self.world.chunk_size_m / self.grid.cell) as usize;
        let coarse_step = self.world.export.mesh_step_cells as usize;
        let rb = crate::region::build_region_height(
            &ctx,
            &hf,
            &self.region,
            cells_per_chunk,
            coarse_step,
        );
        let normals_full = crate::mesh::normals(&self.grid, &hf);
        let rnorms = crate::region::region_normals(&ctx, &normals_full, &rb, coarse_step);
        let mut files = Vec::new();
        let mut entries = Vec::new();
        for cz in self.region.chunk_min[1]..=self.region.chunk_max[1] {
            for cx in self.region.chunk_min[0]..=self.region.chunk_max[0] {
                let (cx, cz) = (cx as usize, cz as usize);
                let mesh = crate::region::region_chunk_mesh(
                    &ctx,
                    &self.region,
                    cells_per_chunk,
                    &rb,
                    &rnorms,
                    self.world.sea_level_m,
                    self.world.profile.snow_line_m,
                    cx,
                    cz,
                );
                let key = format!("{cx}_{cz}");
                let id = mb_seed::EntityId::from_path(
                    self.project.seed,
                    &[
                        ("world", "main"),
                        ("region", &self.region.id),
                        ("chunk", &key),
                    ],
                );
                let name = format!("chunk_{cx}_{cz}");
                let glb = crate::glb::chunk_glb(&mesh, &name, &id.to_string(), cx, cz);
                let rel = format!("exports/terrain/regions/{}/{}.glb", self.region.id, name);
                entries.push(serde_json::json!({
                    "id": id.to_string(),
                    "cx": cx,
                    "cz": cz,
                    "region": self.region.id,
                    "file": format!("regions/{}/{}.glb", self.region.id, name),
                    "origin": mesh.origin,
                    "vertex_count": mesh.vertex_count,
                    "triangle_count": mesh.triangle_count,
                    "blake3": mb_seed::hex_bytes(&mb_seed::hash_bytes(&glb)),
                }));
                files.push((rel, glb));
            }
        }
        // The bundle also carries the manifest entry list as a JSON
        // pseudo-file the export stage consumes (never written to disk).
        files.push((
            format!("__manifest_entries__/{}", self.region.id),
            serde_json::to_vec(&entries).expect("entries"),
        ));
        FileBundle { files }.encode()
    }
}

impl Stage for ExportStage {
    fn id(&self) -> &str {
        Self::ID
    }
    fn deps(&self) -> &[String] {
        &self.deps
    }
    fn code_version(&self) -> u32 {
        self.code_version
    }
    fn params(&self) -> Vec<u8> {
        params(&serde_json::json!({
            "cell_size_m": self.project.world_scale.cell_size_m,
            "chunk_size_m": self.world.chunk_size_m,
            "export": self.world.export,
            "sea_level_m": self.world.sea_level_m,
            "seed": format!("{:016x}", self.project.seed),
            "generator_version": self.project.generator_version,
        }))
    }
    fn run(&self, inputs: &[&Artifact], _seed: u64) -> Artifact {
        let (_nx, _nz, h_min, h_max, heights, coast) = decode_height(&inputs[0].bytes);
        let (_g, masks) = decode_masks(&inputs[1].bytes);
        let hf = HeightField {
            h: heights,
            h_min,
            h_max,
        };
        // inputs[2..] are region bundles in detail_regions order.
        let mut region_outputs = Vec::new();
        for (i, region) in self.world.detail_regions.iter().enumerate() {
            let rb = FileBundle::decode(&inputs[2 + i].bytes);
            let marker = format!("__manifest_entries__/{}", region.id);
            let mut entries = serde_json::Value::Null;
            let mut files = Vec::new();
            for (rel, data) in rb.files {
                if rel == marker {
                    entries = serde_json::from_slice(&data).expect("region entries");
                } else {
                    files.push((rel, data));
                }
            }
            region_outputs.push(outputs::RegionOutput {
                region: region.clone(),
                entries,
                files,
            });
        }
        let bundle = outputs::build_export_bundle(
            &self.grid,
            &self.project,
            &self.world,
            &hf,
            &masks,
            &coast,
            &region_outputs,
        );
        FileBundle::encode(&bundle)
    }
}
